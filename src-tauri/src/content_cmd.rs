//! 页面理解层命令（阶段 1.1 · Content Layer）
//!
//! `content_snapshot`：从活动标签的实时 DOM 提取语义快照（本地执行，不出网）
//! `content_extract`：正文提取（复用 reader.rs 的 curl+readability 管线）

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use tauri::{AppHandle, Manager};

/// 提取活动标签的语义快照（供 AI 理解页面）。
/// 全程本地：webview 内 JS 提取 DOM → 结构化 JSON，不抓 URL、不出网。
#[tauri::command]
pub async fn content_snapshot(app: AppHandle) -> Result<crate::engine::content::Snapshot, String> {
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return Err("标签管理器未初始化".to_string());
    };
    let id = state
        .runtime_snapshot()
        .1
        .ok_or_else(|| "没有活动标签".to_string())?;
    let Some(window) = app.get_window("main") else {
        return Err("主窗口不存在".to_string());
    };
    let label = format!("tab-{}", id);
    let Some(wv) = window.get_webview(&label) else {
        return Err(format!("标签 {} 的 webview 不存在", id));
    };

    // 只发起提取（with_webview 闭包内不能阻塞等待，否则主线程死锁卡 UI）；
    // 结果经 channel 异步回来，在闭包外（async 线程）等待
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::content::extract_snapshot_async(&pwv.inner(), tx);
    })
    .map_err(|e| format!("提取失败: {}", e))?;

    let snap = rx
        .recv_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::WEBVIEW_IPC_SECS,
        ))
        .map_err(|e| format!("快照提取超时: {}", e))??;

    log::info!(
        "[content] 语义快照: tab={} title={} 链接={} 按钮={} 表单={}",
        id,
        snap.title,
        snap.interactive.links,
        snap.interactive.buttons,
        snap.forms.len()
    );
    Ok(snap)
}

/// 正文提取（content/extract）：复用 reader.rs 的抓取+readability 管线。
/// 注意：这是出网操作（抓 URL），由调用方确保用户已授权。
#[tauri::command]
pub async fn content_extract(url: String) -> Result<ContentExtract, String> {
    let product = crate::reader::scrape_article_public(&url).await?;
    Ok(ContentExtract {
        title: product.title,
        url: url.clone(),
        content: product.content.chars().take(6000).collect::<String>(),
        text: product.text.chars().take(4000).collect::<String>(),
    })
}

/// 计算页面对 AI 的友好度（基于语义快照的无障碍树 + 表单可访问性）
#[tauri::command]
pub async fn ai_friendliness(
    app: AppHandle,
) -> Result<crate::engine::content::AiFriendlinessReport, String> {
    let snap = content_snapshot(app.clone()).await?;
    Ok(crate::engine::content::ai_friendliness(&snap))
}

/// /// 正文提取结果
#[derive(Debug, serde::Serialize)]
pub struct ContentExtract {
    pub title: String,
    pub url: String,
    pub content: String,
    pub text: String,
}

/// 阅读摘要提示词组装结果（只组文本，不调模型，由调用方经 ai_chat 发送）
#[derive(Debug, serde::Serialize)]
pub struct ContentSummaryPrompt {
    pub title: String,
    pub url: String,
    pub prompt: String,
    pub chars: usize,
    pub truncated: bool,
}

/// 组装阅读摘要提示词：复用 reader 抓取管线（含 SSRF 防护），再套单源模板。
/// 出网操作，调用方须确保用户已授权。中文阈值按字符数判定，避免字节口径误杀中文页。
#[tauri::command]
pub async fn content_summary_prompt(url: String) -> Result<ContentSummaryPrompt, String> {
    let product = crate::reader::scrape_article_public(&url).await?;
    // 正文优先用纯文本，缺失时退回净化后 HTML 去标签的粗提取
    let raw = if product.text.trim().is_empty() {
        product.content.clone()
    } else {
        product.text.clone()
    };
    let chars = raw.chars().count();
    if chars < 100 {
        return Err("提取内容太短，可能不是文章页".to_string());
    }
    let prompt = crate::content_prompts::reader_summary(&product.title, &url, &raw);
    let truncated = chars > crate::content_prompts::SUMMARY_CHARS;
    Ok(ContentSummaryPrompt {
        title: product.title,
        url: url.clone(),
        prompt,
        chars,
        truncated,
    })
}

/// 问答提示词组装结果（纯组装，不读页面，由调用方按选区→正文→快照三级提供内容）
#[derive(Debug, serde::Serialize)]
pub struct ContentQaPrompt {
    pub prompt: String,
    pub chars: usize,
    pub truncated: bool,
}

/// 组装快照问答提示词：调用方按三级降级传内容（选区优先，其次正文全文，最后语义快照），
/// 本命令只套单源模板并回传截断标记，前端据此显示来源与是否读全。
/// source 取值建议：选区/正文全文/语义快照；coverage 如全文/已截断/含跨源读不到。
#[tauri::command]
pub async fn content_qa_prompt(
    title: String,
    url: String,
    source: String,
    coverage: String,
    content: String,
    question: String,
) -> Result<ContentQaPrompt, String> {
    if question.trim().is_empty() {
        return Err("问题不能为空".to_string());
    }
    if content.trim().is_empty() {
        return Err("页面数据为空，无法组装问答提示词".to_string());
    }
    let chars = content.chars().count();
    let prompt =
        crate::content_prompts::snapshot_qa(&title, &url, &source, &coverage, &content, &question);
    let truncated = chars > crate::content_prompts::CONTENT_BUDGET_CHARS;
    Ok(ContentQaPrompt {
        prompt,
        chars,
        truncated,
    })
}

/// 订阅去重条目（调用方传 id/标题/来源，来自 feed_items 查询结果）
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FeedDigestItem {
    pub id: String,
    pub title: String,
    pub source: String,
}

/// 组装订阅去重提示词：纯组装，不读库，由调用方传最新条目（建议不超 50 条）。
/// 调度器与启动页渲染不动，本命令只套单源模板，调用方经 ai_chat 发送。
#[tauri::command]
pub async fn feed_dedup_prompt(items: Vec<FeedDigestItem>) -> Result<String, String> {
    if items.is_empty() {
        return Err("订阅条目为空，无法组装去重提示词".to_string());
    }
    if items.len() > 50 {
        return Err("单次去重最多 50 条，请分页".to_string());
    }
    let triples: Vec<(String, String, String)> = items
        .into_iter()
        .map(|i| (i.id, i.title, i.source))
        .collect();
    Ok(crate::content_prompts::feed_dedup(&triples))
}

/// 组装订阅一句话推荐提示词：纯组装，单条调用，调用方经 ai_chat 发送。
#[tauri::command]
pub async fn feed_recommend_prompt(
    title: String,
    source: String,
    summary: String,
) -> Result<String, String> {
    if title.trim().is_empty() {
        return Err("标题不能为空".to_string());
    }
    Ok(crate::content_prompts::feed_recommend(
        &title, &source, &summary,
    ))
}

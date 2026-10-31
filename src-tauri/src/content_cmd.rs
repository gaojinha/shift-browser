// LINES-EXEMPT: 页面理解层命令：content_snapshot / content_extract 的出入参契约为同一组，拆开后契约核对要跨文件，风险大于收益
//! 页面理解层命令（阶段 1.1 · Content Layer）
//!
//! `content_snapshot`：从活动标签的实时 DOM 提取语义快照（本地执行，不出网）
//! `content_extract`：正文提取（复用 reader.rs 的 curl+readability 管线）

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use crate::error::CommandError;
use tauri::{AppHandle, Manager};

/// 提取活动标签的语义快照（供 AI 理解页面）。
/// 全程本地：webview 内 JS 提取 DOM → 结构化 JSON，不抓 URL、不出网。
#[tauri::command]
pub async fn content_snapshot(
    app: AppHandle,
) -> Result<crate::engine::content::Snapshot, CommandError> {
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return Err(CommandError {
            code: crate::error::E_INTERNAL,
            message: "标签管理器未初始化".to_string(),
        });
    };
    let id = state.runtime_snapshot().1.ok_or_else(|| CommandError {
        code: crate::error::E_VALIDATION,
        message: "没有活动标签".to_string(),
    })?;
    let Some(window) = app.get_window("main") else {
        return Err(CommandError {
            code: crate::error::E_INTERNAL,
            message: "主窗口不存在".to_string(),
        });
    };
    let label = format!("tab-{}", id);
    let Some(wv) = window.get_webview(&label) else {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: format!("标签 {} 的 webview 不存在", id),
        });
    };

    // 只发起提取（with_webview 闭包内不能阻塞等待，否则主线程死锁卡 UI）；
    // 结果经 channel 异步回来，在闭包外（async 线程）等待
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::content::extract_snapshot_async(&pwv.inner(), tx);
    })
    .map_err(|e| CommandError {
        code: crate::error::E_NET,
        message: format!("提取失败: {}", e),
    })?;

    let snap = rx
        .recv_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::WEBVIEW_IPC_SECS,
        ))
        .map_err(|e| CommandError {
            code: crate::error::E_TIMEOUT,
            message: format!("快照提取超时: {}", e),
        })??;

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

/// CEF 版语义快照（P1-5 步 1）：单栈窗口内容页固定是 `shell::CONTENT_ID`，
/// 不走 `TabManager`（CEF 构建里没有），直接按 `CONTENT_ID` 取 `BROWSERS`
/// 句柄，经 `CefPageEval`（console 前缀路由）回收结果。
/// 全程本地：CEF 页面 JS 提取 DOM → 结构化 JSON，不抓 URL、不出网。
#[cfg(feature = "cef-engine")]
#[tauri::command]
pub async fn content_snapshot_cef() -> Result<crate::engine::content::Snapshot, CommandError> {
    use crate::engine::cef::eval::CefPageEval;
    use crate::engine::cef::shell::CONTENT_ID;
    use crate::engine::page_eval::PageEval;

    // 只发起（CEF UI 线程铁律：绝不能在回调里阻塞等结果），结果经 channel
    // 异步回来，在 async 线程（非 UI 线程）等待
    let (tx, rx) = std::sync::mpsc::channel();
    crate::engine::cef::cef_ui::cef_ui_call(move || {
        let eval = CefPageEval::new(CONTENT_ID);
        eval.eval_js_with(crate::engine::content::SNAPSHOT_JS, tx);
    });

    let snap_json = rx
        .recv_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::WEBVIEW_IPC_SECS,
        ))
        .map_err(|e| CommandError {
            code: crate::error::E_TIMEOUT,
            message: format!("CEF 快照提取超时: {e}"),
        })??;

    let snap: crate::engine::content::Snapshot =
        serde_json::from_str(&snap_json).map_err(|e| CommandError {
            code: crate::error::E_INTERNAL,
            message: format!("CEF 快照解析失败: {e}"),
        })?;

    log::info!(
        "[content] CEF 语义快照: title={} 链接={} 按钮={} 表单={}",
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
pub async fn content_extract(url: String) -> Result<ContentExtract, CommandError> {
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
) -> Result<crate::engine::content::AiFriendlinessReport, CommandError> {
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
pub async fn content_summary_prompt(url: String) -> Result<ContentSummaryPrompt, CommandError> {
    let product = crate::reader::scrape_article_public(&url).await?;
    // 正文优先用纯文本，缺失时退回净化后 HTML 去标签的粗提取
    let raw = if product.text.trim().is_empty() {
        product.content.clone()
    } else {
        product.text.clone()
    };
    let chars = raw.chars().count();
    if chars < 100 {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "提取内容太短，可能不是文章页".to_string(),
        });
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
) -> Result<ContentQaPrompt, CommandError> {
    if question.trim().is_empty() {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "问题不能为空".to_string(),
        });
    }
    if content.trim().is_empty() {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "页面数据为空，无法组装问答提示词".to_string(),
        });
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
pub async fn feed_dedup_prompt(items: Vec<FeedDigestItem>) -> Result<String, CommandError> {
    if items.is_empty() {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "订阅条目为空，无法组装去重提示词".to_string(),
        });
    }
    if items.len() > 50 {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "单次去重最多 50 条，请分页".to_string(),
        });
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
) -> Result<String, CommandError> {
    if title.trim().is_empty() {
        return Err(CommandError {
            code: crate::error::E_VALIDATION,
            message: "标题不能为空".to_string(),
        });
    }
    Ok(crate::content_prompts::feed_recommend(
        &title, &source, &summary,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 防漂移（R4-1）：content_qa_prompt 空参数必须归 E_VALIDATION
    /// （前端据此提示"请输入问题"，而不是当成系统故障）
    #[tokio::test]
    async fn content_qa_prompt_empty_question_is_validation() {
        let err = content_qa_prompt(
            "title".into(),
            "url".into(),
            "source".into(),
            "coverage".into(),
            "content".into(),
            "".into(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        assert_eq!(err.message, "问题不能为空");

        // 仅空白字符同样视为有效性错误
        let err_blank = content_qa_prompt(
            "title".into(),
            "url".into(),
            "source".into(),
            "coverage".into(),
            "content".into(),
            "   \t\n ".into(),
        )
        .await
        .unwrap_err();
        assert_eq!(err_blank.code, crate::error::E_VALIDATION);
        assert_eq!(err_blank.message, "问题不能为空");
    }

    /// content_qa_prompt：内容为空或纯空白时归 E_VALIDATION
    #[tokio::test]
    async fn content_qa_prompt_empty_content_is_validation() {
        let err = content_qa_prompt(
            "title".into(),
            "url".into(),
            "source".into(),
            "coverage".into(),
            "".into(),
            "页面讲了什么？".into(),
        )
        .await
        .unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        assert_eq!(err.message, "页面数据为空，无法组装问答提示词");

        let err_blank = content_qa_prompt(
            "title".into(),
            "url".into(),
            "source".into(),
            "coverage".into(),
            "  \r\n  ".into(),
            "页面讲了什么？".into(),
        )
        .await
        .unwrap_err();
        assert_eq!(err_blank.code, crate::error::E_VALIDATION);
        assert_eq!(err_blank.message, "页面数据为空，无法组装问答提示词");
    }

    /// content_qa_prompt：正常入参稳定拼接关键字段，不截断与截断标记正确
    #[tokio::test]
    async fn content_qa_prompt_constructs_stable_prompt_and_sets_truncated() {
        let res = content_qa_prompt(
            "测试文章".into(),
            "https://example.com/post".into(),
            "正文全文".into(),
            "全文".into(),
            "Rust 是一门系统级编程语言。".into(),
            "Rust 适合做什么？".into(),
        )
        .await
        .unwrap();

        assert_eq!(res.chars, "Rust 是一门系统级编程语言。".chars().count());
        assert!(!res.truncated);
        // 关键子串均存在于组装后的 prompt 中
        assert!(res.prompt.contains("测试文章"));
        assert!(res.prompt.contains("https://example.com/post"));
        assert!(res.prompt.contains("正文全文"));
        assert!(res.prompt.contains("Rust 是一门系统级编程语言。"));
        assert!(res.prompt.contains("Rust 适合做什么？"));

        // 超过预算字符时标记 truncated
        let long_content = "X".repeat(crate::content_prompts::CONTENT_BUDGET_CHARS + 1);
        let res_long = content_qa_prompt(
            "t".into(),
            "u".into(),
            "s".into(),
            "c".into(),
            long_content,
            "q".into(),
        )
        .await
        .unwrap();
        assert!(res_long.truncated);
    }

    /// feed_dedup_prompt：空 items 归 E_VALIDATION
    #[tokio::test]
    async fn feed_dedup_prompt_empty_items_is_validation() {
        let err = feed_dedup_prompt(vec![]).await.unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        assert_eq!(err.message, "订阅条目为空，无法组装去重提示词");
    }

    /// feed_dedup_prompt：超过 50 条拦截归 E_VALIDATION
    #[tokio::test]
    async fn feed_dedup_prompt_over_50_items_is_validation() {
        let items = (0..51)
            .map(|i| FeedDigestItem {
                id: format!("id-{}", i),
                title: format!("Title {}", i),
                source: "RSS".into(),
            })
            .collect();
        let err = feed_dedup_prompt(items).await.unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        assert_eq!(err.message, "单次去重最多 50 条，请分页");
    }

    /// feed_dedup_prompt：正常入参拼接稳定，所有条目关键字段存在
    #[tokio::test]
    async fn feed_dedup_prompt_stable_content() {
        let items = vec![
            FeedDigestItem {
                id: "item-1".into(),
                title: "第一条资讯".into(),
                source: "源A".into(),
            },
            FeedDigestItem {
                id: "item-2".into(),
                title: "第二条资讯".into(),
                source: "源B".into(),
            },
        ];
        let prompt = feed_dedup_prompt(items).await.unwrap();
        assert!(prompt.contains("item-1"));
        assert!(prompt.contains("第一条资讯"));
        assert!(prompt.contains("源A"));
        assert!(prompt.contains("item-2"));
        assert!(prompt.contains("第二条资讯"));
        assert!(prompt.contains("源B"));
    }

    /// feed_recommend_prompt：空标题 / 纯空白标题归 E_VALIDATION
    #[tokio::test]
    async fn feed_recommend_prompt_empty_title_is_validation() {
        let err = feed_recommend_prompt("".into(), "源A".into(), "简介".into())
            .await
            .unwrap_err();
        assert_eq!(err.code, crate::error::E_VALIDATION);
        assert_eq!(err.message, "标题不能为空");

        let err_blank = feed_recommend_prompt("   ".into(), "源A".into(), "简介".into())
            .await
            .unwrap_err();
        assert_eq!(err_blank.code, crate::error::E_VALIDATION);
        assert_eq!(err_blank.message, "标题不能为空");
    }

    /// feed_recommend_prompt：正常拼接各关键字段
    #[tokio::test]
    async fn feed_recommend_prompt_stable_content() {
        let prompt = feed_recommend_prompt(
            "Rust 1.85 发布".into(),
            "Rust Blog".into(),
            "本次版本带来全新 Edition 支持".into(),
        )
        .await
        .unwrap();
        assert!(prompt.contains("Rust 1.85 发布"));
        assert!(prompt.contains("Rust Blog"));
        assert!(prompt.contains("本次版本带来全新 Edition 支持"));
    }
}

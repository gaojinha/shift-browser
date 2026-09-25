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
        .recv_timeout(std::time::Duration::from_secs(crate::contracts::timeout::WEBVIEW_IPC_SECS))
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
pub async fn ai_friendliness(app: AppHandle) -> Result<crate::engine::content::AiFriendlinessReport, String> {
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

//! 标签导航命令（R1 二次拆分 · 2026-09-24）：
//! 整页翻译 / 导航 / 刷新 / 后退 / 前进 / 导航状态查询。

use super::super::*;
use crate::error::{AppError, CommandResult};
use tauri::Manager;

/// 翻译当前活动标签的整个页面（供前端「翻译当前页面」按钮调用）
/// 复用既有整页翻译脚本：脚本自包含（进度浮层 + 分批调 shift://__translate__/batch + 回填 DOM），
/// 需要直接访问 TabManager 的 active_id（该字段不对外暴露）
#[tauri::command]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn translate_current_page(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<()> {
    let id = state
        .read_state()
        .active_id
        .clone()
        .ok_or_else(|| AppError::validation("当前没有活动标签"))?;
    let label = format!("tab-{}", id);
    let wv = app
        .get_webview(&label)
        .ok_or_else(|| AppError::internal("活动标签的 webview 不存在"))?;
    wv.eval(crate::proto_shift::WHOLE_PAGE_TRANSLATE_JS)
        .map_err(|e| AppError::internal(format!("注入整页翻译脚本失败: {}", e)))?;
    log::info!("[translate] 已对标签 {} 注入整页翻译脚本", id);
    Ok(())
}

/// 导航到新 URL
#[tauri::command]
pub async fn navigate_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
    url: String,
) -> CommandResult<Option<Tab>> {
    // 隐私防护：导航前清洗跨站追踪参数（utm_*/fbclid/gclid/msclkid/spm 等）
    let url = clean_tracking_params(&url);
    // 用户直驱导航不拦内网（路由器/NAS 后台要能开），但日志必须脱敏，
    // 防路径/query/token 落盘（与导航守卫、open_and_read 同脱敏口径）
    log::info!(
        "[tabs] navigate_tab: id={} url={}",
        id,
        crate::redact_url(&url)
    );
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal(format!("标签 {} 的 webview 不存在", id)))?;

    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| AppError::validation(format!("URL 解析失败: {}", e)))?;
    // P2：导航同样做 scheme 白名单（与 create_tab 一致），防止注入的脚本导航到 file://
    match parsed_url.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains(crate::contracts::urls::NEWTAB_MARKER)
                && !url.contains(crate::contracts::urls::WORLD_NAV_FILE)
            {
                return Err(AppError::permission("不允许导航到本地文件").into());
            }
        }
        other => {
            return Err(AppError::permission(format!("不允许的 URL scheme: {}", other)).into())
        }
    }
    webview
        .navigate(parsed_url)
        .map_err(|e| AppError::internal(format!("导航失败: {}", e)))?;

    let mut tab = None;
    state.mutate(|s| {
        if let Some(t) = s.tabs.get_mut(&id) {
            t.url = url.clone();
            t.title = url.clone();
            tab = Some(t.clone());
        }
    });
    Ok(tab)
}

/// 刷新当前标签（Ctrl+R）
#[tauri::command]
pub async fn reload_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> CommandResult<Option<Tab>> {
    log::info!("[tabs] reload_tab: id={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal(format!("标签 {} 的 webview 不存在", id)))?;
    webview
        .with_webview(|pwv| {
            crate::engine::webview::reload(&pwv.inner());
        })
        .map_err(|e| AppError::internal(format!("刷新失败: {}", e)))?;
    Ok(state.read_state().tabs.get(&id).cloned())
}

#[tauri::command]
pub async fn go_back(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<Option<Tab>> {
    let id = state.read_state().active_id.clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_back: tab={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal(format!("标签 {} 的 webview 不存在", id)))?;
    // 修复（2026-09-22）：改用 WebKit 原生导航，不再 eval("history.back()")——
    // 后者受页面 CSP / 页面脚本重写 history 影响，会"点了没反应"
    webview
        .with_webview(|pwv| {
            webkit2gtk::WebViewExt::go_back(&pwv.inner());
        })
        .map_err(|e| AppError::internal(format!("后退失败: {}", e)))?;
    Ok(state.read_state().tabs.get(&id).cloned())
}

/// 前进（对当前活动标签执行原生 go_forward()）
#[tauri::command]
pub async fn go_forward(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<Option<Tab>> {
    let id = state.read_state().active_id.clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_forward: tab={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal(format!("标签 {} 的 webview 不存在", id)))?;
    // 修复（2026-09-22）：同 go_back，改用 WebKit 原生导航
    webview
        .with_webview(|pwv| {
            webkit2gtk::WebViewExt::go_forward(&pwv.inner());
        })
        .map_err(|e| AppError::internal(format!("前进失败: {}", e)))?;
    Ok(state.read_state().tabs.get(&id).cloned())
}

#[tauri::command]
pub async fn get_nav_state(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: Option<String>,
) -> CommandResult<NavState> {
    let id = match id {
        Some(i) => i,
        None => state
            .read_state()
            .active_id
            .clone()
            .ok_or_else(|| AppError::validation("无活动标签"))?,
    };
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| AppError::internal("webview 不存在"))?;
    let (tx, rx) = tokio::sync::oneshot::channel::<(bool, bool)>();
    webview
        .with_webview(move |pwv| {
            let _ = tx.send(crate::engine::webview::navigation_state(&pwv.inner()));
        })
        .map_err(|e| AppError::internal(e.to_string()))?;
    let (b, f) = match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
        Ok(Ok(v)) => v,
        _ => (false, false),
    };
    Ok(NavState {
        can_go_back: b,
        can_go_forward: f,
    })
}

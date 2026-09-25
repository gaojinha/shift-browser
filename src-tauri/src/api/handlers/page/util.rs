//! 页面读取共享件（R1 三次拆分 · 2026-09-25）：
//! 参数解析（bool_arg）、目标标签解析（target_label）、
//! webview 脚本执行原语（eval_value / probe_page）。

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 取布尔参数（true/false、"true"/"1"、数字 1 均可；缺省 false）
pub(super) fn bool_arg(req: &Value, key: &str) -> bool {
    match req.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true" | "yes"),
        _ => false,
    }
}

/// 解析目标标签的 webview label（`tab_id` 优先，缺省活动标签）。
/// 供 read_page / dismiss / extract / actions 等按标签读取的命令复用
/// （兄弟 handler 经门面 `page::target_label` 访问）。
pub(crate) fn target_label(app: &AppHandle, req: &Value) -> Result<String, Value> {
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return Err(AppError::internal("标签管理器不可用").error_json());
    };
    let all = state.all_tabs();
    let target = match req.get("tab_id").and_then(|v| v.as_str()) {
        Some(tid) if !tid.is_empty() => all.iter().find(|t| t.id == tid).ok_or_else(|| {
            AppError::validation(format!("标签不存在: {tid}（先 list_tabs 取 id）")).error_json()
        })?,
        _ => all
            .iter()
            .find(|t| t.is_active)
            .ok_or_else(|| AppError::validation("没有活动标签（可传 tab_id 指定标签）").error_json())?,
    };
    Ok(format!("tab-{}", target.id))
}

/// 在给定 webview 上执行脚本并取回解析后的 JSON（lazy 滚动扫掠用）。
pub(super) async fn eval_value(
    wv: &tauri::Webview,
    script: &str,
    timeout_secs: u64,
) -> Option<Value> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let s = script.to_string();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &s, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return None;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(Ok(raw))) => serde_json::from_str::<Value>(&raw).ok(),
        _ => None,
    }
}

/// 单次探测标签状态（open_and_read 等加载 / 最终 URL 复核用）。返回 page_status.js 的完整 JSON。
pub(super) async fn probe_page(app: &AppHandle, tab_id: &str) -> Option<Value> {
    let wv = app.get_webview(&format!("tab-{}", tab_id))?;
    let script = include_str!("../../../page_status.js");
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return None;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
        Ok(Ok(Ok(raw))) => serde_json::from_str::<Value>(&raw).ok(),
        _ => None,
    }
}

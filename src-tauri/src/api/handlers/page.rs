//! 页面交互 handler 门面（R1 三次拆分 · 2026-09-25）
//!
//! 主体按域拆分到 page/ 子模块：
//! 1. [`shot`]：标签截图 + 截图目录治理（保留最近 100 张）；
//! 2. [`read`]：read_page（多模式页面读取）/ read_status（就绪态元信息）；
//! 3. [`open_read`]：open_and_read（开标签→等就绪→SSRF 复核→读取）；
//! 4. [`util`]：bool_arg / target_label / eval_value / probe_page 共享件。
//!
//! 本门面保留两个直接操作活动标签的 handler：eval（受控 JS 执行）与
//! action/dismiss（保守弹窗关闭）。对 handlers/mod.rs 的分发面不变
//! （仍经 `page::xxx` 调用，下方 re-export 保持路径稳定）。

#[path = "page/shot.rs"]
mod shot;
#[path = "page/util.rs"]
mod util;
#[path = "page/read.rs"]
mod read;
#[path = "page/open_read.rs"]
mod open_read;

pub(crate) use open_read::open_and_read;
pub(crate) use read::{read_page, read_status};
pub(crate) use shot::screenshot;
pub(crate) use util::{bool_arg, target_label};

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

pub(super) async fn eval(app: &AppHandle, req: &Value) -> Value {
    // 执行 JS（安全闸门：设置 api_allow_eval 必须为 'true'，默认关）
    let db = app.state::<crate::db::Db>();
    let allowed: String = sqlx::query_as::<_, (String,)>(
        "SELECT value FROM settings WHERE key = 'api_allow_eval'",
    )
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()
    .map(|(v,)| v)
    .unwrap_or_default();
    if allowed != "true" {
        return AppError::permission("执行 JS 已禁用：需在设置里显式开启「允许 API 执行 JS」")
            .error_json();
    }
    let code = req
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if code.is_empty() {
        return AppError::validation("缺少 code").error_json();
    }
    if code.len() > 100_000 {
        return AppError::validation("代码过长（>100KB）").error_json();
    }
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let Some(active) = state.all_tabs().into_iter().find(|t| t.is_active) else {
        return AppError::validation("没有活动标签").error_json();
    };
    let label = format!("tab-{}", active.id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到活动标签 webview").error_json();
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &code, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("执行调度失败").error_json();
    }
    match rx.await {
        Ok(Ok(v)) => json!({"ok": true, "data": {"result": v}}),
        Ok(Err(e)) => AppError::internal(e).error_json(),
        Err(_) => AppError::internal("执行结果丢失").error_json(),
    }
}

/// `action/dismiss`：关闭常见弹窗（B8/H3）。保守识别（浮层内 close 语义/白名单文案），
/// 识别不了返回提示，**绝不盲点**。属交互（改变页面状态）→ Risk::Write，受 api_readonly 约束。
pub(super) async fn dismiss(app: &AppHandle, req: &Value) -> Value {
    let label = match util::target_label(app, req) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    let script = include_str!("../../page_dismiss.js");
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("弹窗识别调度失败").error_json();
    }
    let raw = match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => return AppError::internal(format!("弹窗识别执行失败: {e}")).error_json(),
        Ok(Err(_)) => return AppError::internal("弹窗识别结果丢失").error_json(),
        Err(_) => return AppError::timeout("弹窗识别超时").error_json(),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::internal(format!("弹窗识别结果解析失败: {e}")).error_json(),
    }
}

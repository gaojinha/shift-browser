//! 交互 handler（P2）：wait_for / scroll / click / fill / back
//!
//! 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.4）
//! 安全：固定脚本 page_action.js + 独立开关 `api_agent_actions`（默认关）+ 交互预检（I2）+
//!       每次交互现查 DOM（I10）。写类动作另受 `api_readonly` 闸门约束（Risk::Write）。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 交互总开关（默认关）：settings.api_agent_actions == "true"
async fn agent_actions_enabled(app: &AppHandle) -> bool {
    let db = app.state::<crate::db::Db>();
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'api_agent_actions'")
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false)
}

/// 注入 page_action.js 执行一次（解析后的 JSON；网络/超时为 Err 响应）
#[allow(clippy::too_many_arguments)]
async fn eval_action(
    app: &AppHandle,
    req: &Value,
    action: &str,
    selector: &str,
    index: i64,
    value: &str,
    direction: &str,
    amount: u64,
    condition: &str,
    key: &str,
    timeout_secs: u64,
) -> Result<Value, Value> {
    let label = super::page::target_label(app, req)?;
    let Some(wv) = app.get_webview(&label) else {
        return Err(AppError::internal("找不到标签 webview").error_json());
    };
    let script = include_str!("../../page_action.js")
        .replace("__ACTION__", action)
        .replace("__SELECTOR__", &crate::api::js_escape(selector))
        .replace("__INDEX__", &index.to_string())
        .replace("__VALUE__", &crate::api::js_escape(value))
        .replace("__DIRECTION__", direction)
        .replace("__AMOUNT__", &amount.to_string())
        .replace("__CONDITION__", condition)
        .replace("__KEY__", &crate::api::js_escape(key));
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return Err(AppError::internal("交互调度失败").error_json());
    }
    let raw = match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => {
            return Err(AppError::internal(format!("交互脚本执行失败: {e}")).error_json())
        }
        Ok(Err(_)) => return Err(AppError::internal("交互结果丢失").error_json()),
        Err(_) => return Err(AppError::timeout("交互超时").error_json()),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(v) => Ok(v),
        Err(e) => Err(AppError::internal(format!("交互结果解析失败: {e}")).error_json()),
    }
}

/// 组装结果：含 action_error 时 ok:false（错误码透传给调用方）
fn wrap(v: Value) -> Value {
    if v.get("action_error").is_some() {
        let code = v
            .get("action_error")
            .and_then(|c| c.as_str())
            .unwrap_or("action_error")
            .to_string();
        let msg = v
            .get("error")
            .and_then(|m| m.as_str())
            .unwrap_or("交互失败")
            .to_string();
        json!({"ok": false, "code": code, "error": msg, "data": v})
    } else {
        json!({"ok": true, "data": v})
    }
}

/// 开关未开时的统一拒绝
fn gated() -> Value {
    AppError::permission(
        "交互能力未开启：需在设置里开启「允许 API 页面交互」(api_agent_actions)",
    )
    .error_json()
}

/// `action/wait_for`：等待元素出现/消失或页面加载完成（超时返回 met:false）。
pub(super) async fn wait_for(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let condition = req
        .get("condition")
        .and_then(|v| v.as_str())
        .unwrap_or("appear");
    if !matches!(condition, "appear" | "gone" | "load") {
        return AppError::validation("condition 可选 appear / gone / load").error_json();
    }
    if condition != "load" && selector.trim().is_empty() {
        return AppError::validation("condition=appear/gone 时 selector 必填").error_json();
    }
    let timeout_ms = req
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(10_000)
        .min(60_000);
    let start = std::time::Instant::now();
    loop {
        match eval_action(app, req, "check", &selector, 0, "", "down", 0, condition, "", 5).await {
            Ok(v) => {
                if v.get("met").and_then(|b| b.as_bool()).unwrap_or(false) {
                    return json!({"ok": true, "data": {
                        "met": true,
                        "waited_ms": start.elapsed().as_millis() as u64,
                        "detail": v
                    }});
                }
            }
            Err(resp) => return resp,
        }
        let waited = start.elapsed().as_millis() as u64;
        if waited >= timeout_ms {
            return json!({"ok": true, "data": {
                "met": false, "waited_ms": waited, "hint": "等待超时"
            }});
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// `action/scroll`：滚动（up/down/top/bottom）。
pub(super) async fn scroll(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let direction = req
        .get("direction")
        .and_then(|v| v.as_str())
        .unwrap_or("down");
    if !matches!(direction, "up" | "down" | "top" | "bottom") {
        return AppError::validation("direction 可选 up/down/top/bottom").error_json();
    }
    let amount = req
        .get("amount")
        .and_then(|v| v.as_u64())
        .unwrap_or(600)
        .min(20_000);
    match eval_action(app, req, "scroll", "", 0, "", direction, amount, "", "", 5).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/click`：预检 → 点第 index 个匹配元素 → 返回是否发生导航。
pub(super) async fn click(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(app, req, "click", &selector, index, "", "down", 0, "", "", 10).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/fill`：预检 → 设值 + 触发 input/change（值只落字符串，不执行）。
pub(super) async fn fill(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let value = req.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(app, req, "fill", &selector, index, value, "down", 0, "", "", 10).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/back`：history.back()（交互撤销）。
pub(super) async fn back(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    match eval_action(app, req, "back", "", 0, "", "down", 0, "", "", 5).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/hover`：悬停（派发 mouseover/mouseenter/mousemove）——用于 hover 展开的菜单。
pub(super) async fn hover(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(app, req, "hover", &selector, index, "", "down", 0, "", "", 10).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/key`：键盘事件（Enter/Escape/Tab/ArrowDown 等）。可选先聚焦 selector 命中元素。
pub(super) async fn key(app: &AppHandle, req: &Value) -> Value {
    if !agent_actions_enabled(app).await {
        return gated();
    }
    let k = req
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if k.is_empty() {
        return AppError::validation("缺少 key（如 Enter/Escape/Tab/ArrowDown）").error_json();
    }
    if k.chars().count() > 32 {
        return AppError::validation("key 过长（>32 字符）").error_json();
    }
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(app, req, "key", &selector, index, "", "down", 0, "", &k, 5).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// selector 校验（同 read_page：非空 + ≤1024 字符）
fn required_selector(req: &Value) -> Result<String, Value> {
    let s = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if s.is_empty() {
        return Err(AppError::validation("缺少 selector").error_json());
    }
    if s.chars().count() > 1024 {
        return Err(AppError::validation("selector 过长（>1024 字符）").error_json());
    }
    Ok(s)
}

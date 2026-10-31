// LINES-EXEMPT: 9 个交互动作 handler 同域（selector/等待工具共用），拆分排期 R4（2026-10-14 红线卡标注）
//! 交互 handler（P2）：wait_for / scroll / click / fill / back
//!
//! 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.4）
//! 安全：固定脚本 page_action.js + 独立开关 `api_agent_actions`（默认关）+ 交互预检（I2）+
//!       每次交互现查 DOM（I10）。写类动作另受 `api_readonly` 闸门约束（Risk::Write）。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use super::page::bool_arg;
use crate::error::AppError;

// 交互闸（agent_actions）已收口至 `api::matrix::can()`（R4-3 包 A，2026-10-14）：
// 8 个交互 handler 的开关判定统一在入口闸执行，本文件不再持第二份判定。
// 现状例外保持：action/find（Risk=Read）与 action/dismiss（page.rs，历史不挂闸）不受此闸约束。

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
    // 扩展参数（缺口 1/2/4/5）：全部可选、缺省关闭；校验失败直接回 validation 错误
    let frames = frames_literal(req)?;
    let modifiers = modifiers_literal(req)?;
    let expect = expect_literal(req)?;
    // text 不可 trim：action/type 输入首尾空格必须保留（find 侧由 JS 归一化）
    let text = req.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.chars().count() > 4096 {
        return Err(AppError::validation("text 过长（>4096 字符）").error_json());
    }
    let text = text.to_string();
    let role = str_arg(req, "role", 64)?;
    let name = str_arg(req, "name", 256)?;
    let within = str_arg(req, "within", 1024)?;
    let nth = req.get("nth").and_then(|v| v.as_i64()).unwrap_or(0);
    let quiet_ms = req
        .get("quiet_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .min(10_000);
    let script = include_str!("../../page_action.js")
        .replace("__ACTION__", action)
        .replace("__SELECTOR__", &crate::api::js_escape(selector))
        .replace("__INDEX__", &index.to_string())
        .replace("__VALUE__", &crate::api::js_escape(value))
        .replace("__DIRECTION__", direction)
        .replace("__AMOUNT__", &amount.to_string())
        .replace("__CONDITION__", condition)
        .replace("__KEY__", &crate::api::js_escape(key))
        .replace(
            "__PIERCE__",
            if bool_arg(req, "pierce") { "1" } else { "0" },
        )
        .replace("__FRAMES__", &frames)
        .replace("__MODIFIERS__", &modifiers)
        .replace("__TEXT__", &crate::api::js_escape(&text))
        .replace("__ROLE__", &crate::api::js_escape(&role))
        .replace("__NAME__", &crate::api::js_escape(&name))
        .replace("__NTH__", &nth.to_string())
        .replace("__EXACT__", if bool_arg(req, "exact") { "1" } else { "0" })
        .replace("__WITHIN__", &crate::api::js_escape(&within))
        .replace(
            "__VISIBLE_ONLY__",
            if visible_only_arg(req) { "1" } else { "0" },
        )
        .replace("__EXPECT__", &expect)
        .replace("__QUIET_MS__", &quiet_ms.to_string())
        .replace("__CLEAR__", if bool_arg(req, "clear") { "1" } else { "0" });
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

/// `action/wait_for`：等待元素出现/消失或页面加载完成（超时返回 met:false）。
pub(super) async fn wait_for(app: &AppHandle, req: &Value) -> Value {
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let condition = req
        .get("condition")
        .and_then(|v| v.as_str())
        .unwrap_or("appear");
    if !matches!(
        condition,
        "appear" | "gone" | "load" | "text" | "enabled" | "idle"
    ) {
        return AppError::validation("condition 可选 appear/gone/load/text/enabled/idle")
            .error_json();
    }
    if matches!(condition, "appear" | "gone" | "text" | "enabled") && selector.trim().is_empty() {
        return AppError::validation(format!("condition={condition} 时 selector 必填"))
            .error_json();
    }
    // text 条件必须带合法 expect（预检在 Rust 侧完成，非法直接拒，不进轮询）
    if condition == "text" {
        if let Err(resp) = expect_literal(req) {
            return resp;
        }
        if req.get("expect").is_none() {
            return AppError::validation("condition=text 时 expect 必填（{op, value}）")
                .error_json();
        }
    }
    let timeout_ms = req
        .get("timeout")
        .and_then(|v| v.as_u64())
        .unwrap_or(10_000)
        .min(60_000);
    let start = std::time::Instant::now();
    loop {
        match eval_action(
            app, req, "check", &selector, 0, "", "down", 0, condition, "", 5,
        )
        .await
        {
            Ok(v) => {
                // 脚本级致命错误（如非法正则）直接透传，不再盲等
                if v.get("action_error").is_some() {
                    return wrap(v);
                }
                if v.get("met").and_then(|b| b.as_bool()).unwrap_or(false) {
                    idle_cleanup(app, req, condition).await;
                    return json!({"ok": true, "data": {
                        "met": true,
                        "waited_ms": start.elapsed().as_millis() as u64,
                        "detail": v
                    }});
                }
            }
            Err(resp) => {
                idle_cleanup(app, req, condition).await;
                return resp;
            }
        }
        let waited = start.elapsed().as_millis() as u64;
        if waited >= timeout_ms {
            idle_cleanup(app, req, condition).await;
            return json!({"ok": true, "data": {
                "met": false, "waited_ms": waited, "hint": "等待超时"
            }});
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

/// idle 条件结束后清理页面级计数器（恢复 fetch/XHR 原函数、断开 MutationObserver）。
/// 尽力而为：失败不影响主结果。非 idle 条件直接跳过。
async fn idle_cleanup(app: &AppHandle, req: &Value, condition: &str) {
    if condition != "idle" {
        return;
    }
    let _ = eval_action(
        app,
        req,
        "check",
        "",
        0,
        "",
        "down",
        0,
        "idle_cleanup",
        "",
        2,
    )
    .await;
}

/// `action/scroll`：滚动（up/down/top/bottom）。
pub(super) async fn scroll(app: &AppHandle, req: &Value) -> Value {
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
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(
        app, req, "click", &selector, index, "", "down", 0, "", "", 10,
    )
    .await
    {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/fill`：预检 → 设值 + 触发 input/change（值只落字符串，不执行）。
pub(super) async fn fill(app: &AppHandle, req: &Value) -> Value {
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let value = req.get("value").and_then(|v| v.as_str()).unwrap_or("");
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(
        app, req, "fill", &selector, index, value, "down", 0, "", "", 10,
    )
    .await
    {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/back`：history.back()（交互撤销）。
pub(super) async fn back(app: &AppHandle, req: &Value) -> Value {
    match eval_action(app, req, "back", "", 0, "", "down", 0, "", "", 5).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/hover`：悬停（派发 mouseover/mouseenter/mousemove）——用于 hover 展开的菜单。
pub(super) async fn hover(app: &AppHandle, req: &Value) -> Value {
    let selector = match required_selector(req) {
        Ok(s) => s,
        Err(resp) => return resp,
    };
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(
        app, req, "hover", &selector, index, "", "down", 0, "", "", 10,
    )
    .await
    {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// `action/key`：键盘事件（Enter/Escape/Tab/ArrowDown 等）。可选先聚焦 selector 命中元素。
pub(super) async fn key(app: &AppHandle, req: &Value) -> Value {
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

/// `action/type`（缺口 5）：逐字符输入（纯字符串，不执行）。
/// 可选 selector（+index/pierce/frames）：定位、聚焦、输入在同一脚本同步完成；
/// 不传 selector 时作用于当前焦点。input/textarea 走 native setter + InputEvent；
/// contenteditable 走 Selection/Range。密码框拒绝（password_refused）。Risk=Write，受双闸门。
pub(super) async fn type_text(app: &AppHandle, req: &Value) -> Value {
    let text = req.get("text").and_then(|v| v.as_str()).unwrap_or("");
    if text.is_empty() && !bool_arg(req, "clear") {
        return AppError::validation("缺少 text（或显式 clear:true）").error_json();
    }
    if text.chars().count() > 4096 {
        return AppError::validation("text 过长（>4096 字符）").error_json();
    }
    // selector 可选（≤1024）；不传则向当前焦点输入
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if selector.chars().count() > 1024 {
        return AppError::validation("selector 过长（>1024 字符）").error_json();
    }
    let index = req.get("index").and_then(|v| v.as_i64()).unwrap_or(0);
    match eval_action(
        app, req, "type", &selector, index, "", "down", 0, "", "", 10,
    )
    .await
    {
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

/// `action/find`（缺口 2/6）：只读定位——text / role+name / selector 三种瞄准，
/// 返回稳定 selector + 坐标；找不到附 diagnose（nearby/same_text/in_iframe，限量截断）。
/// Risk=Read：只读、不改页面，不挂 api_agent_actions（只读模式下脑也必须能"找"）。
pub(super) async fn find(app: &AppHandle, req: &Value) -> Value {
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if selector.chars().count() > 1024 {
        return AppError::validation("selector 过长（>1024 字符）").error_json();
    }
    let has_text = req
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let has_role = req
        .get("role")
        .and_then(|v| v.as_str())
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    if selector.is_empty() && !has_text && !has_role {
        return AppError::validation("需 text / role / selector 至少其一").error_json();
    }
    match eval_action(app, req, "find", &selector, 0, "", "down", 0, "", "", 10).await {
        Ok(v) => wrap(v),
        Err(resp) => resp,
    }
}

/// 扩展参数解析：字符串参数（trim + 字符数硬顶）
fn str_arg(req: &Value, key: &str, max_chars: usize) -> Result<String, Value> {
    let s = req
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if s.chars().count() > max_chars {
        return Err(AppError::validation(format!("{key} 过长（>{max_chars} 字符）")).error_json());
    }
    Ok(s)
}

/// 扩展参数解析：visible_only 缺省 true（与 find 语义一致；其余动作不传此参数，无影响）
fn visible_only_arg(req: &Value) -> bool {
    match req.get("visible_only") {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(1) != 0,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true" | "yes"),
        _ => true,
    }
}

/// frames 参数校验并序列化为 JS 数组字面量（数字=第 N 个 iframe，字符串=选择器；≤4 层）。
/// serde_json 序列化保证字符串转义安全，无注入面。
fn frames_literal(req: &Value) -> Result<String, Value> {
    let mut out: Vec<Value> = Vec::new();
    if let Some(f) = req.get("frames") {
        let arr = f.as_array().ok_or_else(|| {
            AppError::validation("frames 必须是数组（数字=第 N 个 iframe，字符串=选择器）")
                .error_json()
        })?;
        if arr.len() > 4 {
            return Err(AppError::validation("frames 层级过深（>4）").error_json());
        }
        for item in arr {
            match item {
                Value::Number(n) => {
                    let i = n.as_u64().ok_or_else(|| {
                        AppError::validation("frames 数字元素必须是非负整数").error_json()
                    })?;
                    if i > 99 {
                        return Err(AppError::validation("frames 序号过大（>99）").error_json());
                    }
                    out.push(json!(i));
                }
                Value::String(s) => {
                    if s.chars().count() > 256 {
                        return Err(
                            AppError::validation("frames 选择器元素过长（>256 字符）").error_json()
                        );
                    }
                    out.push(json!(s));
                }
                _ => return Err(AppError::validation("frames 元素只支持数字或字符串").error_json()),
            }
        }
    }
    Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".into()))
}

/// modifiers 参数校验并序列化为 JS 数组字面量（仅白名单四键）
fn modifiers_literal(req: &Value) -> Result<String, Value> {
    const ALLOWED: &[&str] = &["Control", "Shift", "Alt", "Meta"];
    let mut out: Vec<String> = Vec::new();
    if let Some(m) = req.get("modifiers") {
        let arr = m.as_array().ok_or_else(|| {
            AppError::validation("modifiers 必须是数组（如 [\"Control\",\"Shift\"]）").error_json()
        })?;
        for item in arr {
            let s = item
                .as_str()
                .ok_or_else(|| AppError::validation("modifiers 元素必须是字符串").error_json())?;
            if !ALLOWED.contains(&s) {
                return Err(AppError::validation(format!(
                    "modifiers 仅支持 Control/Shift/Alt/Meta，收到: {s}"
                ))
                .error_json());
            }
            out.push(s.to_string());
        }
    }
    Ok(serde_json::to_string(&out).unwrap_or_else(|_| "[]".into()))
}

/// wait_for text 条件的 expect 参数：{op: contains/equals/regex, value: string}。
/// 非 text 条件或未传时返回 null（JS 侧不用）。
fn expect_literal(req: &Value) -> Result<String, Value> {
    let Some(e) = req.get("expect") else {
        return Ok("null".into());
    };
    let obj = e
        .as_object()
        .ok_or_else(|| AppError::validation("expect 必须是对象 {op, value}").error_json())?;
    let op = obj.get("op").and_then(|v| v.as_str()).unwrap_or("contains");
    if !matches!(op, "contains" | "equals" | "regex") {
        return Err(AppError::validation("expect.op 可选 contains/equals/regex").error_json());
    }
    let value = obj
        .get("value")
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::validation("expect.value 必填（字符串）").error_json())?;
    if value.chars().count() > 512 {
        return Err(AppError::validation("expect.value 过长（>512 字符）").error_json());
    }
    Ok(serde_json::to_string(&json!({"op": op, "value": value})).unwrap_or_else(|_| "null".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn frames_literal_rejects_non_array() {
        let req = json!({"frames": "0"});
        assert!(frames_literal(&req).is_err());
    }

    #[test]
    fn frames_literal_accepts_mixed_path() {
        let req = json!({"frames": [0, "#login-box"]});
        assert_eq!(frames_literal(&req).unwrap(), "[0,\"#login-box\"]");
    }

    #[test]
    fn frames_literal_rejects_deep_nesting() {
        let req = json!({"frames": [0, 1, 2, 3, 4]});
        assert!(frames_literal(&req).is_err());
    }

    #[test]
    fn frames_literal_escapes_strings() {
        let req = json!({"frames": ["a'b\"c"]});
        let lit = frames_literal(&req).unwrap();
        assert_eq!(lit, "[\"a'b\\\"c\"]");
    }

    #[test]
    fn frames_literal_default_empty() {
        assert_eq!(frames_literal(&json!({})).unwrap(), "[]");
    }

    #[test]
    fn modifiers_literal_whitelist() {
        assert_eq!(
            modifiers_literal(&json!({"modifiers": ["Control", "Shift"]})).unwrap(),
            "[\"Control\",\"Shift\"]"
        );
        assert!(modifiers_literal(&json!({"modifiers": ["Win"]})).is_err());
        assert!(modifiers_literal(&json!({"modifiers": "Control"})).is_err());
        assert_eq!(modifiers_literal(&json!({})).unwrap(), "[]");
    }

    #[test]
    fn expect_literal_validation() {
        assert_eq!(expect_literal(&json!({})).unwrap(), "null");
        let ok = expect_literal(&json!({"expect": {"op": "regex", "value": "¥\\d+"}})).unwrap();
        assert!(ok.contains("\"op\":\"regex\""));
        assert!(expect_literal(&json!({"expect": {"op": "bad", "value": "x"}})).is_err());
        assert!(expect_literal(&json!({"expect": {"op": "contains"}})).is_err());
        assert!(expect_literal(&json!({"expect": "x"})).is_err());
    }

    #[test]
    fn visible_only_defaults_true() {
        assert!(visible_only_arg(&json!({})));
        assert!(visible_only_arg(&json!({"visible_only": true})));
        assert!(!visible_only_arg(&json!({"visible_only": false})));
        assert!(!visible_only_arg(&json!({"visible_only": 0})));
    }

    #[test]
    fn str_arg_caps_length() {
        assert!(str_arg(&json!({"role": "button"}), "role", 64).is_ok());
        assert!(str_arg(&json!({"role": "x".repeat(65)}), "role", 64).is_err());
    }

    #[test]
    fn wrap_action_error_maps_to_ok_false() {
        // action_error 在 → ok:false，错误码透传，原文保留，原始结构进 data
        let w = wrap(json!({"action_error": "password_refused", "error": "密码框受保护"}));
        assert_eq!(w["ok"], false);
        assert_eq!(w["code"], "password_refused");
        assert_eq!(w["error"], "密码框受保护");
        assert!(w["data"].is_object());
        // code 非字符串/缺失 → "action_error" 兜底；error 缺失 → "交互失败" 兜底
        let w2 = wrap(json!({"action_error": true}));
        assert_eq!(w2["code"], "action_error");
        assert_eq!(w2["error"], "交互失败");
    }

    #[test]
    fn wrap_plain_result_passthrough() {
        let w = wrap(json!({"clicked": true}));
        assert_eq!(w["ok"], true);
        assert_eq!(w["data"]["clicked"], true);
    }

    #[test]
    fn required_selector_validation() {
        // 缺失/纯空白 → E_VALIDATION；trim 后放行
        assert!(required_selector(&json!({})).is_err());
        assert!(required_selector(&json!({"selector": "   "})).is_err());
        assert_eq!(
            required_selector(&json!({"selector": " #buy "})).unwrap(),
            "#buy"
        );
        // 1024 字符硬顶（边界放行，超 1 拒）
        assert!(required_selector(&json!({"selector": "a".repeat(1024)})).is_ok());
        assert!(required_selector(&json!({"selector": "a".repeat(1025)})).is_err());
    }
}

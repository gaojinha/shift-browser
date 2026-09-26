//! 内容处理 handler（P1a）：结构化提取 / 阅读模式正文 / 翻译 / 语义快照
//!
//! 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.2 P1a）

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// extract 字段白名单：text / href / src / alt / title + data-*
fn field_allowed(f: &str) -> bool {
    matches!(f, "text" | "href" | "src" | "alt" | "title") || f.starts_with("data-")
}

/// `extract`：按 `selector` + `fields` 白名单做结构化提取（固定模板，非任意 JS）。
/// 参数：selector（必填）、fields（默认 ["text"]）、tab_id（可选）、limit（默认 500，硬顶 5000）。
pub(super) async fn extract(app: &AppHandle, req: &Value) -> Value {
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if selector.is_empty() {
        return AppError::validation("缺少 selector").error_json();
    }
    if selector.chars().count() > 1024 {
        return AppError::validation("selector 过长（>1024 字符）").error_json();
    }
    let fields: Vec<String> = req
        .get("fields")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_else(|| vec!["text".to_string()]);
    if let Some(bad) = fields.iter().find(|f| !field_allowed(f)) {
        return AppError::validation(format!(
            "不支持的字段: {bad}（可选 text/href/src/alt/title/data-*）"
        ))
        .error_json();
    }
    let limit = req
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .min(5000);

    let label = match super::page::target_label(app, req) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    let fields_json = serde_json::to_string(&fields).unwrap_or_else(|_| "[\"text\"]".to_string());
    let script = include_str!("../../page_extract.js")
        .replace("__SELECTOR__", &crate::api::js_escape(&selector))
        .replace("__FIELDS__", &fields_json)
        .replace("__LIMIT__", &limit.to_string());
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("提取调度失败").error_json();
    }
    let raw = match tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::READ_PAGE_SECS),
        rx,
    )
    .await
    {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => return AppError::internal(format!("提取脚本执行失败: {e}")).error_json(),
        Ok(Err(_)) => return AppError::internal("提取结果丢失").error_json(),
        Err(_) => return AppError::timeout("提取超时").error_json(),
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return AppError::internal(format!("提取结果解析失败: {e}")).error_json(),
    };
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return AppError::validation(err).error_json();
    }
    json!({"ok": true, "data": v})
}

/// 选区采集固定脚本（编译期嵌入；占位符 `__EXPAND__` / `__MAX_CHARS__` 为数字，无注入面）。
const SELECTION_JS: &str = include_str!("../../page_selection.js");

/// `read_selection` 参数解析（抽纯函数便于单测，对齐 actions.rs 范式）。
/// 返回 `(expand, max_chars)`：
/// - `expand` **缺省 true**（把选区碎片扩到段落；问"这段说的啥"需要语境）；
/// - `max_chars` 默认 4000，硬顶 20000（防单次采集撑爆上下文）。
fn selection_args(req: &Value) -> (bool, u64) {
    // bool_arg 缺省为 false，而此处缺省要 true，故先判存在性
    let expand = if req.get("expand").is_none() {
        true
    } else {
        super::page::bool_arg(req, "expand")
    };
    let max_chars = req
        .get("max_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(4000)
        .min(20_000);
    (expand, max_chars)
}

/// `read_selection`：取活动/指定标签的**当前选区**（纯只读，不改页面）。
///
/// P1「网页随手问 AI」上下文采集（docs/网页随手问AI-全方位规划_2026-09-26_v1.md §6.1）。
/// 参数：
/// - `tab_id`：可选，缺省活动标签（`target_label` 统一解析）；
/// - `expand`：可选，默认 `true`——把选区碎片扩到所属段落全文；
/// - `max_chars`：可选，默认 4000，硬顶 20000。
///
/// 返回（常态即 `has_selection=false`，**不报错**——没选区是正常状态）：
/// `has_selection / text / text_chars / block_text / block_chars / tag / truncated /
///   rejected / code / reject_reason / url / title`
///
/// 安全：选区落在密码/敏感区域**一律拒绝**且不返回任何内容（铁律 1.4），
/// 以 `rejected=true` + `code="secret_region"` 表达，属业务结果而非故障。
pub(super) async fn selection(app: &AppHandle, req: &Value) -> Value {
    let (expand, max_chars) = selection_args(req);
    let label = match super::page::target_label(app, req) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let Some(wv) = app.get_webview(&label) else {
        // 休眠标签：webview 已被销毁。这是**状态**不是故障——如实回报，
        // 绝不能让侧边栏把"标签睡着了"误报成"你没选东西"（随手问最易命中休眠标签）。
        // 范式对齐 read_status：ok:true + 机器可判的 hibernated + 可操作 hints。
        if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
            if let Some(t) = st
                .all_tabs()
                .iter()
                .find(|t| format!("tab-{}", t.id) == label)
            {
                if t.hibernated {
                    return json!({"ok": true, "data": {
                        "has_selection": false, "text": "", "text_chars": 0,
                        "block_text": "", "block_chars": 0, "tag": "", "truncated": false,
                        "expanded": expand, "rejected": false, "reject_reason": "",
                        "url": t.url, "title": t.title, "in_iframe": false,
                        "hibernated": true,
                        "hints": ["标签已休眠，webview 已销毁；切回该标签唤醒后可再读取选区"]
                    }});
                }
            }
        }
        return AppError::internal("找不到标签 webview").error_json();
    };
    // 固定脚本 + 占位符（EXPAND/MAX_CHARS 为数字，无 js_escape 面）
    let script = SELECTION_JS
        .replace("__EXPAND__", if expand { "1" } else { "0" })
        .replace("__MAX_CHARS__", &max_chars.to_string());
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("选区采集调度失败").error_json();
    }
    let raw = match tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::READ_PAGE_SECS),
        rx,
    )
    .await
    {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => {
            return AppError::internal(format!("选区采集脚本执行失败: {e}")).error_json()
        }
        Ok(Err(_)) => return AppError::internal("选区采集结果丢失").error_json(),
        Err(_) => return AppError::timeout("选区采集超时").error_json(),
    };
    let mut v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return AppError::internal(format!("选区采集结果解析失败: {e}")).error_json(),
    };
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return AppError::validation(err).error_json();
    }
    // 密码区域拒绝：补机器可判的 code（业务结果，仍 ok:true）
    if v.get("rejected").and_then(|b| b.as_bool()).unwrap_or(false) {
        v["code"] = Value::String("secret_region".to_string());
        v["has_selection"] = Value::Bool(false);
    }
    json!({"ok": true, "data": v})
}

/// `reader/extract`：给 URL 出净化正文（复用 `reader::fetch_article`，SSRF 链路已就绪）。
pub(super) async fn reader_extract(_app: &AppHandle, req: &Value) -> Value {
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return AppError::validation("缺少 url").error_json();
    }
    match crate::reader::fetch_article(url).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::validation(e).error_json(),
    }
}

/// `translate`：翻译文本（复用 `translate::translate_text`）。参数：text（必填）、target（可选）。
pub(super) async fn translate(app: &AppHandle, req: &Value) -> Value {
    let text = req
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if text.trim().is_empty() {
        return AppError::validation("缺少 text").error_json();
    }
    if text.len() > 20_000 {
        return AppError::validation("text 过长（>20000 字节）").error_json();
    }
    let target = req
        .get("target")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let db = app.state::<crate::db::Db>();
    match crate::translate::translate_text(db, text, target).await {
        Ok(r) => json!({"ok": true, "data": r}),
        Err(e) => AppError::internal(e).error_json(),
    }
}

/// `content/snapshot`：活动标签语义快照（复用 `content_cmd::content_snapshot`，本地、不出网）。
pub(super) async fn snapshot(app: &AppHandle, _req: &Value) -> Value {
    match crate::content_cmd::content_snapshot(app.clone()).await {
        Ok(s) => json!({"ok": true, "data": s}),
        Err(e) => AppError::internal(e).error_json(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selection_expand_defaults_true() {
        // 缺省要 true：问"这段说的啥"需要段落语境，碎片不够用
        let (expand, _) = selection_args(&json!({}));
        assert!(expand, "expand 缺省必须为 true");
    }

    #[test]
    fn selection_expand_explicit_false() {
        let (expand, _) = selection_args(&json!({"expand": false}));
        assert!(!expand);
    }

    #[test]
    fn selection_expand_accepts_loose_types() {
        // bool_arg 宽松解析：字符串/数字形态（对齐项目既有约定）
        assert!(selection_args(&json!({"expand": "true"})).0);
        assert!(selection_args(&json!({"expand": "1"})).0);
        assert!(selection_args(&json!({"expand": 1})).0);
        assert!(!selection_args(&json!({"expand": "no"})).0);
    }

    #[test]
    fn selection_max_chars_defaults_and_cap() {
        assert_eq!(selection_args(&json!({})).1, 4000);
        assert_eq!(selection_args(&json!({"max_chars": 500})).1, 500);
        // 硬顶 20000：防单次采集撑爆提示词上下文
        assert_eq!(selection_args(&json!({"max_chars": 99_999})).1, 20_000);
        // 负数/非数字回落缺省，不 panic
        assert_eq!(selection_args(&json!({"max_chars": -5})).1, 4000);
        assert_eq!(selection_args(&json!({"max_chars": "x"})).1, 4000);
    }

    #[test]
    fn selection_script_placeholders_present() {
        // 占位符漏写会在运行时静默降级（脚本里残留 __EXPAND__ 字面量），
        // 故编译期兜一道：模板必须含两个占位符，且替换后一个不剩。
        assert!(
            SELECTION_JS.contains("__EXPAND__"),
            "模板缺 __EXPAND__ 占位符"
        );
        assert!(
            SELECTION_JS.contains("__MAX_CHARS__"),
            "模板缺 __MAX_CHARS__ 占位符"
        );
        let (expand, max_chars) = selection_args(&json!({}));
        let rendered = SELECTION_JS
            .replace("__EXPAND__", if expand { "1" } else { "0" })
            .replace("__MAX_CHARS__", &max_chars.to_string());
        assert!(!rendered.contains("__EXPAND__"));
        assert!(!rendered.contains("__MAX_CHARS__"));
    }

    #[test]
    fn selection_script_is_read_only() {
        // 「纯只读」是本能力的安全承诺（不改页面/不派发事件/不聚焦/不执行），
        // 用静态断言兜住：一旦有人往脚本里加了写操作，此测试立刻红。
        for forbidden in [
            "document.write",
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "eval(",
            "Function(",
            "dispatchEvent",
            ".click(",
            ".focus(",
            ".remove(",
            ".submit(",
            ".value =",
            "localStorage",
            "XMLHttpRequest",
            "fetch(",
        ] {
            assert!(
                !SELECTION_JS.contains(forbidden),
                "选区采集脚本必须是纯只读，但出现了写/执行操作：{forbidden}"
            );
        }
    }
}

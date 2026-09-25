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

//! 只读数据查询 handler：历史/书签/下载/会话/标签组/工作区/用户脚本/设置

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

pub(super) async fn history(app: &AppHandle, req: &Value) -> Value {
    let limit = req
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50)
        .clamp(1, 500);
    let db = app.state::<crate::db::Db>();
    let rows = sqlx::query_as::<_, (String, Option<String>, i64, String)>(
        "SELECT url, title, visit_count, last_visit_at FROM history
                 ORDER BY last_visit_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(&db.pool)
    .await
    .unwrap_or_default();
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(url, title, vc, lv)| {
            json!({"url": url, "title": title, "visit_count": vc, "last_visit_at": lv})
        })
        .collect();
    json!({"ok": true, "data": items})
}

pub(super) async fn bookmarks(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    let rows = sqlx::query_as::<_, (String, String, Option<String>, i64)>(
        "SELECT b.title, b.url, f.name, b.sort_order FROM bookmarks b
                 LEFT JOIN bookmark_folders f ON f.id = b.folder_id ORDER BY b.id",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap_or_default();
    let items: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(title, url, folder, so)| {
            json!({"title": title, "url": url, "folder": folder, "sort_order": so})
        })
        .collect();
    json!({"ok": true, "data": items})
}

pub(super) async fn list_downloads(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::downloads::get_downloads(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn list_sessions(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::sessions::list_sessions(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn list_groups(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::tab_groups::get_tab_groups(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn list_spaces(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::workspaces::get_workspaces(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn list_userscripts(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::userscripts::list_userscripts(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn search_history(app: &AppHandle, req: &Value) -> Value {
    let query = req
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if query.is_empty() {
        return AppError::validation("缺少 query").error_json();
    }
    let limit = req.get("limit").and_then(|v| v.as_i64());
    let db = app.state::<crate::db::Db>();
    match crate::db::history::search_history(db, query, limit).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn get_setting(app: &AppHandle, req: &Value) -> Value {
    let key = req
        .get("key")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if key.is_empty() {
        return AppError::validation("缺少 key").error_json();
    }
    let db = app.state::<crate::db::Db>();
    match crate::db::settings::get_setting(db, key).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

pub(super) async fn list_settings(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::db::settings::get_all_settings(db).await {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::db(e.to_string()).error_json(),
    }
}

/// AI 读取审计查询（K 类 / O3 持久化）：返回最近 N 条 AI 操作记录。
/// 只记 命令/url/tab_id/结果码，**不含页面内容**（N5）。Read，只读模式下放行。
pub(super) async fn audit_reads(app: &AppHandle, req: &Value) -> Value {
    let limit = req
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(50)
        .clamp(1, 500);
    let db = app.state::<crate::db::Db>();
    match crate::db::ai_audit::recent(&db.pool, limit).await {
        Ok(items) => json!({"ok": true, "data": items}),
        Err(e) => AppError::db(format!("查询审计失败: {e}")).error_json(),
    }
}

/// 网络外发审计（N3）：列出本应用及其子进程当前的全部 TCP 外联端点。
/// 复用 `audit::egress_audit`（只读 /proc，零出网）。Read，只读模式下放行。
pub(super) async fn audit_egress(_app: &AppHandle, _req: &Value) -> Value {
    match crate::audit::egress_audit().await {
        Ok(report) => json!({"ok": true, "data": report}),
        Err(e) => AppError::internal(format!("网络审计失败: {e}")).error_json(),
    }
}

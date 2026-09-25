//! 插件四形态统一视图 handler：用户脚本 / 内容过滤 / Web 扩展 / MCP 服务器

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 插件清单（四形态统一视图）
pub(super) async fn list_plugins(app: &AppHandle, _req: &Value) -> Value {
    use crate::capabilities::plugins::{entry, PluginKind};
    let db = app.state::<crate::db::Db>();
    let mut list: Vec<serde_json::Value> = Vec::new();

    // 用户脚本
    if let Ok(rows) = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, name, enabled FROM userscripts ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await
    {
        for (id, name, enabled) in rows {
            list.push(entry(
                PluginKind::UserScript,
                &id.to_string(),
                &name,
                enabled != 0,
                "注入页面",
            ));
        }
    }

    // MCP 服务器
    if let Ok(rows) = sqlx::query_as::<_, (i64, String, i64)>(
        "SELECT id, name, enabled FROM mcp_servers ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await
    {
        for (id, name, enabled) in rows {
            list.push(entry(
                PluginKind::McpServer,
                &id.to_string(),
                &name,
                enabled != 0,
                "外部工具服务器",
            ));
        }
    }

    // 内容过滤（广告拦截）
    let adblock_on = sqlx::query_as::<_, (String,)>(
        "SELECT value FROM settings WHERE key = 'adblock_enabled'",
    )
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()
    .map(|(v,)| v != "false")
    .unwrap_or(true);
    list.push(entry(
        PluginKind::ContentFilter,
        "adblock",
        "广告与追踪拦截",
        adblock_on,
        "内置规则 + 自定义关键词",
    ));

    // Web 扩展（编译期加载）
    list.push(entry(
        PluginKind::WebExtension,
        "shift-web-extension",
        "网络层拦截扩展",
        true,
        "编译期 .so，随应用加载",
    ));

    let count = list.len();
    json!({ "ok": true, "data": { "plugins": list, "count": count } })
}

/// 插件启停（四形态统一入口；Web 扩展为编译期加载，不可运行时启停）
pub(super) async fn set_enabled(app: &AppHandle, req: &Value) -> Value {
    let kind = req.get("kind").and_then(|v| v.as_str()).unwrap_or("");
    let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let enabled = req.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
    let db = app.state::<crate::db::Db>();
    let flag: i64 = if enabled { 1 } else { 0 };
    let num_id: i64 = id.parse().unwrap_or(-1);
    let res = match kind {
        "userscript" => {
            sqlx::query("UPDATE userscripts SET enabled = ? WHERE id = ?")
                .bind(flag)
                .bind(num_id)
                .execute(&db.pool)
                .await
        }
        "mcp_server" => {
            sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
                .bind(flag)
                .bind(num_id)
                .execute(&db.pool)
                .await
        }
        "content_filter" => {
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES ('adblock_enabled', ?)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            )
            .bind(if enabled { "true" } else { "false" })
            .execute(&db.pool)
            .await
        }
        "web_extension" => {
            return AppError::permission("Web 扩展为编译期加载，不支持运行时启停").error_json()
        }
        other => return AppError::validation(format!("未知插件形态: {}", other)).error_json(),
    };
    match res {
        Ok(r) => {
            // 统一审计：插件启停记入 tool_audit_log（与 MCP 工具调用同一张表，
            // server_name='plugin' 区分来源；后续审计面板一屏看全部能力调用）
            let _ = sqlx::query(
                "INSERT INTO tool_audit_log
                         (server_id, server_name, tool_name, arguments, result_summary, success)
                         VALUES (NULL, 'plugin', 'set_enabled', ?, ?, 1)",
            )
            .bind(
                serde_json::json!({ "kind": kind, "id": id, "enabled": enabled }).to_string(),
            )
            .bind(format!("affected={}", r.rows_affected()))
            .execute(&db.pool)
            .await;
            json!({ "ok": true, "data": {
                "kind": kind, "id": id, "enabled": enabled, "affected": r.rows_affected()
            }})
        }
        Err(e) => AppError::db(format!("启停失败: {}", e)).error_json(),
    }
}

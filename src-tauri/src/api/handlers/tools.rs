//! MCP 工具网关 handler：列出可用工具 / 调用一个工具

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 工具网关（阶段4 自托管套件）：列出可用 MCP 工具
pub(super) async fn list_tools(app: &AppHandle, _req: &Value) -> Value {
    let db = app.state::<crate::db::Db>();
    match crate::engine::mcp::list_tools(&db.pool).await {
        Ok(tools) => {
            let items: Vec<serde_json::Value> = tools
                .into_iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "server_name": t.server_name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();
            json!({"ok": true, "data": items})
        }
        Err(e) => AppError::net(e).error_json(),
    }
}

/// 工具网关：调用一个 MCP 工具（受 MCP server 的 allowed_tools 白名单约束）
pub(super) async fn call_tool(app: &AppHandle, req: &Value) -> Value {
    let name = req
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if name.is_empty() {
        return AppError::validation("缺少 name").error_json();
    }
    let args = req.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let db = app.state::<crate::db::Db>();
    match crate::engine::mcp::call_tool(&db.pool, &name, args).await {
        Ok(r) => json!({"ok": true, "data": {"result": r}}),
        Err(e) => AppError::net(e).error_json(),
    }
}

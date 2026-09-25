// MCP 工具层命令（阶段 1.3 / R1 错误类型化第二批 · 2026-09-25）：server 配置管理 + 工具列表 + 手动调用 + 审计查询
use crate::db::Db;
use crate::error::CommandResult;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerView {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
    pub allowed_tools: Vec<String>,
    /// 该 server 当前可用的工具数（拉取失败为 0）
    pub tool_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolView {
    pub name: String,
    pub server_name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// 全部 server（含启用状态与工具数）——设置页展示
#[tauri::command]
pub async fn mcp_list_servers(db: tauri::State<'_, Db>) -> CommandResult<Vec<McpServerView>> {
    let servers = crate::engine::mcp::list_servers(&db.pool).await?;
    let mut views = Vec::new();
    for s in &servers {
        let tool_count = if s.enabled {
            match crate::engine::mcp::fetch_tools(&db.pool, s).await {
                Ok(t) => t.len(),
                Err(_) => 0,
            }
        } else {
            0
        };
        views.push(McpServerView {
            id: s.id,
            name: s.name.clone(),
            command: s.command.clone(),
            args: s.args.clone(),
            enabled: s.enabled,
            allowed_tools: s.allowed_tools.clone(),
            tool_count,
        });
    }
    Ok(views)
}

/// 新增 server（name/command 必填，args 为参数数组，allowed_tools 白名单）
#[tauri::command]
pub async fn mcp_add_server(
    db: tauri::State<'_, Db>,
    name: String,
    command: String,
    args: Vec<String>,
    allowed_tools: Vec<String>,
) -> CommandResult<i64> {
    crate::engine::mcp::add_server(&db.pool, name, command, args, allowed_tools).await
}

/// 删除 server
#[tauri::command]
pub async fn mcp_remove_server(db: tauri::State<'_, Db>, id: i64) -> CommandResult<()> {
    crate::engine::mcp::remove_server(&db.pool, id).await
}

/// 切换启用状态
#[tauri::command]
pub async fn mcp_set_server_enabled(
    db: tauri::State<'_, Db>,
    id: i64,
    enabled: bool,
) -> CommandResult<()> {
    crate::engine::mcp::set_server_enabled(&db.pool, id, enabled).await
}

/// 全部启用 server 的工具列表（合并，供 AI 面板展示 / function calling）
#[tauri::command]
pub async fn mcp_list_tools(db: tauri::State<'_, Db>) -> CommandResult<Vec<McpToolView>> {
    let tools = crate::engine::mcp::list_tools(&db.pool).await?;
    Ok(tools
        .into_iter()
        .map(|t| McpToolView {
            name: t.name,
            server_name: t.server_name,
            description: t.description,
            input_schema: t.input_schema,
        })
        .collect())
}

/// 手动调用工具（设置页测试用；AI 执行循环走内部 call_tool）
#[tauri::command]
pub async fn mcp_call_tool(
    db: tauri::State<'_, Db>,
    name: String,
    arguments: serde_json::Value,
) -> CommandResult<String> {
    crate::engine::mcp::call_tool(&db.pool, &name, arguments).await
}

/// 审计日志（倒序）
#[tauri::command]
pub async fn mcp_get_audit_log(
    db: tauri::State<'_, Db>,
    limit: Option<i64>,
) -> CommandResult<Vec<crate::engine::mcp::AuditEntry>> {
    crate::engine::mcp::get_audit_log(&db.pool, limit.unwrap_or(50)).await
}

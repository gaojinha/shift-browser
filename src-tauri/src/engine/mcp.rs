//! MCP 客户端（阶段 1.3 工具层）门面
//!
//! 第一期：**stdio 传输**（本地进程，JSON-RPC 2.0，newline 分隔）。
//! 远端 MCP server（streamable HTTP）留后续阶段。
//!
//! 职责：
//! - 连接管理：按 mcp_servers 表配置 spawn 本地进程，initialize 握手，长驻缓存
//! - 工具注册表：tools/list 汇总为 OpenAI function 定义（供 LLM function calling）
//! - 工具执行：tools/call + 权限白名单校验 + 审计日志写库
//!
//! 安全边界：
//! - server 命令来自 DB 配置（本地可信配置，不从前端任意执行）
//! - allowed_tools 白名单（空 = 允许全部）
//! - 每次调用写 tool_audit_log（审计）
//!
//! R1 拆分（2026-09-25）：传输/握手/连接缓存见 `mcp/transport.rs`，测试见 `mcp/tests.rs`。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;

use crate::error::{AppError, CommandResult};

#[path = "mcp/tests.rs"]
#[cfg(test)]
mod tests;
#[path = "mcp/transport.rs"]
mod transport;

use transport::{connect, connections};

/// DB 里的 server 配置行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerRow {
    pub id: i64,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub enabled: bool,
    pub allowed_tools: Vec<String>,
}

/// 工具定义（OpenAI function calling 格式 + server 归属）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// 全局唯一工具名（server 前缀：`{server_name}__{tool_name}`，AI 调用用这个）
    pub name: String,
    /// 原始工具名（server 内）
    pub tool_name: String,
    pub server_id: i64,
    pub server_name: String,
    pub description: String,
    /// OpenAI function.parameters（inputSchema）
    pub input_schema: Value,
}

/// 审计日志行
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub id: i64,
    pub server_name: String,
    pub tool_name: String,
    pub arguments: String,
    pub result_summary: String,
    pub success: bool,
    pub created_at: String,
}

/// 读取全部启用 server 的配置行
pub async fn list_servers(pool: &SqlitePool) -> CommandResult<Vec<McpServerRow>> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, i64, String)>(
        "SELECT id, name, command, args, enabled, allowed_tools FROM mcp_servers ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::db(format!("读取 MCP server 配置失败: {}", e)))?;

    Ok(rows
        .into_iter()
        .map(|(id, name, command, args, enabled, allowed)| McpServerRow {
            id,
            name,
            command,
            args: serde_json::from_str(&args).unwrap_or_default(),
            enabled: enabled != 0,
            allowed_tools: serde_json::from_str(&allowed).unwrap_or_default(),
        })
        .collect())
}

/// 新增 server 配置（name 唯一；command 必须非空）
pub async fn add_server(
    pool: &SqlitePool,
    name: String,
    command: String,
    args: Vec<String>,
    allowed_tools: Vec<String>,
) -> CommandResult<i64> {
    if name.trim().is_empty() || command.trim().is_empty() {
        return Err(AppError::validation("name 与 command 不能为空").into());
    }
    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "[]".to_string());
    let allowed_json = serde_json::to_string(&allowed_tools).unwrap_or_else(|_| "[]".to_string());
    let result = sqlx::query(
    "INSERT INTO mcp_servers (name, command, args, enabled, allowed_tools) VALUES (?, ?, ?, 1, ?)",
  )
  .bind(name.trim())
  .bind(command.trim())
  .bind(args_json)
  .bind(allowed_json)
  .execute(pool)
  .await
  .map_err(|e| AppError::db(format!("新增 MCP server 失败: {}", e)))?;
    Ok(result.last_insert_rowid())
}

/// 删除 server 配置（同时断开连接缓存）
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn remove_server(pool: &SqlitePool, id: i64) -> CommandResult<()> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::db(format!("删除 MCP server 失败: {}", e)))?;
    // 断开缓存连接（drop 后子进程随 stdin 关闭退出）
    connections().lock().unwrap().remove(&id);
    Ok(())
}

/// 切换 server 启用状态
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub async fn set_server_enabled(pool: &SqlitePool, id: i64, enabled: bool) -> CommandResult<()> {
    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::db(format!("更新 MCP server 状态失败: {}", e)))?;
    if !enabled {
        connections().lock().unwrap().remove(&id);
    }
    Ok(())
}

/// 拉取单个 server 的工具列表（OpenAI function 格式）
pub async fn fetch_tools(pool: &SqlitePool, row: &McpServerRow) -> CommandResult<Vec<McpTool>> {
    let conn = connect(pool, row).await.map_err(AppError::net)?;
    let mut conn = conn.lock().await;
    let result = conn
        .request("tools/list", json!({}))
        .await
        .map_err(|e| AppError::net(format!("server '{}' tools/list 失败: {}", row.name, e)))?;

    let tools: Vec<Value> = result
        .get("tools")
        .and_then(|t| t.as_array())
        .cloned()
        .unwrap_or_default();

    let mut out = Vec::new();
    for t in tools {
        let tool_name = t
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or_default()
            .to_string();
        if tool_name.is_empty() {
            continue;
        }
        // 权限白名单：allowed_tools 非空时只暴露白名单内的工具
        if !row.allowed_tools.is_empty() && !row.allowed_tools.iter().any(|a| a == &tool_name) {
            continue;
        }
        out.push(McpTool {
            name: format!("{}__{}", row.name, tool_name),
            tool_name: tool_name.clone(),
            server_id: row.id,
            server_name: row.name.clone(),
            description: t
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or_default()
                .to_string(),
            input_schema: t.get("inputSchema").cloned().unwrap_or(json!({})),
        });
    }
    Ok(out)
}

/// 汇总全部启用 server 的工具列表（工具注册表快照）
pub async fn list_tools(pool: &SqlitePool) -> CommandResult<Vec<McpTool>> {
    let servers = list_servers(pool).await?;
    let mut all = Vec::new();
    for row in servers {
        if !row.enabled {
            continue;
        }
        match fetch_tools(pool, &row).await {
            Ok(tools) => all.extend(tools),
            Err(e) => log::error!("[mcp] 拉取工具失败: {}", e),
        }
    }
    Ok(all)
}

/// 调用工具（AI 执行循环与手动调用统一入口）。
/// 校验白名单 + 审计写库。返回文本结果（MCP content 拼接）。
pub async fn call_tool(
    pool: &SqlitePool,
    full_name: &str,
    arguments: Value,
) -> CommandResult<String> {
    // 解析 full_name：`{server_name}__{tool_name}`（__ 分隔）
    let (server_name, tool_name) = full_name
        .split_once("__")
        .ok_or_else(|| AppError::validation(format!("工具名格式不合法: {}", full_name)))?;

    let servers = list_servers(pool).await?;
    let row = servers
        .iter()
        .find(|s| s.name == server_name)
        .ok_or_else(|| AppError::validation(format!("MCP server '{}' 不存在", server_name)))?;
    if !row.enabled {
        return Err(AppError::permission(format!("MCP server '{}' 已停用", server_name)).into());
    }
    // 白名单校验
    if !row.allowed_tools.is_empty() && !row.allowed_tools.iter().any(|a| a == tool_name) {
        return Err(AppError::permission(format!(
            "工具 '{}' 不在 server '{}' 白名单内",
            tool_name, server_name
        ))
        .into());
    }

    let conn = connect(pool, row).await.map_err(AppError::net)?;
    let mut conn = conn.lock().await;
    let result = conn
        .request(
            "tools/call",
            json!({ "name": tool_name, "arguments": arguments }),
        )
        .await;

    // 审计写库（无论成败）
    let (success, text) = match &result {
        Ok(v) => {
            let is_error = v.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
            let text = v
                .get("content")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| item.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            (!is_error, text)
        }
        Err(e) => (false, e.clone()),
    };
    let summary: String = text.chars().take(200).collect();
    let _ = sqlx::query(
    "INSERT INTO tool_audit_log (server_id, server_name, tool_name, arguments, result_summary, success) VALUES (?, ?, ?, ?, ?, ?)",
  )
  .bind(row.id)
  .bind(&row.name)
  .bind(tool_name)
  .bind(arguments.to_string())
  .bind(&summary)
  .bind(if success { 1 } else { 0 })
  .execute(pool)
  .await;

    match result {
        Ok(v) => {
            let is_error = v.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
            if is_error {
                Err(AppError::net(format!("工具 '{}' 执行失败: {}", full_name, text)).into())
            } else {
                log::info!(
                    "[mcp] 工具调用成功: {} 结果 {} 字符",
                    full_name,
                    text.chars().count()
                );
                Ok(text)
            }
        }
        Err(e) => Err(AppError::net(e).into()),
    }
}

/// 最近审计日志（倒序，limit 条）
pub async fn get_audit_log(pool: &SqlitePool, limit: i64) -> CommandResult<Vec<AuditEntry>> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, i64, String)>(
    "SELECT id, server_name, tool_name, arguments, result_summary, success, created_at FROM tool_audit_log ORDER BY id DESC LIMIT ?",
  )
  .bind(limit)
  .fetch_all(pool)
  .await
  .map_err(|e| AppError::db(format!("读取审计日志失败: {}", e)))?;

    Ok(rows
        .into_iter()
        .map(
            |(id, server_name, tool_name, arguments, result_summary, success, created_at)| {
                AuditEntry {
                    id,
                    server_name,
                    tool_name,
                    arguments,
                    result_summary,
                    success: success != 0,
                    created_at,
                }
            },
        )
        .collect())
}

/// 把工具列表转成 OpenAI function calling 的 tools 参数
pub fn to_openai_tools(tools: &[McpTool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            json!({
              "type": "function",
              "function": {
                "name": t.name,
                "description": t.description,
                "parameters": t.input_schema,
              }
            })
        })
        .collect()
}

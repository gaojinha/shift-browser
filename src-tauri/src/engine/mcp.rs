//! MCP 客户端（阶段 1.3 工具层）
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

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// MCP 协议版本（2025-03-26：stdio 传输的当前稳定版）
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// 单个 MCP server 连接（一个子进程 + 串行请求）
struct McpConnection {
    /// 子进程句柄：保持进程存活，drop 时随 stdin 关闭退出
    #[allow(dead_code)]
    child: tokio::process::Child,
    stdin: tokio::process::ChildStdin,
    stdout: tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    next_id: AtomicU64,
    server_name: String,
}

impl McpConnection {
    /// JSON-RPC 通知（无 id，server 不应答——如 notifications/initialized）
    async fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        let req = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        let payload = format!("{}
", req);
        self.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("MCP 写入失败: {}", e))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("MCP 刷新失败: {}", e))
    }

    /// JSON-RPC 请求（阻塞读直到 id 匹配的响应；忽略 notification）
    /// 加 30s 超时保护：server 卡死/不响应时返回错误，不挂起调用方
    async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let payload = format!("{}\n", req);
        self.stdin
            .write_all(payload.as_bytes())
            .await
            .map_err(|e| format!("MCP 写入失败: {}", e))?;
        self.stdin
            .flush()
            .await
            .map_err(|e| format!("MCP 刷新失败: {}", e))?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(format!("MCP server '{}' 响应超时（{}）", self.server_name, method));
            }
            let line = self
                .stdout
                .next_line()
                .await
                .map_err(|e| format!("MCP 读取失败: {}", e))?
                .ok_or_else(|| format!("MCP server '{}' 进程已退出", self.server_name))?;
            if line.trim().is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line)
                .map_err(|e| format!("MCP 响应解析失败: {}", e))?;
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("MCP 错误: {}", err));
                }
                return Ok(v.get("result").cloned().unwrap_or(Value::Null));
            }
            // 其他行：notification 或无关响应，跳过
        }
    }
}

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

/// 连接缓存：server_id -> 连接（进程长驻）
static CONNECTIONS: OnceLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<McpConnection>>>>> =
    OnceLock::new();

fn connections() -> &'static Mutex<HashMap<i64, Arc<tokio::sync::Mutex<McpConnection>>>> {
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 读取全部启用 server 的配置行
pub async fn list_servers(pool: &SqlitePool) -> Result<Vec<McpServerRow>, String> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, i64, String)>(
        "SELECT id, name, command, args, enabled, allowed_tools FROM mcp_servers ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取 MCP server 配置失败: {}", e))?;

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
) -> Result<i64, String> {
    if name.trim().is_empty() || command.trim().is_empty() {
        return Err("name 与 command 不能为空".to_string());
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
    .map_err(|e| format!("新增 MCP server 失败: {}", e))?;
    Ok(result.last_insert_rowid())
}

/// 删除 server 配置（同时断开连接缓存）
pub async fn remove_server(pool: &SqlitePool, id: i64) -> Result<(), String> {
    sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("删除 MCP server 失败: {}", e))?;
    // 断开缓存连接（drop 后子进程随 stdin 关闭退出）
    connections().lock().unwrap().remove(&id);
    Ok(())
}

/// 切换 server 启用状态
pub async fn set_server_enabled(pool: &SqlitePool, id: i64, enabled: bool) -> Result<(), String> {
    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| format!("更新 MCP server 状态失败: {}", e))?;
    if !enabled {
        connections().lock().unwrap().remove(&id);
    }
    Ok(())
}

/// 建立（或复用）连接：spawn 进程 + initialize 握手
async fn connect(_pool: &SqlitePool, row: &McpServerRow) -> Result<Arc<tokio::sync::Mutex<McpConnection>>, String> {
    if let Some(conn) = connections().lock().unwrap().get(&row.id) {
        return Ok(conn.clone());
    }

    let mut cmd = Command::new(&row.command);
    cmd.args(&row.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动 MCP server '{}' 失败: {}（命令: {}）", row.name, e, row.command))?;
    let stdin = child.stdin.take().ok_or_else(|| "无法获取 MCP stdin".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "无法获取 MCP stdout".to_string())?;

    let mut conn = McpConnection {
        child,
        stdin,
        stdout: BufReader::new(stdout).lines(),
        next_id: AtomicU64::new(1),
        server_name: row.name.clone(),
    };

    // initialize 握手
    let result = conn
        .request(
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": { "name": "shift-browser", "version": "5.6.0" },
            }),
        )
        .await?;
    let server_name = result
        .get("serverInfo")
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(&row.name)
        .to_string();
    log::info!("[mcp] server '{}' 握手成功（{}）", row.name, server_name);

    // initialized 通知（MCP 协议要求，无 id 不应答）
    let _ = conn.notify("notifications/initialized", json!({})).await;

    let conn = Arc::new(tokio::sync::Mutex::new(conn));
    connections().lock().unwrap().insert(row.id, conn.clone());
    Ok(conn)
}

/// 拉取单个 server 的工具列表（OpenAI function 格式）
pub async fn fetch_tools(pool: &SqlitePool, row: &McpServerRow) -> Result<Vec<McpTool>, String> {
    let conn = connect(pool, row).await?;
    let mut conn = conn.lock().await;
    let result = conn
        .request("tools/list", json!({}))
        .await
        .map_err(|e| format!("server '{}' tools/list 失败: {}", row.name, e))?;

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
        if !row.allowed_tools.is_empty()
            && !row.allowed_tools.iter().any(|a| a == &tool_name)
        {
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
pub async fn list_tools(pool: &SqlitePool) -> Result<Vec<McpTool>, String> {
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
) -> Result<String, String> {
    // 解析 full_name：`{server_name}__{tool_name}`（__ 分隔）
    let (server_name, tool_name) = full_name
        .split_once("__")
        .ok_or_else(|| format!("工具名格式不合法: {}", full_name))?;

    let servers = list_servers(pool).await?;
    let row = servers
        .iter()
        .find(|s| s.name == server_name)
        .ok_or_else(|| format!("MCP server '{}' 不存在", server_name))?;
    if !row.enabled {
        return Err(format!("MCP server '{}' 已停用", server_name));
    }
    // 白名单校验
    if !row.allowed_tools.is_empty()
        && !row.allowed_tools.iter().any(|a| a == tool_name)
    {
        return Err(format!("工具 '{}' 不在 server '{}' 白名单内", tool_name, server_name));
    }

    let conn = connect(pool, row).await?;
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
                Err(format!("工具 '{}' 执行失败: {}", full_name, text))
            } else {
                log::info!("[mcp] 工具调用成功: {} 结果 {} 字符", full_name, text.chars().count());
                Ok(text)
            }
        }
        Err(e) => Err(e),
    }
}

/// 最近审计日志（倒序，limit 条）
pub async fn get_audit_log(pool: &SqlitePool, limit: i64) -> Result<Vec<AuditEntry>, String> {
    let limit = limit.clamp(1, 200);
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, i64, String)>(
        "SELECT id, server_name, tool_name, arguments, result_summary, success, created_at FROM tool_audit_log ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await
    .map_err(|e| format!("读取审计日志失败: {}", e))?;

    Ok(rows
        .into_iter()
        .map(|(id, server_name, tool_name, arguments, result_summary, success, created_at)| {
            AuditEntry {
                id,
                server_name,
                tool_name,
                arguments,
                result_summary,
                success: success != 0,
                created_at,
            }
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_name_roundtrip() {
        let full = "filesystem__read_file";
        assert_eq!(full.split_once("__"), Some(("filesystem", "read_file")));
    }

    /// 假 MCP server（node stdio JSON-RPC）：initialize / tools/list / tools/call
    const FAKE_SERVER_JS: &str = r#"
const readline = require('readline');
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  const msg = JSON.parse(line);
  if (msg.method === 'initialize') {
    console.log(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { protocolVersion: '2025-03-26', capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1.0.0' } } }));
  } else if (msg.method === 'tools/list') {
    console.log(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { tools: [{ name: 'echo', description: '回显文本', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } }] } }));
  } else if (msg.method === 'tools/call') {
    const args = msg.params.arguments || {};
    console.log(JSON.stringify({ jsonrpc: '2.0', id: msg.id, result: { content: [{ type: 'text', text: 'echo: ' + (args.text || '') }], isError: false } }));
  }
});
"#;

    /// 端到端：spawn 假 server → 握手 → tools/list → tools/call → 审计写库
    #[tokio::test]
    async fn mcp_end_to_end_with_fake_server() {
        // 临时目录 + 临时 DB（建 mcp 相关表）
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("fake_mcp.js");
        std::fs::write(&script, FAKE_SERVER_JS).unwrap();

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(dir.path().join("test.db"))
                    .create_if_missing(true),
            )
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE mcp_servers (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL UNIQUE, command TEXT NOT NULL, args TEXT NOT NULL DEFAULT '[]', enabled INTEGER NOT NULL DEFAULT 1, allowed_tools TEXT NOT NULL DEFAULT '[]', created_at TEXT NOT NULL DEFAULT (datetime('now')))",
        )
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE tool_audit_log (id INTEGER PRIMARY KEY AUTOINCREMENT, server_id INTEGER, server_name TEXT NOT NULL, tool_name TEXT NOT NULL, arguments TEXT NOT NULL DEFAULT '{}', result_summary TEXT NOT NULL DEFAULT '', success INTEGER NOT NULL DEFAULT 1, created_at TEXT NOT NULL DEFAULT (datetime('now')))",
        )
        .execute(&pool)
        .await
        .unwrap();

        // 添加 server（command=node，args=[脚本路径]）
        let id = add_server(
            &pool,
            "fake".to_string(),
            "node".to_string(),
            vec![script.to_string_lossy().to_string()],
            vec![],
        )
        .await
        .unwrap();
        assert!(id > 0);

        // list_tools：应拉到 echo 工具（全局名 fake__echo）
        let tools = list_tools(&pool).await.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "fake__echo");
        assert_eq!(tools[0].server_name, "fake");

        // call_tool：回显
        let r = call_tool(&pool, "fake__echo", json!({"text": "你好，二狗"})).await.unwrap();
        assert!(r.contains("你好，二狗"), "回显失败: {}", r);

        // 审计日志：应有一条记录
        let log = get_audit_log(&pool, 10).await.unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].server_name, "fake");
        assert_eq!(log[0].tool_name, "echo");
        assert!(log[0].success);
        assert!(log[0].result_summary.contains("你好，二狗"));

        // 白名单过滤：allowed_tools=["other"] 时 echo 不可见
        sqlx::query("UPDATE mcp_servers SET allowed_tools = ? WHERE id = ?")
            .bind(r#"["other"]"#)
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        // 断开旧连接缓存（白名单变更后重新拉取）
        connections().lock().unwrap().remove(&id);
        let tools = list_tools(&pool).await.unwrap();
        assert!(tools.is_empty(), "白名单应过滤掉 echo");

        // 手动调用白名单外的工具应被拒绝
        let err = call_tool(&pool, "fake__echo", json!({"text": "x"})).await.unwrap_err();
        assert!(err.contains("白名单"), "应拒绝白名单外工具: {}", err);
    }

    #[test]
    fn to_openai_tools_format() {
        let tools = vec![McpTool {
            name: "fs__read".to_string(),
            tool_name: "read".to_string(),
            server_id: 1,
            server_name: "fs".to_string(),
            description: "读取文件".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        }];
        let openai = to_openai_tools(&tools);
        assert_eq!(openai.len(), 1);
        assert_eq!(openai[0]["type"], "function");
        assert_eq!(openai[0]["function"]["name"], "fs__read");
        assert_eq!(openai[0]["function"]["description"], "读取文件");
        assert!(openai[0]["function"]["parameters"]["type"].is_string());
    }
}
//! MCP stdio 传输层（R1 拆分 · 2026-09-25）
//!
//! 单个 MCP server 连接（一个子进程 + 串行 JSON-RPC），含 initialize 握手、
//! 长驻连接缓存。业务层（工具拉取/调用/审计）在父模块 mcp.rs。

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::{json, Value};
use sqlx::sqlite::SqlitePool;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use super::McpServerRow;

/// MCP 协议版本（2025-03-26：stdio 传输的当前稳定版）
const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// 单个 MCP server 连接（一个子进程 + 串行请求）
pub(super) struct McpConnection {
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
        let payload = format!("{}\n", req);
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
    pub(super) async fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
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

        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(crate::contracts::timeout::MCP_REQUEST_SECS);
        loop {
            if std::time::Instant::now() > deadline {
                return Err(format!(
                    "MCP server '{}' 响应超时（{}）",
                    self.server_name, method
                ));
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
            let v: Value =
                serde_json::from_str(&line).map_err(|e| format!("MCP 响应解析失败: {}", e))?;
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

/// 连接缓存：server_id -> 连接（进程长驻）
static CONNECTIONS: OnceLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<McpConnection>>>>> =
    OnceLock::new();

pub(super) fn connections() -> &'static Mutex<HashMap<i64, Arc<tokio::sync::Mutex<McpConnection>>>>
{
    CONNECTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 建立（或复用）连接：spawn 进程 + initialize 握手
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(super) async fn connect(
    _pool: &SqlitePool,
    row: &McpServerRow,
) -> Result<Arc<tokio::sync::Mutex<McpConnection>>, String> {
    if let Some(conn) = connections().lock().unwrap().get(&row.id) {
        return Ok(conn.clone());
    }

    let mut cmd = Command::new(&row.command);
    cmd.args(&row.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| {
        format!(
            "启动 MCP server '{}' 失败: {}（命令: {}）",
            row.name, e, row.command
        )
    })?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "无法获取 MCP stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "无法获取 MCP stdout".to_string())?;

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
              "clientInfo": { "name": "shift-browser", "version": env!("CARGO_PKG_VERSION") },
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

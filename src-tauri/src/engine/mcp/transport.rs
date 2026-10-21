// LINES-EXEMPT: 单个 MCP server 的 JSON-RPC 串行状态机（握手/长驻缓存/回收），生命周期必须在一个文件内闭合
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
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
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
    /// stdout 缓冲读（不用 `Lines`：其内部缓冲无长度限制，恶意 server 用超大行
    /// 可耗尽内存；此处逐块读取并按 `MCP_MAX_LINE_BYTES` 显式限长）
    stdout: BufReader<tokio::process::ChildStdout>,
    next_id: AtomicU64,
    server_name: String,
}

/// 带单行上限的 stdout 读取（逐块累积，不会先分配整行）：
/// - 返回 `Ok(None)` 表示 EOF（进程已退出）；
/// - 单行（含换行符）超过 `MCP_MAX_LINE_BYTES` 直接报错，不再缓冲。
async fn read_line_limited<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
) -> Result<Option<String>, String> {
    let mut buf: Vec<u8> = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|e| format!("MCP 读取失败: {}", e))?;
        if available.is_empty() {
            if buf.is_empty() {
                return Ok(None);
            }
            return String::from_utf8(buf)
                .map(Some)
                .map_err(|e| format!("MCP 响应非 UTF-8: {}", e));
        }

        let count = available
            .iter()
            .position(|b| *b == b'\n')
            .map_or(available.len(), |i| i + 1);
        if buf.len().saturating_add(count) > max_bytes {
            return Err(format!("MCP 单行响应超限（上限 {} 字节）", max_bytes));
        }
        let has_newline = available[count - 1] == b'\n';
        buf.extend_from_slice(&available[..count]);
        reader.consume(count);
        if has_newline {
            return String::from_utf8(buf)
                .map(Some)
                .map_err(|e| format!("MCP 响应非 UTF-8: {}", e));
        }
    }
}

impl McpConnection {
    /// 强制终止子进程（顽固进程不因 stdin EOF 退出时的兜底；握手失败/remove 路径用）。
    /// kill 后短暂 wait 收尸，避免僵尸进程；wait 超时也不阻塞调用方。
    pub(super) async fn kill(&mut self) {
        let _ = self.child.kill().await;
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(100), self.child.wait()).await;
    }

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
    /// 双层超时保护：整体 deadline（MCP_REQUEST_SECS）+ 单行读取 timeout——
    /// deadline 只在循环迭代开始检查，server 卡住不发换行时 `next_line().await`
    /// 会永久挂起（旧实现缺陷），故每次读取必须用 `tokio::time::timeout` 约束。
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
            // 单行读取 timeout：server 不发换行时不等死，到时即报错释放连接锁，
            // 后续同一 server 的调用不再排队卡住
            let line_result = tokio::time::timeout(
                std::time::Duration::from_secs(crate::contracts::timeout::MCP_REQUEST_SECS),
                read_line_limited(
                    &mut self.stdout,
                    crate::contracts::limits::MCP_MAX_LINE_BYTES,
                ),
            )
            .await;
            let line = match line_result {
                Err(_) => {
                    let error = format!(
                        "MCP server '{}' 读取超时（{}s）：{}",
                        self.server_name,
                        crate::contracts::timeout::MCP_REQUEST_SECS,
                        method
                    );
                    self.kill().await;
                    return Err(error);
                }
                Ok(Err(error)) => {
                    self.kill().await;
                    return Err(error);
                }
                Ok(Ok(None)) => {
                    return Err(format!("MCP server '{}' 进程已退出", self.server_name));
                }
                Ok(Ok(Some(line))) => line,
            };
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

/// 首次连接串行锁（per-server）：`connect` 的缓存检查与插入之间隔着异步握手，
/// 并发请求会各自 spawn 子进程（最后只留一个，白白浪费进程）。同一 server 的首次
/// 连接在此排队，不同 server 互不阻塞。锁本身无状态，server 删除后残留条目可接受。
static CONNECT_LOCKS: OnceLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();

pub(super) fn connect_lock(server_id: i64) -> Arc<tokio::sync::Mutex<()>> {
    // 锁中毒毒化恢复（model.rs 先例）：锁无状态，毒化后数据仍可用，
    // 直接取内层数据继续，不 panic（AGENTS.md 铁律：锁一律毒化恢复或降级）
    let mut map = CONNECT_LOCKS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    map.entry(server_id)
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

/// 建立（或复用）连接：spawn 进程 + initialize 握手
pub(super) async fn connect(
    pool: &SqlitePool,
    row: &McpServerRow,
) -> Result<Arc<tokio::sync::Mutex<McpConnection>>, String> {
    if let Some(conn) = connections()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&row.id)
    {
        return Ok(conn.clone());
    }
    // per-server 串行化首次连接：握手完成前其他并发请求在此排队，不重复 spawn
    let conn_lock = connect_lock(row.id);
    let _connect_guard = conn_lock.lock().await;
    // 双重检查：排队期间可能已有其他请求完成连接
    if let Some(conn) = connections()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&row.id)
    {
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
        stdout: BufReader::new(stdout),
        next_id: AtomicU64::new(1),
        server_name: row.name.clone(),
    };

    // initialize 握手（失败即 kill 子进程，不留残留进程）
    let result = match conn
        .request(
            "initialize",
            json!({
              "protocolVersion": MCP_PROTOCOL_VERSION,
              "capabilities": {},
              "clientInfo": { "name": "shift-browser", "version": env!("CARGO_PKG_VERSION") },
            }),
        )
        .await
    {
        Ok(r) => r,
        Err(e) => {
            conn.kill().await;
            return Err(e);
        }
    };
    let server_name = result
        .get("serverInfo")
        .and_then(|s| s.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or(&row.name)
        .to_string();
    log::info!("[mcp] server '{}' 握手成功（{}）", row.name, server_name);

    // initialized 通知（MCP 协议要求，无 id 不应答）
    let _ = conn.notify("notifications/initialized", json!({})).await;

    // 握手期间可能被删除/禁用：insert 前查 DB 确认仍存在且启用，否则 kill 丢弃，
    // 不留已删除 server 的活动连接（与 remove/set_enabled 的拿锁删除配合）
    let enabled: Option<i64> = sqlx::query_scalar("SELECT enabled FROM mcp_servers WHERE id = ?")
        .bind(row.id)
        .fetch_optional(pool)
        .await
        .map_err(|e| format!("MCP server 状态复核失败: {}", e))?;
    if enabled != Some(1) {
        conn.kill().await;
        return Err(format!(
            "MCP server '{}' 已被删除或停用，连接已丢弃",
            row.name
        ));
    }

    let conn = Arc::new(tokio::sync::Mutex::new(conn));
    connections()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(row.id, conn.clone());
    Ok(conn)
}

#[cfg(test)]
mod tests {
    use super::read_line_limited;
    use std::pin::Pin;
    use std::task::Context;
    use tokio::io::{AsyncBufRead, AsyncRead, BufReader, ReadBuf};

    #[tokio::test]
    async fn limited_line_reader_accepts_boundary_and_eof() {
        let mut reader = BufReader::new(std::io::Cursor::new(b"abc\nlast".to_vec()));
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("abc\n".to_string())
        );
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("last".to_string())
        );
        assert_eq!(read_line_limited(&mut reader, 4).await.unwrap(), None);
    }

    #[tokio::test]
    async fn limited_line_reader_rejects_overlong_line() {
        let mut reader = BufReader::new(std::io::Cursor::new(b"abcde\nnext\n".to_vec()));
        let error = read_line_limited(&mut reader, 4).await.unwrap_err();
        assert!(error.contains("上限 4 字节"));
    }

    /// 按固定块长吐出字节，强制 read_line_limited 跨多次 fill_buf 累积
    /// （BufReader<Cursor> 一次给整段，累积路径走不到；不起真进程）
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }

    impl AsyncRead for ChunkedReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _buf: &mut ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            // 超 trait 占位：read_line_limited 只走 fill_buf/consume 路径，不会调 poll_read
            std::task::Poll::Ready(Ok(()))
        }
    }

    impl AsyncBufRead for ChunkedReader {
        fn poll_fill_buf(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> std::task::Poll<std::io::Result<&[u8]>> {
            // get_mut：借出内层 &mut Self，返回引用挂在内层生命周期上（标准 Cursor 模式）
            let this = self.get_mut();
            let end = (this.pos + this.chunk).min(this.data.len());
            std::task::Poll::Ready(Ok(&this.data[this.pos..end]))
        }

        fn consume(mut self: Pin<&mut Self>, n: usize) {
            self.pos += n;
        }
    }

    #[tokio::test]
    async fn limited_line_reader_accumulates_across_chunks() {
        // chunk=2：每行都要跨多次 fill_buf 拼装
        let mut reader = ChunkedReader {
            data: b"abc\ndef\n\nlast".to_vec(),
            pos: 0,
            chunk: 2,
        };
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("abc\n".to_string()),
            "跨块累积 + 恰好 4 字节（== 上限）放行"
        );
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("def\n".to_string())
        );
        // 空行：单个 \n 也是完整一行（request() 层跳过空行，函数本身原样返回）
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("\n".to_string())
        );
        // EOF 无换行（数据跨块累积后收尾）
        assert_eq!(
            read_line_limited(&mut reader, 4).await.unwrap(),
            Some("last".to_string())
        );
        assert_eq!(read_line_limited(&mut reader, 4).await.unwrap(), None);
    }

    #[tokio::test]
    async fn limited_line_reader_rejects_overlong_across_chunks() {
        // 累积中才超限（既有例只覆盖单块内超限）：4 字节在限内，再来 4 字节 8>6 报错
        let mut reader = ChunkedReader {
            data: b"abcdefgh\n".to_vec(),
            pos: 0,
            chunk: 4,
        };
        let error = read_line_limited(&mut reader, 6).await.unwrap_err();
        assert!(error.contains("上限 6 字节"));
    }

    #[tokio::test]
    async fn limited_line_reader_rejects_invalid_utf8() {
        let mut reader = BufReader::new(std::io::Cursor::new(b"\xff\xfe\n".to_vec()));
        let error = read_line_limited(&mut reader, 10).await.unwrap_err();
        assert!(error.contains("非 UTF-8"));
    }
}

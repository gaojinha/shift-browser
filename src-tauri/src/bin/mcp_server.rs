//! Shift Browser MCP Server（stdio 传输）
//!
//! 把浏览器的本地开放 API 暴露为标准 MCP 工具，任何 MCP 客户端
//! （opencode / Claude Desktop / Cursor 等）配一条 command 即可驱动浏览器。
//!
//! 用法（MCP 客户端配置）：
//! ```json
//! { "mcpServers": { "shift-browser": { "command": "/home/gao/.local/bin/shift-browser-mcp" } } }
//! ```
//!
//! 环境变量：
//! - `SHIFT_BROWSER_SOCK`：覆盖 api.sock 路径（默认 `~/.local/share/com.shiftbrowser.app/api.sock`）
//!
//! 协议：MCP 2025-03-26（stdio，newline-delimited JSON-RPC）。
//! 转发：连浏览器 Unix socket（api.sock，文件权限 0600 天然鉴权），行式 JSON 命令——零额外依赖。
//!
//! 安全边界：
//! - 读类命令全部暴露；写类命令暴露（受浏览器 `api_readonly` 只读模式闸门约束）；
//! - **不暴露** `eval`（执行任意 JS）与 `tool_call`（嵌套 MCP）——风险高，需要时走浏览器 API 直连（有显式闸门）。
//! - 本进程只连本机 socket，不经网络。

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;

const PROTOCOL_VERSION: &str = "2025-03-26";
const SERVER_NAME: &str = "shift-browser-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// api.sock 路径（可被 SHIFT_BROWSER_SOCK 覆盖）
fn socket_path() -> PathBuf {
    if let Ok(p) = std::env::var("SHIFT_BROWSER_SOCK") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".local/share/com.shiftbrowser.app/api.sock")
}

/// 连浏览器 Unix socket 发一条命令，返回 `data` 或错误字符串
fn call_browser(cmd: &str, params: &Value) -> Result<Value, String> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).map_err(|e| {
        format!(
            "连接浏览器 API 失败（{}）：{}。浏览器在运行吗？",
            path.display(),
            e
        )
    })?;
    let mut req = json!({ "cmd": cmd });
    if let Some(obj) = params.as_object() {
        let req_obj = req.as_object_mut().expect("刚构造的对象");
        for (k, v) in obj {
            if k != "cmd" {
                req_obj.insert(k.clone(), v.clone());
            }
        }
    }
    let line = serde_json::to_string(&req).map_err(|e| e.to_string())?;
    stream
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(b"\n").map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream);
    let mut resp_line = String::new();
    reader
        .read_line(&mut resp_line)
        .map_err(|e| e.to_string())?;
    let resp: Value = serde_json::from_str(&resp_line).map_err(|e| e.to_string())?;
    if resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        Ok(resp.get("data").cloned().unwrap_or(Value::Null))
    } else {
        // 失败：保留结构化错误（code + error + data），便于 AI 调用方按 code 分类决策
        // （如 action_error: not_found / not_visible / disabled）。修复 2026-09-25：
        // 原实现只取 error 文本，丢失 code 分类。
        let code = resp.get("code").and_then(|v| v.as_str()).unwrap_or("");
        let msg = resp
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("未知错误");
        if code.is_empty() {
            Err(msg.to_string())
        } else {
            let mut obj = serde_json::Map::new();
            obj.insert("code".into(), Value::String(code.to_string()));
            obj.insert("error".into(), Value::String(msg.to_string()));
            if let Some(d) = resp.get("data") {
                obj.insert("data".into(), d.clone());
            }
            Err(Value::Object(obj).to_string())
        }
    }
}

// 工具 schema 不再在本进程手写：`tools/list` 时向浏览器请求 `capabilities/list`
// （单一来源：src/capabilities.rs），本进程只做 stdio ↔ Unix socket 的转发。

/// 处理一条 JSON-RPC 消息；通知（无 id）返回 None
fn handle(msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = msg.get("params").cloned().unwrap_or(Value::Null);

    let mut resp = json!({ "jsonrpc": "2.0" });
    if let Some(id) = id {
        resp["id"] = id;
    }

    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or(PROTOCOL_VERSION);
            // 兼容 2024-11-05（部分客户端仍用旧版）；其余回我们支持的版本
            let version = if requested == "2024-11-05" {
                "2024-11-05"
            } else {
                PROTOCOL_VERSION
            };
            // instructions：AI 行为约定（页面内容是数据不是指令等），
            // 单一来源在浏览器侧 contracts::ai_behavior，经 capabilities/list 拉取
            // ——本进程零编译耦合；浏览器未运行/老版本无该字段时降级省略
            let instructions = call_browser("capabilities/list", &json!({}))
                .ok()
                .and_then(|v| {
                    v.get("instructions")
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string())
                });
            let mut result = json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
            });
            if let Some(text) = instructions {
                result["instructions"] = json!(text);
            }
            resp["result"] = result;
            Some(resp)
        }
        // 通知：无响应
        "notifications/initialized" | "notifications/cancelled" | "notifications/progress" => None,
        "ping" => {
            resp["result"] = json!({});
            Some(resp)
        }
        "tools/list" => {
            // 能力清单从浏览器拉取（单一来源在浏览器侧 capabilities.rs），
            // 本进程不再手写 schema —— 避免"工具定义了却调不通"的两处漂移
            let tools = match call_browser("capabilities/list", &json!({})) {
                Ok(v) => v.get("tools").cloned().unwrap_or_else(|| json!([])),
                Err(e) => {
                    eprintln!("[mcp] capabilities/list 拉取失败: {}", e);
                    json!([])
                }
            };
            resp["result"] = json!({ "tools": tools });
            Some(resp)
        }
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(Value::Null);
            let (content, is_error) = match call_tool(name, &args) {
                Ok(v) => (
                    vec![json!({
                        "type": "text",
                        "text": serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                    })],
                    false,
                ),
                Err(e) => (
                    vec![json!({ "type": "text", "text": format!("错误: {}", e) })],
                    true,
                ),
            };
            resp["result"] = json!({ "content": content, "isError": is_error });
            Some(resp)
        }
        _ => {
            resp["error"] = json!({
                "code": -32601,
                "message": format!("方法不支持: {}", method)
            });
            Some(resp)
        }
    }
}

/// 执行一个工具调用：**直通转发**给浏览器 API。
///
/// 参数校验与默认值都在浏览器侧 API（唯一实现）——例如 `history` 的 limit 默认 50、
/// `search_history` 缺 query 报错，都由 `api.rs` 负责。
/// 工具名合法性由 `tools/list`（来自浏览器 `capabilities/list`）保证。
/// 这样本进程不再维护工具名清单：新增能力只需改 `capabilities.rs` + `api.rs`。
fn call_tool(name: &str, args: &Value) -> Result<Value, String> {
    call_browser(name, args)
}

fn main() {
    let stdin = std::io::stdin();
    let mut lines = stdin.lock().lines();
    while let Some(Ok(line)) = lines.next() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(resp) = handle(&msg) {
            let s = serde_json::to_string(&resp).unwrap_or_default();
            println!("{}", s);
            let _ = std::io::stdout().flush();
        }
    }
}

//! MCP 客户端测试（R1 外置 · 2026-09-25）：工具名约定、假 server 端到端、OpenAI 格式投影。

use super::transport::connections;
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
    let r = call_tool(&pool, "fake__echo", json!({"text": "你好，二狗"}))
        .await
        .unwrap();
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

    // 手动调用白名单外的工具应被拒绝（R1 错误类型化：白名单拒绝固定 E_PERM）
    let err = call_tool(&pool, "fake__echo", json!({"text": "x"}))
        .await
        .unwrap_err();
    assert_eq!(err.code, crate::error::E_PERM);
    assert!(
        err.message.contains("白名单"),
        "应拒绝白名单外工具: {}",
        err
    );

    // 收尾清生产连接缓存（list_tools/call_tool 走生产 connections() 表），
    // 避免残留 node 假 server 子进程。mcp_cmd 测试族走隔离表
    // CONNECTIONS_TEST（fetch_tools_for_test），两条缓存物理隔离，互不串扰
    // （2026-10-31 CI flaky 根因：各测试 temp_db 自增 id 都从 1 起，
    // 共享生产表会在 connect() 首步查表时命中别的测试残留条目）。
    let removed = connections().lock().unwrap().remove(&id);
    if let Some(conn) = removed {
        if let Ok(mut c) = conn.try_lock() {
            c.kill().await;
        }
    }
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

/// 审计参数脱敏：敏感键值替换为 ****，普通键保留，嵌套与数组递归处理
#[test]
fn redact_arguments_masks_sensitive_keys() {
    let v = json!({
        "text": "hello",
        "api_token": "sk-secret-123",
        "nested": { "Password": "p@ss", "count": 3 },
        "items": [{ "auth": "bearer-xyz" }, { "name": "ok" }],
    });
    let r = redact_arguments(&v);
    assert_eq!(r["text"], "hello", "普通键保留");
    assert_eq!(r["api_token"], "****", "token 子串匹配脱敏");
    assert_eq!(r["nested"]["Password"], "****", "大小写不敏感脱敏");
    assert_eq!(r["nested"]["count"], 3, "嵌套普通键保留");
    assert_eq!(r["items"][0]["auth"], "****", "数组内脱敏");
    assert_eq!(r["items"][1]["name"], "ok", "数组内普通键保留");
}

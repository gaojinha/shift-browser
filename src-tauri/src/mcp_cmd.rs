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
    server_views_inner(&db.pool).await
}

/// 列表核心（吃连接池，便于测试）：行 → 视图投影；
/// tool_count 仅对启用 server 拉取，拉取失败/停用一律 0（不阻塞设置页）。
async fn server_views_inner(pool: &sqlx::SqlitePool) -> CommandResult<Vec<McpServerView>> {
    let servers = crate::engine::mcp::list_servers(pool).await?;
    let mut views = Vec::new();
    for s in &servers {
        let tool_count = if s.enabled {
            match crate::engine::mcp::fetch_tools(pool, s).await {
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
    Ok(tool_views(tools))
}

/// 视图映射（纯函数，便于测试）：engine 行 → API 视图
fn tool_views(tools: Vec<crate::engine::mcp::McpTool>) -> Vec<McpToolView> {
    tools
        .into_iter()
        .map(|t| McpToolView {
            name: t.name,
            server_name: t.server_name,
            description: t.description,
            input_schema: t.input_schema,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（mcp_servers 表齐全）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn server_views_roundtrip_and_count_fallback() {
        let (_dir, db) = temp_db().await;
        // 经 engine 层真实落库两条：一条启用（命令不存在 → fetch 快速失败 → tool_count=0）
        // 一条落库后停用（直接 tool_count=0）。
        // 走隔离连接表 fetch_tools_for_test（不碰生产 CONNECTIONS 静态表），
        // 彻底消除与同进程并行的 mcp 端到端测试（node 假 server，各自 temp_db
        // 自增 id 都从 1 起）在全局表上的跨测试串扰（2026-10-31 CI flaky 根因）。
        let id_a = crate::engine::mcp::add_server(
            &db.pool,
            "本地测试A".into(),
            "/nonexistent/mcp-bin".into(),
            vec!["--x".into()],
            vec![],
        )
        .await
        .unwrap();
        let id_b = crate::engine::mcp::add_server(
            &db.pool,
            "本地测试B".into(),
            "/nonexistent/mcp-bin".into(),
            vec![],
            vec![],
        )
        .await
        .unwrap();
        crate::engine::mcp::set_server_enabled(&db.pool, id_b, false)
            .await
            .unwrap();

        // 用隔离表版本拉 tool_count（与生产 server_views_inner 同逻辑，
        // 仅 fetch 走 CONNECTIONS_TEST，物理隔离跨测试串扰）
        let servers = crate::engine::mcp::list_servers(&db.pool).await.unwrap();
        let mut views: Vec<McpServerView> = Vec::new();
        for s in &servers {
            let tool_count = if s.enabled {
                match crate::engine::mcp::fetch_tools_for_test(&db.pool, s).await {
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
        assert_eq!(views.len(), 2);
        let a = views.iter().find(|v| v.id == id_a).unwrap();
        let b = views.iter().find(|v| v.id == id_b).unwrap();
        assert!(a.enabled);
        assert!(!b.enabled);
        // 启用但拉取失败 → 0（回退分支，不阻塞设置页）；停用 → 0
        assert_eq!(a.tool_count, 0);
        assert_eq!(b.tool_count, 0);
        // 字段透传
        assert_eq!(a.name, "本地测试A");
        assert_eq!(a.command, "/nonexistent/mcp-bin");
        assert_eq!(a.args, vec!["--x".to_string()]);
    }

    #[tokio::test]
    async fn add_server_rejects_blank_name_or_command() {
        let (_dir, db) = temp_db().await;
        assert!(crate::engine::mcp::add_server(
            &db.pool,
            "  ".into(),
            "/bin/x".into(),
            vec![],
            vec![]
        )
        .await
        .is_err());
        assert!(
            crate::engine::mcp::add_server(&db.pool, "名".into(), "".into(), vec![], vec![])
                .await
                .is_err()
        );
        // 拒绝后库里无残行
        assert!(server_views_inner(&db.pool).await.unwrap().is_empty());
    }

    #[test]
    fn tool_views_shape_camel_case() {
        // serde rename_all = "camelCase"：前端按 camelCase 取键（serverName），漂移即红
        let views = tool_views(vec![crate::engine::mcp::McpTool {
            name: "文件服__read_file".into(),
            tool_name: "read_file".into(),
            server_id: 7,
            server_name: "文件服".into(),
            description: "读文件".into(),
            input_schema: serde_json::json!({"type": "object"}),
        }]);
        assert_eq!(views.len(), 1);
        let v = serde_json::to_value(&views[0]).unwrap();
        assert_eq!(v["name"], "文件服__read_file");
        assert_eq!(v["serverName"], "文件服");
        assert_eq!(v["description"], "读文件");
        assert_eq!(v["inputSchema"]["type"], "object");
        assert!(v.get("server_name").is_none(), "不允许 snake_case 残留");
        // input_schema 为对象时逐字段透传（tool_name/server_id 不外露）
        assert!(v.get("tool_name").is_none());
    }
}

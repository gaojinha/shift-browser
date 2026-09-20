-- 阶段 1.3 工具层（MCP 客户端）：
-- mcp_servers：MCP server 配置（stdio 传输，本地进程）
--   command 为可执行命令，args 为 JSON 数组参数
--   allowed_tools：工具白名单 JSON 数组，空数组 = 允许全部（权限声明）
-- tool_audit_log：工具调用审计日志（谁/何时/调了啥/结果）
CREATE TABLE IF NOT EXISTS mcp_servers (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL UNIQUE,
  command TEXT NOT NULL,
  args TEXT NOT NULL DEFAULT '[]',
  enabled INTEGER NOT NULL DEFAULT 1,
  allowed_tools TEXT NOT NULL DEFAULT '[]',
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS tool_audit_log (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  server_id INTEGER,
  server_name TEXT NOT NULL,
  tool_name TEXT NOT NULL,
  arguments TEXT NOT NULL DEFAULT '{}',
  result_summary TEXT NOT NULL DEFAULT '',
  success INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_tool_audit_log_created ON tool_audit_log(created_at);
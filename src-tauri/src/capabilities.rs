// 能力注册表门面（工具 / 命令 / 动作的单一事实来源）
//
// 背景：同一批能力原先在 3 处各写一遍——MCP `tool_defs()`（25 个 JSON Schema）、
// 开放 API `match cmd`（30 个分支）、Agent 动作表（9 个）。三者号称"同名同参"，
// 但没有机制保证，新增能力要改 3 处、漏一处就"定义了工具却调不通"。
//
// R1 拆分（2026-09-25）：
//   - registry：能力清单 + Risk 分级（MCP schema / API 只读闸门同源）
//   - skills：skills/list 响应
//   - plugins：四类扩展统一视图
//   - agent_actions：AI 浏览器动作白名单
//
// 见 docs/架构-能力与常量统一治理方案_2026-09-23.md（P3）

#[path = "capabilities/registry.rs"]
mod registry;

pub(crate) use registry::{is_write, list_json};
// ids / risk_of 仅 lib 内防漂移测试使用（api/handlers tests）
#[cfg(test)]
pub(crate) use registry::{ids, risk_of};

#[path = "capabilities/skills.rs"]
mod skills;

pub use skills::skills_list_json;

#[path = "capabilities/plugins.rs"]
pub mod plugins;

#[path = "capabilities/agent_actions.rs"]
pub mod agent_actions;

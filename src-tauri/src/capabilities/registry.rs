//! 能力注册表门面（工具 / 命令的单一事实来源，R1 拆分 · 2026-09-25）
//!
//! 背景：同一批能力原先在 3 处各写一遍——MCP `tool_defs()`（25 个 JSON Schema）、
//! 开放 API `match cmd`（30 个分支）、Agent 动作表（9 个）。三者号称"同名同参"，
//! 但没有机制保证，新增能力要改 3 处、漏一处就"定义了工具却调不通"。
//!
//! 本模块把**能力清单**收敛为唯一来源（2026-09-25 三次拆分后结构）：
//!   - [`core`]：Risk 等级 + 内部 ToolDef 结构 + 统一构造器；
//!   - [`table_read`] / [`table_write`]：读 24 / 写 19 条数据条目；
//!   - 本门面：注册表拼接与查询投影（risk_of / is_write / tool_defs / ids / list_json）。
//!
//! 消费方：
//!   - MCP Server（独立二进制）通过 `capabilities/list` API 拉取 schema（零编译耦合）
//!   - 开放 API / Agent 的动作校验从这里取 id 集合
//!
//! 见 docs/架构-能力与常量统一治理方案_2026-09-23.md（P3）

#[path = "registry/core.rs"]
mod core;
#[path = "registry/table_read.rs"]
mod table_read;
#[path = "registry/table_write.rs"]
mod table_write;

use serde_json::{json, Value};

// Risk 经 pub fn risk_of 外露（API 闸门/防漂移测试消费），可见性需与函数一致
pub(crate) use core::Risk;

/// 完整注册表（读类在前、写类在后；消费方一律按 name 查找，顺序不构成契约）。
fn registry() -> Vec<core::ToolDef> {
  let mut all = table_read::defs();
  all.extend(table_write::defs());
  all
}

/// 仅开放 API 暴露、不进 MCP 清单的命令风险等级（API 闸门与防漂移测试必须覆盖）：
/// - 写：`eval`/`tool_call`（注释明确不暴露给 MCP）、`plugins/set_enabled`；
/// - 读：`tools`（列 MCP 工具清单，无状态变更）。
fn extra_risk(name: &str) -> Option<Risk> {
  match name {
    "eval" | "tool_call" | "plugins/set_enabled" => Some(Risk::Write),
    "tools" => Some(Risk::Read),
    _ => None,
  }
}

/// 查询命令风险等级（API 只读闸门唯一来源；未知命令返回 None）
pub fn risk_of(name: &str) -> Option<Risk> {
  registry()
    .iter()
    .find(|d| d.name == name)
    .map(|d| d.risk)
    .or_else(|| extra_risk(name))
}

/// 是否为写命令（`api_readonly=true` 时统一拒绝）
pub fn is_write(name: &str) -> bool {
  risk_of(name) == Some(Risk::Write)
}

/// MCP JSON 视图：把内部注册表（含风险等级）投影为工具 schema；
/// risk 是内部安全元数据，不对外暴露。
fn tool_defs() -> Vec<Value> {
  registry()
    .iter()
    .map(|d| {
      json!({
        "name": d.name,
        "description": d.description,
        "inputSchema": {
          "type": "object",
          "properties": d.props,
          "required": d.required
        }
      })
    })
    .collect()
}

/// 全部能力 id（供 API 校验 / Agent 动作白名单 / 文档生成）
#[allow(dead_code)] // 目前经 tests 与 list_json 间接覆盖；保留为公开校验入口
pub fn ids() -> Vec<String> {
  registry().iter().map(|d| d.name.to_string()).collect()
}

/// `capabilities/list` API 响应：能力清单 + schema（MCP Server 动态拉取用）
pub fn list_json() -> Value {
  let defs = tool_defs();
  let count = defs.len();
  json!({ "tools": defs, "count": count })
}

// 注册表防漂移测试随被测面外置（避免门面文件超长）
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

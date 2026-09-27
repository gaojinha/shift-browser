//! 能力注册表核心类型（R1 三次拆分 · 2026-09-25）：
//! Risk 等级、内部 ToolDef 结构与统一构造器。
//! 数据条目按读/写拆到 table_read / table_write，查询投影在 registry 门面。

use serde_json::Value;

/// 能力风险等级（决定 MCP 是否暴露、API 只读闸门、是否需要人工确认）
///
/// - `Read`/`Write`：MCP 视图与 API 只读闸门共用本枚举（2026-09-24 接入，
///   消灭 api.rs 手写第二份写命令名单的漂移风险）；
/// - `Agent`：预留给 Agent 动作校验（当前动作白名单在 `agent_actions`，
///   每步已有人工确认，尚未并入本枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Risk {
    /// 只读（低风险）
    Read,
    /// 写操作（受 `api_readonly` 只读闸门约束）
    Write,
    /// Agent 浏览器动作（需人工确认；待接入）
    #[allow(dead_code)]
    Agent,
}

/// 内部能力定义（带风险等级）；MCP JSON 视图由门面 tool_defs 投影生成，
/// API 只读闸门从 risk_of 查询——同一注册表，不再两处手写。
pub(super) struct ToolDef {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
    pub(super) props: Value,
    pub(super) required: Vec<&'static str>,
    pub(super) risk: Risk,
}

/// 条目统一构造器（读/写两张表共用，避免各处重复结构体字面量）
pub(super) fn tool(
    name: &'static str,
    description: &'static str,
    props: Value,
    required: Vec<&'static str>,
    risk: Risk,
) -> ToolDef {
    ToolDef {
        name,
        description,
        props,
        required,
        risk,
    }
}

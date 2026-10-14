//! R4-3 包 A（2026-10-14）：鉴权矩阵判定单源。
//!
//! 设计：`docs/R4-3-鉴权矩阵拆包设计_2026-10-14.md` §三 包 A。
//! 本模块收敛四处离散判定为一份实现（行为零变化）：
//! - readonly 写命令闸（原 `api.rs` 主闸门 + `http.rs` 网关闸，语义同源）
//! - agent_actions 交互闸（原 `handlers/actions.rs` 8 处 handler 内挂闸）
//! - allow_eval（原 `handlers/page.rs::eval` 内查询）
//! - model_gateway（原 `dashboard.rs::model_gateway` 内查询）
//!
//! 边界（本包不动）：
//! - `handlers/data.rs` 凭据/掩码整项拒绝（readonly 闸门③）保持原位；
//! - 未登记命令 fail-open（Gap-2）与 per-Principal 覆盖（Gap-1/Gap-4）
//!   待包 B/D 及 D-1~D-5 拍板后在此单点收口。

use tauri::{AppHandle, Manager};

/// 主体身份（设计稿 §2.1 的 P1–P4；P5 internal 走 Tauri invoke 不经 API 层，不入矩阵）。
///
/// 包 A 仅打标透传，判定不按主体分支（现状无主体粒度，Gap-1）；
/// per-Principal 差异化为包 D 范围（前置决策 D-2）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Principal {
    /// MCP 进程（api.sock，文件权限 0600 天然鉴权）
    Sock,
    /// 本机 HTTP 客户端（Bearer token；LoginGuard 豁免限速）
    LoopbackHttp,
    /// 局域网 HTTP 客户端（Bearer + IP 白名单 + LoginGuard 限速）
    LanHttp,
    /// 状态面板（Bearer / 一次性 ticket / 表单 token）。
    /// 包 A 无构造点：面板路径不执行命令、不消费 Decision，接线在包 D
    /// （拒绝审计按主体打标；对齐 `api.rs::readonly_denied` 的先例——语义已锁定，暂未接线）。
    #[allow(dead_code)]
    Dashboard,
}

impl Principal {
    /// 由对端地址推导 HTTP 主体：loopback → 本机，否则局域网。
    /// 未开 `api_bind_lan` 时实践中不存在非 loopback 对端；
    /// 推导结果仅作标识，不影响包 A 的判定。
    pub fn for_http(peer_is_loopback: bool) -> Self {
        if peer_is_loopback {
            Self::LoopbackHttp
        } else {
            Self::LanHttp
        }
    }
}

/// 设置开关快照（判定输入）。四个键各查一次（不合 IN 查询），
/// 逐键容错语义与包 A 前四处判定点完全一致：查询失败/键缺失 → false
/// （readonly 的 fail-open 见 `api::auth::is_readonly` 注释；其余三个默认关）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    pub readonly: bool,
    pub agent_actions: bool,
    pub allow_eval: bool,
    pub model_gateway: bool,
}

/// 单键读取：键值严格等于 "true" 才算开（与包 A 前四处判定逐字节同口径）。
async fn flag(app: &AppHandle, key: &'static str) -> bool {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return false;
    };
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .is_some_and(|v| v == "true")
}

impl Policy {
    pub async fn load(app: &AppHandle) -> Self {
        let (readonly, agent_actions, allow_eval, model_gateway) = tokio::join!(
            flag(app, "api_readonly"),
            flag(app, "api_agent_actions"),
            flag(app, "api_allow_eval"),
            flag(app, "api_model_gateway"),
        );
        Self {
            readonly,
            agent_actions,
            allow_eval,
            model_gateway,
        }
    }
}

/// 拒绝原因（判定结果结构化；文案由 [`DenyReason::message`] 生成，
/// 与包 A 前各闸门的错误文案逐字节一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DenyReason {
    /// 写命令 + api_readonly
    ReadonlyWrite,
    /// 模型网关 + api_readonly
    GatewayReadonly,
    /// 交互动作 + api_agent_actions 未开
    AgentActionsDisabled,
    /// eval + api_allow_eval 未开
    EvalDisabled,
    /// 模型网关 + api_model_gateway 未开
    GatewayDisabled,
}

impl DenyReason {
    pub fn message(&self, cmd: &str) -> String {
        match self {
            Self::ReadonlyWrite => {
                format!("只读模式已开启，写命令 {cmd} 被拒绝")
            }
            Self::GatewayReadonly => "只读模式已开启，模型网关被拒绝".to_string(),
            Self::AgentActionsDisabled => {
                "交互能力未开启：需在设置里开启「允许 API 页面交互」(api_agent_actions)".to_string()
            }
            Self::EvalDisabled => {
                "执行 JS 已禁用：需在设置里显式开启「允许 API 执行 JS」".to_string()
            }
            Self::GatewayDisabled => {
                "模型网关已禁用：需在设置里显式开启「允许模型网关」".to_string()
            }
        }
    }
}

/// 判定结果
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny(DenyReason),
}

/// 受 `api_agent_actions` 约束的交互命令集合（恰为包 A 前 actions.rs 内挂闸的 8 个 handler）。
///
/// 现状例外（包 A 如实复刻，不"顺手修正"），共三处：
///
/// - `action/find`（Risk=Read）不挂闸——只读模式下脑也必须能"找"（actions.rs 注释）；
/// - `action/wait_for`（Risk=Read，"按读管理"）**挂闸**——历史如此，如实保留；
/// - `action/dismiss`（Risk=Write，handler 在 page.rs）不挂闸——保守弹窗关闭，历史如此。
///
/// 防漂移测试锁定：写表 action/* == 本集合 − {wait_for} ∪ {dismiss}。
const AGENT_ACTION_GATED: &[&str] = &[
    "action/wait_for",
    "action/scroll",
    "action/click",
    "action/fill",
    "action/back",
    "action/hover",
    "action/key",
    "action/type",
];

/// 命令闸门判定（纯函数，主体 × 命令 × 开关 → 决策）。
///
/// 闸门顺序与包 A 前一致：入口先 readonly（写命令），再 agent_actions，再 eval——
/// 例如 action/click 在 readonly+agent_actions 双关时，包 A 前命中的就是 readonly
/// 拒绝（入口闸先于 handler 闸），本函数同序复刻。
pub fn can(_principal: Principal, cmd: &str, p: &Policy) -> Decision {
    if crate::capabilities::is_write(cmd) && p.readonly {
        return Decision::Deny(DenyReason::ReadonlyWrite);
    }
    if AGENT_ACTION_GATED.contains(&cmd) && !p.agent_actions {
        return Decision::Deny(DenyReason::AgentActionsDisabled);
    }
    if cmd == "eval" && !p.allow_eval {
        return Decision::Deny(DenyReason::EvalDisabled);
    }
    Decision::Allow
}

/// 模型网关闸门判定（`POST /v1/chat/completions` 不是 HANDLERS 命令，
/// 走独立入口；readonly 约束口径与命令闸同源——网关携带 key 调上游，
/// 属写/外联原语，不能因不在 HANDLERS 表就漏网）。
pub fn can_gateway(_principal: Principal, p: &Policy) -> Decision {
    if p.readonly {
        return Decision::Deny(DenyReason::GatewayReadonly);
    }
    Decision::Allow
}

/// 模型网关开关判定（`api_model_gateway`，默认关）。
/// 原 `dashboard.rs::model_gateway` 内联查询收口至此（R4-3 包 A）。
pub fn gateway_switch(p: &Policy) -> Decision {
    if p.model_gateway {
        Decision::Allow
    } else {
        Decision::Deny(DenyReason::GatewayDisabled)
    }
}

#[cfg(test)]
mod tests {
    use super::{can, can_gateway, Decision, DenyReason, Policy, Principal};

    fn p(readonly: bool, agent_actions: bool, allow_eval: bool, model_gateway: bool) -> Policy {
        Policy {
            readonly,
            agent_actions,
            allow_eval,
            model_gateway,
        }
    }

    const ALL_PRINCIPALS: &[Principal] = &[
        Principal::Sock,
        Principal::LoopbackHttp,
        Principal::LanHttp,
        Principal::Dashboard,
    ];

    #[test]
    fn readonly_blocks_write_commands_for_every_principal() {
        let pol = p(true, true, true, true);
        for who in ALL_PRINCIPALS {
            for cmd in ["new_tab", "close_tab", "navigate", "action/click"] {
                assert_eq!(
                    can(*who, cmd, &pol),
                    Decision::Deny(DenyReason::ReadonlyWrite),
                    "{who:?} {cmd}"
                );
            }
        }
    }

    #[test]
    fn readonly_spares_read_commands_and_unknown() {
        let pol = p(true, true, true, true);
        for who in ALL_PRINCIPALS {
            for cmd in ["list_tabs", "read_page", "history", "no_such_cmd"] {
                assert_eq!(can(*who, cmd, &pol), Decision::Allow, "{who:?} {cmd}");
            }
        }
    }

    #[test]
    fn agent_actions_gate_applies_only_to_gated_set() {
        let pol = p(false, false, true, true);
        for who in ALL_PRINCIPALS {
            for cmd in AGENT_ACTION_GATED_CMD_SAMPLES {
                assert_eq!(
                    can(*who, cmd, &pol),
                    Decision::Deny(DenyReason::AgentActionsDisabled),
                    "{who:?} {cmd}"
                );
            }
            // 现状例外：find / dismiss 不受 agent_actions 约束
            assert_eq!(can(*who, "action/find", &pol), Decision::Allow);
            assert_eq!(can(*who, "action/dismiss", &pol), Decision::Allow);
            // 开关打开后放行
            let on = p(false, true, true, true);
            assert_eq!(can(*who, "action/click", &on), Decision::Allow);
        }
    }

    const AGENT_ACTION_GATED_CMD_SAMPLES: &[&str] = &[
        "action/wait_for",
        "action/scroll",
        "action/click",
        "action/fill",
        "action/back",
        "action/hover",
        "action/key",
        "action/type",
    ];

    #[test]
    fn eval_gate_follows_allow_eval() {
        let off = p(false, true, false, true);
        for who in ALL_PRINCIPALS {
            assert_eq!(
                can(*who, "eval", &off),
                Decision::Deny(DenyReason::EvalDisabled)
            );
        }
        let on = p(false, true, true, true);
        assert_eq!(can(Principal::Sock, "eval", &on), Decision::Allow);
    }

    #[test]
    fn readonly_wins_over_agent_actions_and_eval() {
        // 双关时包 A 前命中入口 readonly 闸；can() 同序复刻
        let pol = p(true, false, false, true);
        assert_eq!(
            can(Principal::LoopbackHttp, "action/click", &pol),
            Decision::Deny(DenyReason::ReadonlyWrite)
        );
        assert_eq!(
            can(Principal::LoopbackHttp, "eval", &pol),
            Decision::Deny(DenyReason::ReadonlyWrite)
        );
    }

    #[test]
    fn gateway_readonly_gate() {
        let on = p(true, true, true, true);
        for who in ALL_PRINCIPALS {
            assert_eq!(
                can_gateway(*who, &on),
                Decision::Deny(DenyReason::GatewayReadonly)
            );
        }
        let off = p(false, true, true, true);
        assert_eq!(can_gateway(Principal::LanHttp, &off), Decision::Allow);
    }

    #[test]
    fn deny_messages_match_pre_package_a_wording() {
        assert_eq!(
            DenyReason::ReadonlyWrite.message("new_tab"),
            "只读模式已开启，写命令 new_tab 被拒绝"
        );
        assert_eq!(
            DenyReason::GatewayReadonly.message(""),
            "只读模式已开启，模型网关被拒绝"
        );
        assert_eq!(
            DenyReason::AgentActionsDisabled.message(""),
            "交互能力未开启：需在设置里开启「允许 API 页面交互」(api_agent_actions)"
        );
        assert_eq!(
            DenyReason::EvalDisabled.message(""),
            "执行 JS 已禁用：需在设置里显式开启「允许 API 执行 JS」"
        );
        assert_eq!(
            DenyReason::GatewayDisabled.message(""),
            "模型网关已禁用：需在设置里显式开启「允许模型网关」"
        );
    }

    /// 防漂移：写表 action/*（capabilities Risk::Write 且 action/ 前缀）
    /// == AGENT_ACTION_GATED − {action/wait_for} ∪ {action/dismiss}。
    /// 三个现状例外的语义见 AGENT_ACTION_GATED 文档注释；registry 增删交互
    /// 命令而忘改本表时，本测试抓红（对应包 B 的一致性测试先行动作）。
    #[test]
    fn agent_action_gated_set_matches_registry() {
        use crate::capabilities::{ids, is_write};

        let write_actions: Vec<String> = ids()
            .into_iter()
            .filter(|id| id.starts_with("action/") && is_write(id))
            .collect();
        let mut expected: Vec<String> = super::AGENT_ACTION_GATED
            .iter()
            .map(|s| s.to_string())
            .filter(|s| s != "action/wait_for") // 现状例外：Read 也挂闸
            .chain(std::iter::once("action/dismiss".to_string())) // 现状例外：Write 不挂闸
            .collect();
        expected.sort();
        let mut actual = write_actions;
        actual.sort();
        assert_eq!(actual, expected, "交互闸集合与 capabilities 写表漂移");
    }
}

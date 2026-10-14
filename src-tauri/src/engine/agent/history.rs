//! Agent 动作历史（R1 拆分：从 engine/agent.rs 抽出，逻辑零改动）
//!
//! 动作历史（阶段 1.8：回放/重试的可信地基）
//!
//! 每次 Agent 动作确认执行后，由命令层调用 `record_action` 落一条快照
//! （动作名 + 参数 + 结果 + 时间戳），AI/用户可回看、回放、重试。
//! 安全：快照只存动作参数与结果文本，**不存页面凭据/密码**（铁律 1.4）。
//!
//! 历史是**审计链路**的一部分（对齐倒查/零遥测承诺）：重放/重试
//! 必须重新走 `execute_agent_action`，不提供绕过窗口/标签的捷径。

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use super::{AgentActionParams, AgentStep, MultiStepPlan};
use crate::error::{AppError, CommandResult};

/// 单条动作历史记录
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ActionHistoryItem {
    /// 自增步号（回放定位用）
    pub step: u64,
    /// 动作名（click/fill/...）
    pub action: String,
    pub selector: Option<String>,
    pub value: Option<String>,
    pub direction: Option<String>,
    pub url: Option<String>,
    pub ms: Option<u64>,
    /// 执行结果文本（给 AI 回看）
    pub result: String,
    /// unix 秒时间戳
    pub ts: i64,
    /// 是否成功（结果不含"失败/错误"）
    pub ok: bool,
}

/// 环形历史缓冲（最多 128 条，超限丢最旧）
static ACTION_HISTORY: OnceLock<Mutex<VecDeque<ActionHistoryItem>>> = OnceLock::new();
/// 自增步号
static ACTION_STEP: OnceLock<Mutex<u64>> = OnceLock::new();

fn action_history() -> std::sync::MutexGuard<'static, VecDeque<ActionHistoryItem>> {
    ACTION_HISTORY
        .get_or_init(|| Mutex::new(VecDeque::with_capacity(128)))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn action_step_no() -> std::sync::MutexGuard<'static, u64> {
    ACTION_STEP
        .get_or_init(|| Mutex::new(0))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 记录一条动作（命令层每次执行后调用；返回本步号）
pub fn record_action(params: &AgentActionParams, result: &str) -> u64 {
    let step = {
        let mut n = action_step_no();
        *n += 1;
        *n
    };
    let item = ActionHistoryItem {
        step,
        action: params.action.clone(),
        selector: params.selector.clone(),
        value: params.value.clone(),
        direction: params.direction.clone(),
        url: params.url.clone(),
        ms: params.ms,
        result: result.to_string(),
        ts: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        ok: !result.starts_with("失败") && !result.contains("错误") && !result.contains("未找到"),
    };
    let mut h = action_history();
    h.push_back(item);
    while h.len() > 128 {
        h.pop_front();
    }
    step
}

/// 读取历史（最新在前，limit 默认 50）
pub fn action_history_list(limit: Option<usize>) -> Vec<ActionHistoryItem> {
    let h = action_history();
    let n = limit.unwrap_or(50).min(h.len());
    h.iter().rev().take(n).cloned().collect()
}

/// 按步号取回参数（回放/重试用；找不到返回 Err）
pub fn rebuild_params(step: u64) -> CommandResult<AgentActionParams> {
    let h = action_history();
    let item = h
        .iter()
        .find(|i| i.step == step)
        .ok_or_else(|| AppError::validation(format!("没有第 {} 步的历史", step)))?;
    Ok(AgentActionParams {
        action: item.action.clone(),
        selector: item.selector.clone(),
        value: item.value.clone(),
        direction: item.direction.clone(),
        url: item.url.clone(),
        ms: item.ms,
    })
}

/// 最近一次失败动作的步号（重试用；无则 Err）
pub fn last_failed_step() -> CommandResult<u64> {
    let h = action_history();
    h.iter()
        .rev()
        .find(|i| !i.ok)
        .map(|i| i.step)
        .ok_or_else(|| AppError::validation("没有失败过的动作"))
        .map_err(Into::into)
}

/// 从历史里挑出**所有失败步**并重建参数（整批重试地基）。
/// 先收集步号再重建，避免与 `rebuild_params` 重复加锁（std Mutex 不可重入）。
pub fn failed_action_params() -> CommandResult<Vec<(u64, AgentActionParams)>> {
    let steps: Vec<u64> = {
        let h = action_history();
        h.iter().filter(|i| !i.ok).map(|i| i.step).collect()
    };
    if steps.is_empty() {
        return Err(AppError::validation("历史里没有失败过的动作").into());
    }
    steps
        .into_iter()
        .map(|s| rebuild_params(s).map(|p| (s, p)))
        .collect()
}

/// 把批量重建的失败步参数转成多步计划（供 `request_multi_step_action` 整批确认）。
pub fn batch_retry_plan(pairs: &[(u64, AgentActionParams)]) -> MultiStepPlan {
    let steps = pairs
        .iter()
        .map(|(_, p)| AgentStep {
            step: 0,
            action: p.action.clone(),
            selector: p.selector.clone(),
            value: p.value.clone(),
            direction: p.direction.clone(),
            url: p.url.clone(),
            ms: p.ms,
        })
        .collect();
    MultiStepPlan {
        steps,
        description: format!("整批重试 {} 个失败动作", pairs.len()),
    }
}

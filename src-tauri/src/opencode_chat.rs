//! 侧边栏聊天的本机 opencode 推理 Provider（provider id：`opencode`）。
//!
//! 与 [`crate::webqa`]（网页随手问）的分工：随手问是"前端持 sid 的多轮追问"；
//! 本模块实现统一 [`AiProvider`] 契约，让设置里选「本机 opencode」后，侧边栏
//! 聊天整体走本机 opencode serve（127.0.0.1:4096）。
//!
//! 多轮策略（2026-09-26 实测结论）：opencode 无法向新会话播种 assistant 角色
//! 历史——`POST /message`（noReply）写入的消息一律是 user 角色。因此每次请求
//! 新建一个会话，历史以"转录文本"随首条 prompt 发出；system 消息走 prompt 的
//! `system` 字段。会话都保留在 opencode 侧（标题固定，便于事后在控制台翻看）。
//!
//! 错误识别：上游失败（免费额度用尽、provider 配置错误等）不会产生 assistant
//! 文本，只发 `session.status`（type=retry/error，带 message）并最终 idle。
//! 若只按 idle 收尾就会得到"空回复"，故这里额外捕获 status 错误并抛出。
//!
//! 凭据完全复用 webqa 的发现逻辑（设置项优先，/proc 自动发现兜底）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};

// re-export：真机集成测试（tests/opencode_chat_live.rs）通过本模块取得 trait/类型
pub use crate::engine::ai_provider::{AiProvider, ProviderRequest};
pub use crate::engine::types::ChatMessage;
use crate::error::{AppError, CommandResult};
use crate::webqa::DEFAULT_AGENT;
use crate::webqa::{
    base_url_pub, client_pub, ensure_agent_exists_pub, fold_event, Credentials, SseAccumulator,
};

/// SSE 等待上限：与 webqa 一致，防"迟迟不 idle"挂住协程。
const SSE_MAX_SECS: u64 = 120;

/// 侧边栏会话在 opencode 控制台里的固定标题。
const SESSION_TITLE: &str = "Shift 侧边栏对话";

/// 本机 opencode Provider。
///
/// `model` = `(providerID, modelID)`；`None` 表示不随请求指定模型，
/// 跟随 opencode `/config` 的当前默认。
pub struct OpencodeProvider {
    creds: Credentials,
    model: Option<(String, String)>,
}

impl OpencodeProvider {
    /// `model_ref` 形如 `"opencode/big-pickle"`；空串/非法 = 跟随默认模型。
    pub fn new(creds: Credentials, model_ref: &str) -> Self {
        Self {
            creds,
            model: parse_model_ref(model_ref),
        }
    }

    /// 一次完整问答：建会话 → 订阅 SSE → 发问 → 收敛增量。
    /// 无状态（每次新会话），多轮上下文由 [`render_transcript`] 转录进 prompt。
    ///
    /// 回调按值泛型持有（而非 `&dyn Fn`）：boxed 回调只 `Send` 不 `Sync`，
    /// 跨 await 持引用要求 `Sync`，按值持有只需 `Send`。
    async fn run_once<F: Fn(String) + Send>(
        &self,
        messages: &[ChatMessage],
        cancel: &AtomicBool,
        on_delta: F,
    ) -> CommandResult<String> {
        let (system, prompt) = render_transcript(messages);
        if prompt.trim().is_empty() {
            return Err(AppError::validation("没有可发送的对话内容").into());
        }
        let http = client_pub(&self.creds)?;
        // 与随手问相同的只读边界：plan 不存在时拒绝发问（未知 agent 会被
        // opencode 静默回退成有编辑权限的默认 agent）。
        ensure_agent_exists_pub(&http, DEFAULT_AGENT).await?;

        // 新建会话（无 baseline：本会话只有这一轮问答）
        let resp = http
            .post(format!("{}/session", base_url_pub()))
            .json(&json!({ "title": SESSION_TITLE }))
            .send()
            .await
            .map_err(|e| AppError::net(format!("建 opencode 会话失败：{e}")))?;
        if !resp.status().is_success() {
            return Err(
                AppError::net(format!("建 opencode 会话失败：HTTP {}", resp.status())).into(),
            );
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|e| AppError::net(format!("解析会话响应失败：{e}")))?;
        let sid = body
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| AppError::net("opencode 会话响应里没有 id"))?
            .to_string();

        // 先开 SSE 再发问：顺序反了会漏掉开头的事件
        let sse = http
            .get(format!("{}/event", base_url_pub()))
            .send()
            .await
            .map_err(|e| AppError::net(format!("订阅 opencode 事件流失败：{e}")))?;

        // prompt_async 的模型字段是 {providerID, modelID}（与建会话的
        // ModelRef {id, providerID} 不同，实测 2026-09-26）。
        let mut payload = json!({
            "agent": DEFAULT_AGENT,
            "parts": [{ "type": "text", "text": prompt }],
        });
        if let Some(sys) = system.filter(|s| !s.trim().is_empty()) {
            payload["system"] = Value::String(sys);
        }
        if let Some((pid, mid)) = &self.model {
            payload["model"] = json!({ "providerID": pid, "modelID": mid });
        }

        let post = http
            .post(format!("{}/session/{}/prompt_async", base_url_pub(), sid))
            .json(&payload)
            .send()
            .await
            .map_err(|e| AppError::net(format!("发问失败：{e}")))?;
        if !post.status().is_success() {
            return Err(AppError::net(format!("opencode 拒收提问：HTTP {}", post.status())).into());
        }

        drive_sse(&sid, sse, cancel, on_delta).await
    }
}

#[async_trait]
impl AiProvider for OpencodeProvider {
    fn id(&self) -> &str {
        "opencode"
    }
    fn name(&self) -> &str {
        "本机 opencode"
    }
    fn is_configured(&self) -> bool {
        // 凭据在构造前已解析成功；模型可选（跟随默认），故恒为 true。
        true
    }

    async fn chat(&self, req: &ProviderRequest) -> CommandResult<ChatMessage> {
        let flag = AtomicBool::new(false);
        let full = self.run_once(&req.messages, &flag, |_| {}).await?;
        Ok(ChatMessage {
            role: "assistant".to_string(),
            content: full,
            tool_call_id: None,
            tool_calls: None,
        })
    }

    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> CommandResult<String> {
        self.run_once(&req.messages, &cancel, move |d| {
            let _ = on_delta(d);
        })
        .await
    }
}

/// 读 SSE 直到 `session.idle`。事件语义复用 webqa 的 [`fold_event`]；
/// 另外捕获 `session.status` 的 retry/error 消息，空回复时作为错误抛出
/// （否则上游额度/配置故障会表现成"模型没说话"，无法排查）。
async fn drive_sse<F: Fn(String) + Send>(
    sid: &str,
    sse: reqwest::Response,
    cancel: &AtomicBool,
    on_delta: F,
) -> CommandResult<String> {
    // async move：sse/on_delta 按值进块（只需 Send）；若只借用回调，
    // 块对它持 &F，会要求 F: Sync（boxed 回调只有 Send）。
    let loop_res: Result<Result<String, AppError>, tokio::time::error::Elapsed> =
        tokio::time::timeout(Duration::from_secs(SSE_MAX_SECS), async move {
            let mut stream = sse.bytes_stream();
            let mut acc = SseAccumulator::new();
            let mut target_msg: Option<String> = None;
            let baseline: Vec<String> = Vec::new();
            let mut full = String::new();
            let mut last_error: Option<String> = None;
            while let Some(chunk) = stream.next().await {
                if cancel.load(Ordering::Relaxed) {
                    return Err(AppError::net("已停止生成".to_string()));
                }
                let bytes = chunk.map_err(|e| AppError::net(format!("读事件流失败：{e}")))?;
                for data in acc.push(&bytes) {
                    let Ok(ev) = serde_json::from_str::<Value>(&data) else {
                        continue;
                    };
                    // 永久性失败（免费额度耗尽等）立即中止——这类 retry 的下次
                    // 尝试在数小时后，干等只会吃满 120 秒超时
                    if let Some(fatal) = fatal_status(&ev) {
                        return Err(AppError::net(format!("opencode 未返回内容：{fatal}")));
                    }
                    capture_status_error(&ev, &mut last_error);
                    if let Some((delta, done)) = fold_event(&ev, sid, &mut target_msg, &baseline) {
                        if let Some(d) = delta {
                            full.push_str(&d);
                            on_delta(d);
                        }
                        if done {
                            if full.trim().is_empty() {
                                if let Some(msg) = last_error {
                                    return Err(AppError::net(format!(
                                        "opencode 未返回内容：{msg}"
                                    )));
                                }
                            }
                            return Ok(full);
                        }
                    }
                }
            }
            Err(AppError::net(format!(
                "事件流在回答完成前断开（已收到 {} 字）",
                full.chars().count()
            )))
        })
        .await;

    match loop_res {
        Ok(inner) => inner.map_err(Into::into),
        Err(_) => Err(AppError::net(format!("等待回答超时（{} 秒）", SSE_MAX_SECS)).into()),
    }
}

/// 判断 `session.status` 是否为"不必再等"的永久失败。
///
/// opencode 的瞬时故障也发 retry（随后自愈、idle）；但额度/配额类失败带
/// `action.reason`（如 free_tier_limit）或把 `next` 重试时刻定在数十分钟后，
/// 本轮已无成功可能，直接抛出比干等超时体验好得多。
fn fatal_status(ev: &Value) -> Option<String> {
    if ev.get("type").and_then(Value::as_str) != Some("session.status") {
        return None;
    }
    let status = ev.get("properties")?.get("status")?;
    let kind = status.get("type").and_then(Value::as_str)?;
    let message = status
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("上游错误")
        .to_string();
    if kind == "error" {
        return Some(message);
    }
    if kind != "retry" {
        return None;
    }
    // 带结构化原因（free_tier_limit 等）= 已知永久失败
    if status
        .pointer("/action/reason")
        .and_then(Value::as_str)
        .is_some()
    {
        return Some(message);
    }
    // next 是下次重试的毫秒时间戳；远在 60 秒后也按永久失败处理
    if let Some(next) = status.get("next").and_then(Value::as_i64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if next > now + 60_000 {
            return Some(message);
        }
    }
    None
}

/// 从 `session.status` 事件里提取上游错误（retry/error 带 message）。
fn capture_status_error(ev: &Value, last_error: &mut Option<String>) {
    if ev.get("type").and_then(Value::as_str) != Some("session.status") {
        return;
    }
    let Some(status) = ev.get("properties").and_then(|p| p.get("status")) else {
        return;
    };
    let kind = status.get("type").and_then(Value::as_str).unwrap_or("");
    if matches!(kind, "retry" | "error") {
        if let Some(msg) = status.get("message").and_then(Value::as_str) {
            if !msg.trim().is_empty() {
                *last_error = Some(msg.to_string());
            }
        }
    }
}

/// 解析模型引用串：`"providerID/modelID"`，在**第一个** `/` 处切分
/// （modelID 本身可含 `/`，如 `cloudflare/@cf/meta/llama-3.2-1b-instruct`）。
pub fn parse_model_ref(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (pid, mid) = s.split_once('/')?;
    if pid.trim().is_empty() || mid.trim().is_empty() {
        return None;
    }
    Some((pid.to_string(), mid.to_string()))
}

/// 把统一消息数组转录成 `(system, prompt)`。
///
/// - 所有 system 消息合并进 system（走 opencode prompt 的 `system` 字段）；
/// - 单轮（只有一条用户消息）时不套转录外壳，原样发问；
/// - 多轮时历史按【用户】/【助手】排版，最后一条即当前问题。
///   只保留 user/assistant 且非空的消息。
pub fn render_transcript(messages: &[ChatMessage]) -> (Option<String>, String) {
    let system_parts: Vec<&str> = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.trim())
        .filter(|c| !c.is_empty())
        .collect();
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };

    let turns: Vec<(String, String)> = messages
        .iter()
        .filter(|m| matches!(m.role.as_str(), "user" | "assistant"))
        .map(|m| (m.role.clone(), m.content.trim().to_string()))
        .filter(|(_, c)| !c.is_empty())
        .collect();

    // 没有任何有效发言：返回空 prompt，由调用方在发 HTTP 前拒绝
    if turns.is_empty() {
        return (system, String::new());
    }
    // 纯单轮：直接发问，不加任何会稀释指令的外壳
    if turns.len() == 1 && turns[0].0 == "user" {
        return (system, turns[0].1.clone());
    }

    let mut out =
        String::from("以下是此前的对话记录，请结合上下文直接回答最后的问题，不要复述记录：\n\n");
    for (role, content) in &turns {
        let label = if role == "assistant" {
            "助手"
        } else {
            "用户"
        };
        out.push_str(&format!("【{label}】{content}\n\n"));
    }
    (system, out.trim_end().to_string())
}

// 模型清单解析与设置页命令独立成子模块（控制单文件规模）
// pub(crate)：generate_handler! 要求命令路径在 crate 内可公开解析
pub(crate) mod models;
// parse_models/OcModelInfo 供单测（super::）与真机测试复用；
// 命令注册走完整路径 opencode_chat::models::opencode_list_models
// （#[tauri::command] 生成的 __cmd__ 模块不随 pub use 再导出）
pub use models::{parse_models, OcModelInfo};

#[cfg(test)]
mod tests;

//! Ollama 本地推理实现（第七件 · 最小可用，2026-09-27）
//!
//! 对接本机 Ollama 的 OpenAI 兼容端点（`{base}/chat/completions`），
//! base 默认 `http://127.0.0.1:11434/v1`。注意两点与 agnes 的差异：
//! - 不走 SSRF 校验：localhost 就是目标（豁免理由见 [`resolve_runtime_provider`]
//!   的 local 分支注释）；base 写死回环默认，调用方透传的外部地址一律忽略，
//!   防止"本地模型"被指到外网。
//! - 错误复用 [`AiError`] 分类：连接被拒/超时 → 传输失败，ai.rs 据此回退云端；
//!   上游有响应的错误（404 模型不存在等）→ 不回退，原样抛给前端。
//!
//! SSE 流式循环是 agnes 管线的精简镜像（同为 OpenAI 格式，同为 AiError 类型，
//! 后续若漂移再抽共享函数，最小可用阶段先各自独立）。

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use crate::engine::ai_provider::ProviderRequest;
use crate::engine::types::ChatMessage;

use super::agnes::{classify_send_error, classify_status, truncate_body, AiError};

/// 本地默认模型（Ollama 需显式模型名；用户在设置页 ai_model 里改）。
/// 默认端点不在此重复定义——回环固定值单一来源在 `engine::ai_provider::OLLAMA_DEFAULT_BASE`。
pub(crate) const OLLAMA_DEFAULT_MODEL: &str = "llama3.1";

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    /// function calling 工具定义（OpenAI 格式；Ollama 部分模型支持，不支持的会忽略）
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatStreamResponse {
    choices: Vec<ChatStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamChoice {
    delta: Option<ChatStreamDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatStreamDelta {
    content: Option<String>,
}

fn client(timeout_secs: u64) -> Result<reqwest::Client, AiError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        // 建连上限（回环地址瞬连，此处仅为全仓统一口径，见 contracts::timeout::CONNECT_TIMEOUT_SECS）
        .connect_timeout(std::time::Duration::from_secs(
            crate::contracts::timeout::CONNECT_TIMEOUT_SECS,
        ))
        .build()
        .map_err(|e| AiError::Internal(format!("HTTP 客户端创建失败: {}", e)))
}

/// Ollama 非流式对话实现（provider 内部调用）
pub(crate) async fn ollama_chat_impl(
    base_url: &str,
    req: &ProviderRequest,
) -> Result<ChatMessage, AiError> {
    // 回环断言：base 只允许出自 resolve 的固定值，外部透传不进此函数
    if !is_loopback_base(base_url) {
        return Err(AiError::Permission(format!(
            "本地模型地址不允许指向非回环: {}",
            base_url
        )));
    }
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let http = client(req.timeout_secs.unwrap_or(90))?;
    let req_body = ChatRequest {
        model: req.model.clone(),
        messages: req.messages.clone(),
        stream: false,
        tools: req.tools.clone(),
    };

    let resp = http
        .post(url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| classify_send_error(e.is_timeout(), &e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::error!("[ollama] 服务错误 {}: {}", status, truncate_body(&body));
        return Err(classify_status(status.as_u16(), &body));
    }

    let data: ChatResponse = resp
        .json()
        .await
        .map_err(|e| AiError::Net(format!("AI 响应解析失败: {}", e)))?;

    let reply = data
        .choices
        .into_iter()
        .next()
        .map(|c| c.message)
        .ok_or_else(|| AiError::Net("AI 无回复".to_string()))?;
    Ok(reply)
}

/// Ollama 流式对话实现（provider 内部调用）。
/// `cancel` 为 true 时中断；`on_delta` 每块增量回调，返回 Err 表示前端通道已关闭。
pub(crate) async fn ollama_chat_stream_impl<F>(
    base_url: &str,
    req: &ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: F,
) -> Result<String, AiError>
where
    F: Fn(String) -> Result<(), ()>,
{
    if !is_loopback_base(base_url) {
        return Err(AiError::Permission(format!(
            "本地模型地址不允许指向非回环: {}",
            base_url
        )));
    }
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let http = client(req.timeout_secs.unwrap_or(120))?;
    let req_body = ChatRequest {
        model: req.model.clone(),
        messages: req.messages.clone(),
        stream: true,
        tools: req.tools.clone(),
    };

    let resp = http
        .post(url)
        .json(&req_body)
        .send()
        .await
        .map_err(|e| classify_send_error(e.is_timeout(), &e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::error!("[ollama] 流式服务错误 {}: {}", status, truncate_body(&body));
        return Err(classify_status(status.as_u16(), &body));
    }

    // SSE 逐块解析（与 agnes 同格式：data: {...} / data: [DONE]）
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full = String::new();
    let mut done = false;

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            log::info!("[ollama] 流式请求被用户取消");
            return Err(AiError::Net("已停止生成".to_string()));
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => return Err(AiError::Net(format!("流读取失败: {}", e))),
        };
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].to_string();
            buf = buf[pos + 1..].to_string();
            let line = line.trim();
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if data == "[DONE]" {
                    done = true;
                    break;
                }
                if let Ok(parsed) = serde_json::from_str::<ChatStreamResponse>(data) {
                    if let Some(delta) = parsed.choices.first().and_then(|c| c.delta.as_ref()) {
                        if let Some(content) = delta.content.as_ref() {
                            if !content.is_empty() {
                                full.push_str(content);
                                if on_delta(content.clone()).is_err() {
                                    log::info!("[ollama] 前端已关闭流式通道，停止推送");
                                    return Ok(full);
                                }
                            }
                        }
                    }
                }
            }
        }
        if done {
            break;
        }
    }

    if done {
        Ok(full)
    } else {
        Err(AiError::Net("AI 响应流意外中断".to_string()))
    }
}

/// 回环断言：只放行 127.0.0.0/8 与 localhost（纯函数，可单测）。
/// base 由后端固定，不经过用户输入；此函数是纵深兜底。
pub(crate) fn is_loopback_base(base_url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(base_url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return ip.is_loopback();
    }
    let h = host.to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost")
}

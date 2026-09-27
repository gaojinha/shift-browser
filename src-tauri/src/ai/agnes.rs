//! AGNES（OpenAI 兼容）聊天实现（R1 拆分 · 2026-09-25）
//!
//! 非流式/SSE 流式两条 HTTP 管线，均含 SSRF 校验与 DNS pin（见 super::ssrf）。
//! 命令面（ai_chat / ai_chat_stream）在 ai.rs 门面经 Provider 抽象路由到这里。

use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};

use crate::engine::ai_provider::ProviderRequest;
use crate::engine::types::ChatMessage;

use super::ssrf::{resolve_pin_for_https, validate_base_url};

#[derive(Debug, Serialize, Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    /// function calling 工具定义（OpenAI 格式）
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

/// AGNES 上游失败分类（第五件后半段：错误码透出）。
///
/// 旧实现两条管线返回裸 String，ai.rs 注册闭包统一压成 E_NET，
/// 前端无法区分"限流稍后重试"与"key 错了去设置页"。
/// 新口径：超时用 `e.is_timeout()` 识别 → E_TIMEOUT；401/403 → E_AUTH；
/// 429 → E_RATELIMIT；SSRF 校验失败 → E_PERM；其余按类归（见分类函数）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AiError {
    Timeout(String),
    Auth(String),
    RateLimited(String),
    Permission(String),
    Net(String),
    Internal(String),
}

impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl AiError {
    /// 稳定错误码（与 error.rs 八码对应）
    pub(crate) fn code(&self) -> &'static str {
        match self {
            AiError::Timeout(_) => crate::error::E_TIMEOUT,
            AiError::Auth(_) => crate::error::E_AUTH,
            AiError::RateLimited(_) => crate::error::E_RATELIMIT,
            AiError::Permission(_) => crate::error::E_PERM,
            AiError::Net(_) => crate::error::E_NET,
            AiError::Internal(_) => crate::error::E_INTERNAL,
        }
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            AiError::Timeout(m)
            | AiError::Auth(m)
            | AiError::RateLimited(m)
            | AiError::Permission(m)
            | AiError::Net(m)
            | AiError::Internal(m) => m,
        }
    }
}

impl From<AiError> for crate::error::CommandError {
    // 单一转换口：码与消息都走 AiError::code()/message()，
    // 与 error.rs 八码同源（AppError 侧由 CommandError→AppError 臂透传）。
    fn from(e: AiError) -> Self {
        Self {
            code: e.code(),
            message: e.message().to_string(),
        }
    }
}

/// 错误体截断（按字符非字节：中文多字节字节切片会 panic；200 字符上限沿用旧值）。
pub(crate) fn truncate_body(body: &str) -> String {
    if body.chars().count() > 200 {
        let truncated: String = body.chars().take(200).collect();
        format!("{}…(截断)", truncated)
    } else {
        body.to_string()
    }
}

/// HTTP 状态码分类（纯函数，可单测）：
/// 401/403 → 鉴权；429 → 限流；其余 → 网络/上游（消息沿用旧格式，前端 toast 不变）。
pub(crate) fn classify_status(status: u16, body: &str) -> AiError {
    let body = truncate_body(body);
    match status {
        401 | 403 => AiError::Auth(format!(
            "AI 鉴权失败（{}）：API Key 无效或已过期，请检查设置。{}",
            status,
            if body.is_empty() { String::new() } else { format!("（{}）", body) }
        )),
        429 => AiError::RateLimited(
            "AI 服务限流（429）：请求太频繁，请稍等片刻再试。若持续出现，可能是当日额度已用完。"
                .to_string(),
        ),
        _ => AiError::Net(format!("AI 服务错误 {}: {}", status, body)),
    }
}

/// reqwest 发送失败分类（纯函数，可单测）：
/// `is_timeout` 由调用方填 `e.is_timeout()`（超时走 E_TIMEOUT，其余走 E_NET）。
/// `msg` 为 reqwest 原始错误文本（保留"AI 请求失败："前缀，toast 口径不变）。
pub(crate) fn classify_send_error(is_timeout: bool, msg: &str) -> AiError {
    if is_timeout {
        AiError::Timeout(format!("AI 请求超时：{}（可在设置里调大超时）", msg))
    } else {
        AiError::Net(format!("AI 请求失败: {}", msg))
    }
}

/// pin/校验失败分类（纯函数，可单测）：
/// SSRF 策略拒绝（含"本机/内网"或"SSRF"或"没有可用"）→ E_PERM；
/// 其余（域名解析失败等网络问题）→ E_NET。
/// validate_base_url 的错误直接按策略拒绝处理，调用方不再过此函数。
pub(crate) fn classify_pin_error(msg: &str) -> AiError {
    if msg.contains("本机/内网") || msg.contains("SSRF") || msg.contains("没有可用") {
        AiError::Permission(msg.to_string())
    } else {
        AiError::Net(msg.to_string())
    }
}

/// AGNES 非流式对话实现（provider 内部调用，含 SSRF 校验）
pub(crate) async fn agnes_chat_impl(
    base_url: &str,
    api_key: &str,
    req: &ProviderRequest,
) -> Result<ChatMessage, AiError> {
    let base_url = validate_base_url(base_url).map_err(AiError::Permission)?;
    // P0-6：解析一次并 pin 死公网地址，reqwest 建连不再二次解析（防 DNS rebinding）
    let (resolve_key, pinned) = resolve_pin_for_https(&base_url)
        .await
        .map_err(|e| classify_pin_error(&e))?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut builder = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(req.timeout_secs.unwrap_or(90)));
    if let Some(addrs) = &pinned {
      builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = builder
      .build()
      .map_err(|e| AiError::Internal(format!("HTTP 客户端创建失败: {}", e)))?;
    let req_body = ChatRequest {
        model: req.model.clone(),
        messages: req.messages.clone(),
        stream: req.stream,
        tools: req.tools.clone(),
    };

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req_body)
        .send()
        .await
        .map_err(|e| classify_send_error(e.is_timeout(), &e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            log::error!("[ai] 服务限流 429");
            return Err(classify_status(429, ""));
        }
        let body = resp.text().await.unwrap_or_default();
        log::error!("[ai] 服务错误 {}: {}", status, truncate_body(&body));
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

/// AGNES 流式对话实现（provider 内部调用，含 SSRF 校验 + SSE 解析）。
/// `cancel` 为 true 时中断（返回"已停止生成"）；`on_delta` 每块增量回调，
/// 返回 Err 表示前端通道已关闭，停止推送。
pub(crate) async fn agnes_chat_stream_impl<F>(
    base_url: &str,
    api_key: &str,
    req: &ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: F,
) -> Result<String, AiError>
where
    F: Fn(String) -> Result<(), ()>,
{
    let base_url =
        validate_base_url(base_url).map_err(AiError::Permission)?;
    // P0-6：解析一次并 pin 死公网地址，reqwest 建连不再二次解析（防 DNS rebinding）
    let (resolve_key, pinned) = resolve_pin_for_https(&base_url)
        .await
        .map_err(|e| classify_pin_error(&e))?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut builder = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(req.timeout_secs.unwrap_or(120)));
    if let Some(addrs) = &pinned {
      builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = builder
      .build()
      .map_err(|e| AiError::Internal(format!("HTTP 客户端创建失败: {}", e)))?;
    let req_body = ChatRequest {
        model: req.model.clone(),
        messages: req.messages.clone(),
        stream: req.stream,
        tools: req.tools.clone(),
    };

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req_body)
        .send()
        .await
        .map_err(|e| classify_send_error(e.is_timeout(), &e.to_string()))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            log::error!("[ai] 流式服务限流 429");
            return Err(classify_status(429, ""));
        }
        let body = resp.text().await.unwrap_or_default();
        let msg = classify_status(status.as_u16(), &body);
        log::error!("[ai] 流式服务错误 {}: {}", status, truncate_body(&body));
        return Err(msg);
    }

    // 读取 SSE 流，逐块解析 delta.content 推送
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();
    let mut full = String::new();
    let mut done = false;

    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            log::info!("[ai] 流式请求被用户取消");
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
                                // 前端已关闭通道时回调返回 Err → 停止推送
                                if on_delta(content.clone()).is_err() {
                                    log::info!("[ai] 前端已关闭流式通道，停止推送");
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

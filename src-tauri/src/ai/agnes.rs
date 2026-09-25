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

/// AGNES 非流式对话实现（provider 内部调用，含 SSRF 校验）
pub(super) async fn agnes_chat_impl(
    base_url: &str,
    api_key: &str,
    req: &ProviderRequest,
) -> Result<ChatMessage, String> {
    let base_url = validate_base_url(base_url)?;
    // P0-6：解析一次并 pin 死公网地址，reqwest 建连不再二次解析（防 DNS rebinding）
    let (resolve_key, pinned) = resolve_pin_for_https(&base_url).await?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut builder = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(90));
    if let Some(addrs) = &pinned {
      builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = builder
      .build()
      .map_err(|e| format!("HTTP 客户端创建失败: {}", e))?;
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
        .map_err(|e| format!("AI 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        // 429 = 服务端限流（免费额度/并发限制），给友好提示，别透传一坨错误体
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let msg = "AI 服务限流（429）：请求太频繁，请稍等片刻再试。若持续出现，可能是当日额度已用完。"
                .to_string();
            log::error!("[ai] 服务限流 429");
            return Err(msg);
        }
        let body = resp.text().await.unwrap_or_default();
        // 截断过长错误体（有些网关返回整页 HTML，原样透传会让前端显示一坨）
        // 注意按字符截断而非字节切片——中文等多字节字符的字节边界会 panic
        let body = if body.chars().count() > 200 {
            let truncated: String = body.chars().take(200).collect();
            format!("{}…(截断)", truncated)
        } else {
            body
        };
        log::error!("[ai] 服务错误 {}: {}", status, body);
        return Err(format!("AI 服务错误 {}: {}", status, body));
    }

    let data: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("AI 响应解析失败: {}", e))?;

    let reply = data
        .choices
        .into_iter()
        .next()
        .map(|c| c.message)
        .ok_or_else(|| "AI 无回复".to_string())?;
    Ok(reply)
}

/// AGNES 流式对话实现（provider 内部调用，含 SSRF 校验 + SSE 解析）。
/// `cancel` 为 true 时中断（返回"已停止生成"）；`on_delta` 每块增量回调，
/// 返回 Err 表示前端通道已关闭，停止推送。
pub(super) async fn agnes_chat_stream_impl<F>(
    base_url: &str,
    api_key: &str,
    req: &ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: F,
) -> Result<String, String>
where
    F: Fn(String) -> Result<(), ()>,
{
    let base_url = validate_base_url(base_url)?;
    // P0-6：解析一次并 pin 死公网地址，reqwest 建连不再二次解析（防 DNS rebinding）
    let (resolve_key, pinned) = resolve_pin_for_https(&base_url).await?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut builder = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(120));
    if let Some(addrs) = &pinned {
      builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = builder
      .build()
      .map_err(|e| format!("HTTP 客户端创建失败: {}", e))?;
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
        .map_err(|e| format!("AI 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let msg = "AI 服务限流（429）：请求太频繁，请稍等片刻再试。若持续出现，可能是当日额度已用完。"
                .to_string();
            log::error!("[ai] 流式服务限流 429");
            return Err(msg);
        }
        let body = resp.text().await.unwrap_or_default();
        let body = if body.chars().count() > 200 {
            let truncated: String = body.chars().take(200).collect();
            format!("{}…(截断)", truncated)
        } else {
            body
        };
        let msg = format!("AI 服务错误 {}: {}", status, body);
        log::error!("[ai] 流式服务错误 {}: {}", status, body);
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
            return Err("已停止生成".to_string());
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => return Err(format!("流读取失败: {}", e)),
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
        Err("AI 响应流意外中断".to_string())
    }
}

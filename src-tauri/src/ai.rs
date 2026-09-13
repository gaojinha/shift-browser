// AI 侧边栏：对接 AGNES AI（OpenAI 兼容 API）
// baseURL: https://apihub.agnes-ai.com/v1（从环境变量 AGNES_API_KEY 读 key）
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

/// AI 对话（AGNES API，OpenAI 兼容）
/// model：agnes-2.0-flash（默认，稳定）/ agnes-2.5-pro / agnes-2.5-flash（可能超时）
/// base_url：API 地址（默认 https://apihub.agnes-ai.com/v1）
/// api_key：API Key（留空/未传时用环境变量 AGNES_API_KEY）
/// messages：对话消息（role: system/user/assistant）
#[tauri::command]
pub async fn ai_chat(
    messages: Vec<ChatMessage>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<String, String> {
    let api_key = api_key
        .filter(|k| !k.trim().is_empty())
        // 掩码（前4后4 ****）不是真 key：get_all_settings 对 ai_api_key 返回掩码，
        // 前端若误把掩码回传，这里忽略并回退环境变量，避免用无效 key 请求
        .filter(|k| !k.contains("****"))
        .or_else(|| std::env::var("AGNES_API_KEY").ok())
        .ok_or_else(|| "未配置 API Key（在设置里填写，或设置 AGNES_API_KEY 环境变量）".to_string())?;
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());
    let base_url = base_url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "https://apihub.agnes-ai.com/v1".to_string());
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| format!("HTTP 客户端创建失败: {}", e))?;
    let req = ChatRequest {
        model: model.clone(),
        messages,
        stream: false,
    };

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("AI 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
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
        .map(|c| c.message.content)
        .ok_or_else(|| "AI 无回复".to_string())?;
    log::info!("[ai] 对话成功: model={} 回复 {} 字符", model, reply.chars().count());
    Ok(reply)
}
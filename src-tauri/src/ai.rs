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
/// model：agnes-2.5-flash（默认）/ agnes-2.5-pro / agnes-2.0-flash
/// messages：对话消息（role: system/user/assistant）
#[tauri::command]
pub async fn ai_chat(
    messages: Vec<ChatMessage>,
    model: Option<String>,
) -> Result<String, String> {
    let api_key = std::env::var("AGNES_API_KEY")
        .map_err(|_| "未配置 AGNES_API_KEY 环境变量".to_string())?;
    let model = model.unwrap_or_else(|| "agnes-2.5-flash".to_string());

    let client = reqwest::Client::new();
    let req = ChatRequest {
        model,
        messages,
        stream: false,
    };

    let resp = client
        .post("https://apihub.agnes-ai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", api_key))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("AI 请求失败: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("AI 服务错误 {}: {}", status, body));
    }

    let data: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("AI 响应解析失败: {}", e))?;

    data.choices
        .into_iter()
        .next()
        .map(|c| c.message.content)
        .ok_or_else(|| "AI 无回复".to_string())
}
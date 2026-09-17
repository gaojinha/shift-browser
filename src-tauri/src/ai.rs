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

/// 校验 AI API 地址（SSRF 防护）：
/// - 只允许 https://（拒绝 http 明文，避免 key 走明文）
/// - 拒绝 URL 内嵌用户名/密码（userinfo@）
/// - 拒绝本机/内网字面地址（localhost、127.x、10.x、172.16-31.x、192.168.x、169.254.x、0.0.0.0、::1 等）
///
/// 说明：域名解析成内网 IP 的 DNS rebinding 属于高级攻击面，
/// 这里做静态校验挡住绝大多数场景（恶意页面无法让 key 发往任意内网/本机）。
fn validate_base_url(base_url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(base_url).map_err(|_| "API 地址格式不合法".to_string())?;
    if parsed.scheme() != "https" {
        return Err("API 地址只允许 https://（防止 API Key 明文传输）".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("API 地址不能包含用户名/密码".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "API 地址缺少主机名".to_string())?;
    if is_private_host(host) {
        return Err(format!("API 地址不允许指向本机/内网: {}", host));
    }
    Ok(base_url.to_string())
}

fn is_private_host(host: &str) -> bool {
    // 字面 IP：按地址段判断（IpAddr 枚举上的 is_private 等方法是较新 Rust 才有，
    // 这里 match 到 Ipv4Addr/Ipv6Addr 用各自方法，兼容当前工具链）
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()
                    || v4.is_private()
                    || v4.is_link_local()
                    || v4.is_unspecified()
                    || v4.is_multicast()
                    || v4.is_broadcast()
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback() || v6.is_unspecified() || v6.is_multicast()
            }
        };
    }
    // 域名：localhost 及常见内网域名后缀
    let h = host.to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".lan")
        || h.ends_with(".home")
}

/// AI 对话（AGNES API，OpenAI 兼容）
/// model：agnes-2.0-flash（默认，稳定）/ agnes-2.5-pro / agnes-2.5-flash（可能超时）
/// base_url：API 地址（默认 https://apihub.agnes-ai.com/v1，仅允许 https 且非内网）
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
    let base_url = validate_base_url(&base_url)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_accepts_https_public() {
        assert!(validate_base_url("https://apihub.agnes-ai.com/v1").is_ok());
        assert!(validate_base_url("https://api.openai.com/v1").is_ok());
    }

    #[test]
    fn base_url_rejects_plain_http() {
        assert!(validate_base_url("http://apihub.agnes-ai.com/v1").is_err());
    }

    #[test]
    fn base_url_rejects_private_and_local() {
        // 本机/内网 IP
        assert!(validate_base_url("https://127.0.0.1:11434/v1").is_err());
        assert!(validate_base_url("https://localhost:11434/v1").is_err());
        assert!(validate_base_url("https://192.168.1.1/v1").is_err());
        assert!(validate_base_url("https://10.0.0.1/v1").is_err());
        assert!(validate_base_url("https://172.16.0.1/v1").is_err());
        assert!(validate_base_url("https://169.254.169.254/latest/meta-data").is_err());
        // 内网域名后缀
        assert!(validate_base_url("https://router.local/v1").is_err());
        assert!(validate_base_url("https://nas.home/v1").is_err());
        assert!(validate_base_url("https://myhost.lan/v1").is_err());
    }

    #[test]
    fn base_url_rejects_userinfo() {
        assert!(validate_base_url("https://user:pass@apihub.agnes-ai.com/v1").is_err());
    }
}
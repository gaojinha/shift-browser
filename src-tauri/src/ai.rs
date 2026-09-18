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
    // url crate 的 host_str() 对 IPv6 返回带方括号的 "[::1]"，先去掉再判断
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    // 1. 标准 IP 字面量（含 IPv4-mapped IPv6 的映射回判）
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    // 2. IPv4 混淆格式（十六进制/八进制/十进制整数、省略段）——解析后按 v4 判断
    if let Some(v4) = parse_obfuscated_ipv4(host) {
        return is_private_v4(v4);
    }
    // 3. 域名：localhost 及常见内网域名后缀
    let h = host.to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".lan")
        || h.ends_with(".home")
}

/// 判断 IP 是否为本机/内网（v4/v6 统一入口）
fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_v4(v4),
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped IPv6（::ffff:a.b.c.d）：必须按 v4 判断，
            // 否则 ::ffff:192.168.1.1 会被当成"看似公网的 v6"放行（SSRF 绕过）
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // 以下方法在 MSRV(1.77.2) 不可用，用手动位判断：
                // fc00::/7 唯一本地（内网）、fe80::/10 链路本地、fec0::/10 站点本地（已废弃但路由器仍可能实现）
                || v6.segments()[0] & 0xfe00 == 0xfc00
                || v6.segments()[0] & 0xffc0 == 0xfe80
                || v6.segments()[0] & 0xffc0 == 0xfec0
        }
    }
}

fn is_private_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
}

/// 解析 IPv4 混淆写法（浏览器/系统解析器支持，标准 IpAddr::parse 不认）：
/// - 单整数：十六进制 0x7f000001、八进制 017700000001、十进制 2130706433 → 127.0.0.1
/// - 省略段：127.1 → 127.0.0.1、127.0.1 → 127.0.0.1
/// - 段内进制：127.0x0.0x0.1、127.0.1（0 前缀八进制）
fn parse_obfuscated_ipv4(host: &str) -> Option<std::net::Ipv4Addr> {
    let h = host.trim();
    if h.is_empty() || h.contains(':') {
        return None;
    }
    if !h.contains('.') {
        // 单整数形式
        let n = parse_int_radix(h)?;
        if n <= u32::MAX as u64 {
            return Some(std::net::Ipv4Addr::from(n as u32));
        }
        return None;
    }
    // 多段形式：1~4 段，每段 0-255；省略段时最后一段可承载多个字节（127.1 → 127.0.0.1）
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        let v = parse_int_radix(p)?;
        if i == parts.len() - 1 && parts.len() < 4 {
            let remaining = 4 - parts.len();
            let mut v = v;
            for j in 0..=remaining {
                bytes[3 - j] = (v & 0xff) as u8;
                v >>= 8;
            }
            if v != 0 {
                return None;
            }
        } else {
            if v > 255 {
                return None;
            }
            bytes[i] = v as u8;
        }
    }
    Some(std::net::Ipv4Addr::from(bytes))
}

/// 解析带进制前缀的整数段：0x 十六进制、0 八进制、否则十进制
fn parse_int_radix(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// 实际解析域名并校验所有解析结果（防"域名解析到内网 IP"）
/// 注：DNS rebinding（解析后到连接前换 IP）属于高级攻击面，静态+解析校验
/// 能挡住常规场景（恶意域名静态解析到 127.0.0.1 等）。
async fn validate_resolved_host(host: &str) -> Result<(), String> {
    let addrs = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|e| format!("域名解析失败: {}", e))?;
    for addr in addrs {
        if is_private_ip(addr.ip()) {
            return Err(format!("API 地址解析到本机/内网，已拒绝: {}", addr.ip()));
        }
    }
    Ok(())
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
        .ok_or_else(|| {
            "未配置 API Key（在设置里填写，或设置 AGNES_API_KEY 环境变量）".to_string()
        })?;
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());
    let base_url = base_url
        .filter(|u| !u.trim().is_empty())
        .unwrap_or_else(|| "https://apihub.agnes-ai.com/v1".to_string());
    let base_url = validate_base_url(&base_url)?;
    // 实际解析域名，校验解析结果不指向本机/内网（P1-8）
    let host = url::Url::parse(&base_url)
        .map_err(|_| "API 地址格式不合法".to_string())?
        .host_str()
        .ok_or_else(|| "API 地址缺少主机名".to_string())?
        .to_string();
    if host.parse::<std::net::IpAddr>().is_err() {
        validate_resolved_host(&host).await?;
    }
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
    log::info!(
        "[ai] 对话成功: model={} 回复 {} 字符",
        model,
        reply.chars().count()
    );
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

    #[test]
    fn base_url_rejects_ipv6_private_and_mapped() {
        // IPv6 内网/链路本地/站点本地
        assert!(validate_base_url("https://[::1]:11434/v1").is_err());
        assert!(validate_base_url("https://[fc00::1]/v1").is_err());
        assert!(validate_base_url("https://[fd12:3456::1]/v1").is_err());
        assert!(validate_base_url("https://[fe80::1]/v1").is_err());
        assert!(validate_base_url("https://[fec0::1]/v1").is_err());
        // IPv4-mapped IPv6：必须按 v4 判内网（SSRF 绕过点）
        assert!(validate_base_url("https://[::ffff:127.0.0.1]/v1").is_err());
        assert!(validate_base_url("https://[::ffff:192.168.1.1]/v1").is_err());
        assert!(validate_base_url("https://[::ffff:10.0.0.1]/v1").is_err());
        // 公网 v6 放行
        assert!(validate_base_url("https://[2606:4700::1111]/v1").is_ok());
    }

    #[test]
    fn base_url_rejects_obfuscated_ipv4() {
        // 十六进制/八进制/十进制整数混淆 → 127.0.0.1
        assert!(validate_base_url("https://0x7f000001/v1").is_err());
        assert!(validate_base_url("https://017700000001/v1").is_err());
        assert!(validate_base_url("https://2130706433/v1").is_err());
        // 省略段混淆 → 127.0.0.1
        assert!(validate_base_url("https://127.1/v1").is_err());
        assert!(validate_base_url("https://127.0.1/v1").is_err());
        // 段内进制：127.0x0.0x0.1 → 127.0.0.1
        assert!(validate_base_url("https://127.0x0.0x0.1/v1").is_err());
        // 混淆的内网 192.168.1.1
        assert!(validate_base_url("https://0xc0a80101/v1").is_err());
        assert!(validate_base_url("https://3232235777/v1").is_err());
    }

    #[test]
    fn parse_obfuscated_ipv4_works() {
        use std::net::Ipv4Addr;
        assert_eq!(
            parse_obfuscated_ipv4("0x7f000001"),
            Some(Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(
            parse_obfuscated_ipv4("2130706433"),
            Some(Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(
            parse_obfuscated_ipv4("127.1"),
            Some(Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(
            parse_obfuscated_ipv4("127.0.1"),
            Some(Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(
            parse_obfuscated_ipv4("127.0x0.0x0.1"),
            Some(Ipv4Addr::new(127, 0, 0, 1))
        );
        assert_eq!(
            parse_obfuscated_ipv4("0xc0a80101"),
            Some(Ipv4Addr::new(192, 168, 1, 1))
        );
        // 非法/非混淆格式返回 None
        assert_eq!(parse_obfuscated_ipv4("example.com"), None);
        assert_eq!(parse_obfuscated_ipv4("999.1.1.1"), None);
        assert_eq!(parse_obfuscated_ipv4("256.0.0.1"), None);
    }
}

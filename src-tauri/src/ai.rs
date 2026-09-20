// AI 侧边栏：对接 AGNES AI（OpenAI 兼容 API）
// baseURL: https://apihub.agnes-ai.com/v1（从环境变量 AGNES_API_KEY 读 key）
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{ipc::Channel, Manager};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// 工具结果回传时填（role=tool 时必填，对应 assistant 的 tool_call.id）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// assistant 消息请求工具调用时填
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    /// 便捷构造（工具执行循环/测试用）
    #[allow(dead_code)]
    pub fn new(role: &str, content: impl Into<String>) -> Self {
        Self {
            role: role.to_string(),
            content: content.into(),
            tool_call_id: None,
            tool_calls: None,
        }
    }
}

/// OpenAI function calling 的工具调用请求（assistant 消息里）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolCallFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: String,
}

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

/// 流式响应的事件（经 tauri Channel 推给前端）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum StreamEvent {
    /// 增量内容（逐块推）
    Delta { content: String },
    /// 完整回复（流结束）
    Done { full: String },
    /// 出错（中断）
    Error { message: String },
}

/// SSE 流式响应解析结构（OpenAI 兼容）
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

/// 取消注册表：request_id -> 取消标志。前端点"停止"时置 true，流式循环检测后退出。
static CANCEL_FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn cancel_flags() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    CANCEL_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 取消指定 request_id 的流式请求（前端"停止生成"按钮调用）
#[tauri::command]
pub fn ai_cancel_stream(request_id: String) {
    if let Some(flag) = cancel_flags().lock().unwrap().get(&request_id) {
        flag.store(true, Ordering::Relaxed);
        log::info!("[ai] 已请求取消流式请求 {}", request_id);
    }
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

/// AI 对话（经 Provider 抽象路由，默认 AGNES 云端）
/// provider：provider id（"agnes" 默认 / "local" 预留）
/// model / base_url / api_key：透传给 provider（缺省用 AGNES 默认值）
#[tauri::command]
pub async fn ai_chat(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    provider: Option<String>,
    messages: Vec<ChatMessage>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<String, String> {
    let provider_id = provider.unwrap_or_default();
    let provider = crate::engine::ai_provider::resolve_provider(&provider_id, base_url, api_key)?;
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());

    // 工具层（阶段 1.3/1.4）：MCP 工具（有启用时）+ Agent 浏览器动作（总是带上）。
    // Agent 动作执行前会弹确认框等用户批准，安全铁律。
    let mut tool_defs: Vec<serde_json::Value> = Vec::new();
    if let Ok(t) = crate::engine::mcp::list_tools(&db.pool).await {
        if !t.is_empty() {
            tool_defs.extend(crate::engine::mcp::to_openai_tools(&t));
        }
    }
    tool_defs.push(crate::engine::agent::browser_action_tool());
    let tools = Some(tool_defs);
    log::info!("[ai] 本次对话携带 {} 个工具定义（function calling）", tools.as_ref().map(|t| t.len()).unwrap_or(0));

    // 多轮循环：模型请求工具 → 执行 → 结果回传 → 再请求（上限 6 轮防死循环）
    let mut current = messages;
    for round in 0..6 {
        let req = crate::engine::ai_provider::ProviderRequest {
            model: model.clone(),
            messages: current.clone(),
            stream: false,
            tools: tools.clone(),
        };
        let reply = provider.chat(&req).await?;

        // 模型请求调用工具
        if let Some(calls) = reply.tool_calls.clone() {
            if calls.is_empty() {
                return Ok(reply.content);
            }
            log::info!("[ai] 第 {} 轮模型请求 {} 个工具调用", round + 1, calls.len());
            // 追加 assistant 消息（含 tool_calls）
            current.push(reply);
            // 逐个执行工具并追加 tool 结果消息
            for call in &calls {
                let args: serde_json::Value =
                    serde_json::from_str(&call.function.arguments).unwrap_or(serde_json::Value::Null);
                let content = if call.function.name == "browser_action" {
                    // Agent 动作：每步人工确认（安全铁律）
                    match serde_json::from_value::<crate::engine::agent::AgentActionParams>(args.clone()) {
                        Ok(params) => {
                            // 请求确认（前端弹卡片，60s 超时）
                            match crate::agent_cmd::request_agent_action(&app, &params).await {
                                Ok(true) => {
                                    // 用户允许 → 执行（仅当前活动标签）
                                    let window = app
                                        .get_webview_window("main")
                                        .ok_or_else(|| "主窗口不存在".to_string())?;
                                    let state = app
                                        .try_state::<crate::tabs::TabManager>()
                                        .ok_or_else(|| "标签管理器未初始化".to_string())?;
                                    match crate::agent_cmd::execute_agent_action(&app, &window, &state, &params).await {
                                        Ok(text) => format!("动作已执行：{}", text),
                                        Err(e) => format!("动作执行失败: {}", e),
                                    }
                                }
                                Ok(false) => "用户拒绝了该动作".to_string(),
                                Err(e) => format!("动作确认失败: {}", e),
                            }
                        }
                        Err(e) => format!("动作参数解析失败: {}", e),
                    }
                } else if call.function.name == "browser_action_plan" {
                    // 多步任务编排提示词：整体确认 + 按序执行
                    match serde_json::from_value::<crate::engine::agent::MultiStepPlan>(args.clone()) {
                        Ok(plan) => {
                            match crate::agent_cmd::request_multi_step_action(&app, &plan).await {
                                Ok(true) => {
                                    let window = app
                                        .get_webview_window("main")
                                        .ok_or_else(|| "主窗口不存在".to_string())?;
                                    let state = app
                                        .try_state::<crate::tabs::TabManager>()
                                        .ok_or_else(|| "标签管理器未初始化".to_string())?;
                                    match crate::agent_cmd::execute_multi_step_action(&app, &window, &state, &plan).await {
                                        Ok(text) => format!("多步计划已执行：\n{}", text),
                                        Err(e) => format!("多步计划执行失败: {}", e),
                                    }
                                }
                                Ok(false) => "用户拒绝了该多步计划".to_string(),
                                Err(e) => format!("多步计划确认失败: {}", e),
                            }
                        }
                        Err(e) => format!("多步计划解析失败: {}", e),
                    }
                } else {
                    // MCP 工具
                    let result = crate::engine::mcp::call_tool(&db.pool, &call.function.name, args).await;
                    match result {
                        Ok(text) => text,
                        Err(e) => format!("工具执行失败: {}", e),
                    }
                };
                current.push(ChatMessage {
                    role: "tool".to_string(),
                    content,
                    tool_call_id: Some(call.id.clone()),
                    tool_calls: None,
                });
            }
            continue;
        }

        // 无工具调用 → 最终回复
        log::info!(
            "[ai] 对话成功: provider={} 回复 {} 字符",
            provider_id,
            reply.content.chars().count()
        );
        return Ok(reply.content);
    }
    Err("AI 工具调用超过 6 轮上限，已终止".to_string())
}

/// AGNES 非流式对话实现（provider 内部调用，含 SSRF 校验）
pub async fn agnes_chat_impl(
    base_url: &str,
    api_key: &str,
    req: &crate::engine::ai_provider::ProviderRequest,
) -> Result<ChatMessage, String> {
    let base_url = validate_base_url(base_url)?;
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

/// AI 对话（流式，SSE 逐块推送）——经 Provider 抽象路由（默认 AGNES）
/// 前端点"停止"时调 ai_cancel_stream(request_id)，本命令检测到取消标志后提前返回。
#[tauri::command]
pub async fn ai_chat_stream(
    request_id: String,
    provider: Option<String>,
    messages: Vec<ChatMessage>,
    on_event: Channel<StreamEvent>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<(), String> {
    let provider_id = provider.unwrap_or_default();
    let provider =
        match crate::engine::ai_provider::resolve_provider(&provider_id, base_url, api_key) {
            Ok(p) => p,
            Err(e) => {
                on_event
                    .send(StreamEvent::Error { message: e.clone() })
                    .ok();
                return Err(e);
            }
        };
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());
    let req = crate::engine::ai_provider::ProviderRequest {
        model,
        messages,
        stream: true,
        tools: None,
    };

    // 注册取消标志（请求结束无论成败都移除）
    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags()
        .lock()
        .unwrap()
        .insert(request_id.clone(), cancel.clone());

    let result = provider
        .chat_stream(
            &req,
            cancel.clone(),
            Box::new({
                let on_event = on_event.clone();
                move |delta| {
                    on_event
                        .send(StreamEvent::Delta { content: delta })
                        .map_err(|_| ())
                }
            }),
        )
        .await;

    cancel_flags().lock().unwrap().remove(&request_id);

    match result {
        Ok(full) => {
            log::info!(
                "[ai] 流式对话成功: provider={} 回复 {} 字符",
                provider_id,
                full.chars().count()
            );
            on_event.send(StreamEvent::Done { full }).ok();
            Ok(())
        }
        Err(e) => {
            // 用户主动停止不算错误（impl 里 cancel 时返回"已停止生成"）
            if e != "已停止生成" {
                on_event
                    .send(StreamEvent::Error { message: e.clone() })
                    .ok();
            }
            Err(e)
        }
    }
}

/// AGNES 流式对话实现（provider 内部调用，含 SSRF 校验 + SSE 解析）。
/// `cancel` 为 true 时中断（返回"已停止生成"）；`on_delta` 每块增量回调，
/// 返回 Err 表示前端通道已关闭，停止推送。
pub async fn agnes_chat_stream_impl<F>(
    base_url: &str,
    api_key: &str,
    req: &crate::engine::ai_provider::ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: F,
) -> Result<String, String>
where
    F: Fn(String) -> Result<(), ()>,
{
    let base_url = validate_base_url(base_url)?;
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
        .timeout(std::time::Duration::from_secs(120))
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

/// 可用 AI Provider 列表（设置页展示）
#[tauri::command]
pub fn ai_list_providers() -> Vec<crate::engine::ai_provider::ProviderInfo> {
    crate::engine::ai_provider::list_providers()
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

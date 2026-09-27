// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// AI 侧边栏：对接 AGNES AI（OpenAI 兼容 API）
// baseURL: https://apihub.agnes-ai.com/v1（从环境变量 AGNES_API_KEY 读 key）
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{ipc::Channel, Manager};

// R2-1 拆分（2026-09-24）：SSRF 防护独立成子模块
#[path = "ai/ssrf.rs"]
pub(crate) mod ssrf;
pub(crate) use ssrf::{check_resolved_ips, is_private_host, is_private_ip, resolve_pin_for_https, validate_base_url};
// R1 拆分（2026-09-25）：AGNES 非流式/SSE 两条 HTTP 管线实现
#[path = "ai/agnes.rs"]
mod agnes;
use agnes::{agnes_chat_impl, agnes_chat_stream_impl};

// P0-2 补完（2026-09-22）：ChatMessage 下沉到 engine::types 以打破
// ai ↔ ai_provider 循环依赖。此处 re-export 保持既有引用路径不变。
pub use crate::engine::types::ChatMessage;

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

// P0-2 补完（2026-09-22）：ToolCall/ToolCallFunction 下沉到 engine::types，
// 与 ChatMessage 一同断开 engine→ai 反向依赖。此处 re-export 保持路径不变。

// ChatRequest/ChatResponse/ChatChoice 等 AGNES 线协议结构随实现住在 agnes.rs

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

// SSE 流式解析结构（ChatStreamResponse/ChatStreamChoice/ChatStreamDelta）同住 agnes.rs

/// 取消注册表：request_id -> 取消标志。前端点"停止"时置 true，流式循环检测后退出。
static CANCEL_FLAGS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();

fn cancel_flags() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    CANCEL_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 运行时解析 provider。
///
/// `opencode` 与 agnes/local 不同：它的凭据要异步发现（设置项 → /proc 扫
/// serve 进程），模型也来自独立设置项，所以同步纯函数 resolve_provider
/// 装不下，在这里特判分支；其余 id 仍走 engine 的同步解析。
async fn resolve_runtime_provider(
    db: &tauri::State<'_, crate::db::Db>,
    provider_id: &str,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Box<dyn crate::engine::ai_provider::AiProvider>, crate::error::AppError> {
    if provider_id == "opencode" {
        let model_ref = crate::db::settings_cache::get(&db.pool, "opencode_model")
            .await
            .unwrap_or_default();
        let creds = crate::webqa::resolve_credentials_pub(db).await?;
        return Ok(Box::new(crate::opencode_chat::OpencodeProvider::new(
            creds,
            &model_ref,
        )));
    }
    crate::engine::ai_provider::resolve_provider(provider_id, base_url, api_key)
}

/// 取消指定 request_id 的流式请求（前端"停止生成"按钮调用）
/// 毒化容错：持有锁线程 panic 后锁毒化也照常用，不崩进程
#[tauri::command]
pub fn ai_cancel_stream(request_id: String) {
    if let Some(flag) = cancel_flags()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&request_id)
    {
        flag.store(true, Ordering::Relaxed);
        log::info!("[ai] 已请求取消流式请求 {}", request_id);
    }
}


/// AI 对话（经 Provider 抽象路由，默认 AGNES 云端）
/// provider：provider id（"agnes" 默认 / "local" 预留）
/// model / base_url / api_key：透传给 provider（缺省用 AGNES 默认值）
/// R1 错误类型化试点：命令返回 CommandError（{code,message}），
/// 分类规则：provider 解析失败按其 AppError（缺 key=E_AUTH/未知 provider=E_VALIDATION）；
/// 上游 HTTP/网络与模型异常行为 = E_NET；主窗口/状态缺失等内部异常 = E_INTERNAL。
#[tauri::command]
pub async fn ai_chat(
    app: tauri::AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    provider: Option<String>,
    messages: Vec<ChatMessage>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> crate::error::CommandResult<String> {
    let provider_id = provider.unwrap_or_default();
    let provider =
        resolve_runtime_provider(&db, &provider_id, base_url, api_key).await?;
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());

    // 工具层（阶段 1.3/1.4）：MCP 工具（有启用时）+ Agent 浏览器动作（总是带上）。
    // Agent 动作执行前会弹确认框等用户批准，安全铁律。
    let mut tool_defs: Vec<serde_json::Value> = Vec::new();
    if let Ok(t) = crate::engine::mcp::list_tools(&db.pool).await {
        if !t.is_empty() {
            tool_defs.extend(crate::engine::mcp::to_openai_tools(&t));
        }
    }
    tool_defs.extend(crate::engine::agent::agent_tool_defs());
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
        let reply = provider
            .chat(&req)
            .await?;

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
    Err(crate::error::AppError::net("AI 工具调用超过 6 轮上限，已终止").into())
}

/// AI 对话（流式，SSE 逐块推送）——经 Provider 抽象路由（默认 AGNES）
/// 前端点"停止"时调 ai_cancel_stream(request_id)，本命令检测到取消标志后提前返回。
#[tauri::command]
// db 是新增的第 8 个参数（opencode 凭据发现要用）；Tauri 命令按名注入，
// 不宜打包结构体，沿用命令式 allow。
#[allow(clippy::too_many_arguments, clippy::unwrap_used, clippy::expect_used)]
pub async fn ai_chat_stream(
    db: tauri::State<'_, crate::db::Db>,
    request_id: String,
    provider: Option<String>,
    messages: Vec<ChatMessage>,
    on_event: Channel<StreamEvent>,
    model: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
) -> crate::error::CommandResult<()> {
    let provider_id = provider.unwrap_or_default();
    let provider =
        match resolve_runtime_provider(&db, &provider_id, base_url, api_key).await {
            Ok(p) => p,
            Err(e) => {
                // StreamEvent::Error.message 保持纯消息字符串（渠道内契约不变）
                on_event
                    .send(StreamEvent::Error {
                        message: e.to_string(),
                    })
                    .ok();
                return Err(e.into());
            }
        };
    let model = model.unwrap_or_else(|| "agnes-2.0-flash".to_string());
    let req = crate::engine::ai_provider::ProviderRequest {
        model,
        messages,
        stream: true,
        tools: None,
    };

    // 注册取消标志（请求结束无论成败都移除；锁毒化时照常用，不崩进程）
    let cancel = Arc::new(AtomicBool::new(false));
    cancel_flags()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
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

    cancel_flags()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&request_id);

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
            if e.message != "已停止生成" {
                on_event
                    .send(StreamEvent::Error { message: e.message.clone() })
                    .ok();
            }
            // 流式管线失败（SSE 读取/HTTP/上游中断）错误码由 provider 实现给出
            // （agnes 注册闭包统一 E_NET）；停止生成同样向上抛错，调用方按消息识别
            Err(e)
        }
    }
}

/// 可用 AI Provider 列表（设置页展示）
#[tauri::command]
pub fn ai_list_providers() -> Vec<crate::engine::ai_provider::ProviderInfo> {
    crate::engine::ai_provider::list_providers()
}

/// P0-2 补完（2026-09-22）：向 engine 层注册聊天实现，打破反向依赖。
/// 由 lib.rs 启动时调用一次。
pub fn register_provider_impls() {
    crate::engine::ai_provider::register_chat_impl(
        |base_url, api_key, req| {
            Box::pin(async move {
                let r = crate::engine::ai_provider::ProviderRequest { ..req };
                // agnes 实现仍返回裸 String：聊天管线失败统一归 E_NET（与旧调用点行为一致），
                // 后续细化 agnes 内部（鉴权/SSRF/序列化）时在实现侧分类。
                agnes_chat_impl(&base_url, &api_key, &r)
                    .await
                    .map_err(|e| crate::error::AppError::net(e).into())
            })
        },
        |base_url, api_key, req, cancel, on_delta| {
            Box::pin(async move {
                let r = crate::engine::ai_provider::ProviderRequest { ..req };
                agnes_chat_stream_impl(&base_url, &api_key, &r, cancel, on_delta)
                    .await
                    .map_err(|e| crate::error::AppError::net(e).into())
            })
        },
    );
}

// SSRF/pin 相关 12 个安全测试随被测面外置（避免门面超长）
#[cfg(test)]
mod tests;

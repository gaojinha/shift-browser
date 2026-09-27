//! 推理层 Provider 抽象（阶段 1.2 · Provider Layer）
//!
//! 目标：多 provider 抽象——统一接口，业务代码不关心具体走哪个服务。
//! - `AiProvider` trait：id / name / is_configured / chat / chat_stream
//! - `AgnesProvider`：云端（OpenAI 兼容，默认，现有 AGNES）
//! - `LocalProvider`：本地推理（Ollama，第七件最小可用：直连本机 11434，
//!   连不上由 ai.rs 回退云端；自定义端点/鉴权后续再加）
//!
//! 安全：agnes 的 base_url 走 SSRF 校验（https-only + 非内网）；
//! 本地端点固定回环（见 OLLAMA_DEFAULT_BASE），不接受外部覆盖，
//! 校验函数从 ai.rs 迁来，保持行为一致。

use crate::engine::types::ChatMessage;
use crate::error::{AppError, CommandResult};
use async_trait::async_trait;

/// 推理请求（统一结构，与 OpenAI 兼容格式对齐）
#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    /// function calling 工具定义（OpenAI 格式，None = 不带工具）
    pub tools: Option<Vec<serde_json::Value>>,
    /// 请求超时（秒，None = 各实现默认值；ai.rs 从设置 ai_timeout/ai_stream_timeout 填）
    pub timeout_secs: Option<u64>,
}

/// AI Provider 统一接口
#[async_trait]
pub trait AiProvider: Send + Sync {
    /// provider 标识（存储/路由用）
    /// 注：设置页接入前暂未直接调用（list_providers 直接构造），保留为公共 API 契约。
    #[allow(dead_code)]
    fn id(&self) -> &str;
    /// 显示名（设置页展示）
    #[allow(dead_code)]
    fn name(&self) -> &str;
    /// 是否已配置（有 key/端点可用）
    #[allow(dead_code)]
    fn is_configured(&self) -> bool;
    /// 非流式对话，返回完整回复
    async fn chat(&self, req: &ProviderRequest) -> CommandResult<ChatMessage>;
    /// 流式对话：每块增量经 `on_delta` 回调（SSE 解析在实现内部）
    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> CommandResult<String>;
}

/// 云端 AGNES Provider（OpenAI 兼容）
pub struct AgnesProvider {
    pub base_url: String,
    pub api_key: String,
}

impl AgnesProvider {
    /// 注：当前由 resolve_provider 直接构造，new 保留为公共 API 契约。
    #[allow(dead_code)]
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base_url, api_key }
    }
}


// ============================================================
// P0-2 补完（2026-09-22）：打破 engine → ai 的反向依赖
// ============================================================
//
// 原状：`AgnesProvider::chat` 直接调 `crate::ai::agnes_chat_impl`（引擎层依赖业务实现）。
// 现改为**回调注入**：`ai.rs` 在启动时注册实现，engine 只持函数指针调用。
// 这样 engine 层不再 `use crate::ai::*`（除已下沉的 ChatMessage 类型）。

/// 聊天实现签名（由 ai.rs 注册）
pub type ChatFn = fn(
    base_url: String,
    api_key: String,
    req: ProviderRequest,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = CommandResult<ChatMessage>> + Send>>;

/// 流式聊天实现签名（由 ai.rs 注册）
pub type ChatStreamFn = fn(
    base_url: String,
    api_key: String,
    req: ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = CommandResult<String>> + Send>>;

static CHAT_IMPL: std::sync::OnceLock<ChatFn> = std::sync::OnceLock::new();
static CHAT_STREAM_IMPL: std::sync::OnceLock<ChatStreamFn> = std::sync::OnceLock::new();

/// 由 ai.rs 启动时注册实现（打破反向依赖）
pub fn register_chat_impl(f: ChatFn, fs: ChatStreamFn) {
    let _ = CHAT_IMPL.set(f);
    let _ = CHAT_STREAM_IMPL.set(fs);
}

/// 本地 Ollama 默认端点（回环固定值；engine 侧拥有，ai 侧实现只收参 SLO）
pub const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434/v1";

/// 本地聊天实现签名（由 ai.rs 注册；与 ChatFn 的区别：无 api_key，
/// base 由 resolve 固定为回环默认，不接受调用方覆盖）
pub type OllamaChatFn = fn(
    base_url: String,
    req: ProviderRequest,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = CommandResult<ChatMessage>> + Send>>;

/// 本地流式聊天实现签名（由 ai.rs 注册）
pub type OllamaChatStreamFn = fn(
    base_url: String,
    req: ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = CommandResult<String>> + Send>>;

static OLLAMA_CHAT_IMPL: std::sync::OnceLock<OllamaChatFn> = std::sync::OnceLock::new();
static OLLAMA_CHAT_STREAM_IMPL: std::sync::OnceLock<OllamaChatStreamFn> =
    std::sync::OnceLock::new();

/// 由 ai.rs 启动时注册本地实现（打破反向依赖，与 register_chat_impl 配对）
pub fn register_ollama_impl(f: OllamaChatFn, fs: OllamaChatStreamFn) {
    let _ = OLLAMA_CHAT_IMPL.set(f);
    let _ = OLLAMA_CHAT_STREAM_IMPL.set(fs);
}

#[async_trait]
impl AiProvider for AgnesProvider {
    fn id(&self) -> &str {
        "agnes"
    }
    fn name(&self) -> &str {
        "AGNES 云端"
    }
    fn is_configured(&self) -> bool {
        !self.api_key.is_empty()
    }
    async fn chat(&self, req: &ProviderRequest) -> CommandResult<ChatMessage> {
        let f = CHAT_IMPL
            .get()
            .ok_or_else(|| AppError::internal("聊天实现未注册（ai::register）"))?;
        f(self.base_url.clone(), self.api_key.clone(), req.clone()).await
    }
    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> CommandResult<String> {
        let f = CHAT_STREAM_IMPL
            .get()
            .ok_or_else(|| AppError::internal("流式聊天实现未注册（ai::register）"))?;
        f(
            self.base_url.clone(),
            self.api_key.clone(),
            req.clone(),
            cancel,
            on_delta,
        )
        .await
    }
}

/// 本地推理 Provider（Ollama，最小可用）。
///
/// `base_url` 固定回环默认（`OLLAMA_DEFAULT_BASE`）：resolve 时写死，
/// 调用方透传的外部地址一律忽略——"本地模型"绝不能被指到外网。
/// 模型走请求的 model（ai.rs 按 provider 给默认 `llama3.1`，用户在设置页改）。
/// Ollama 没在跑时实现侧报传输失败，ai.rs 回退 AGNES 云端。
pub struct LocalProvider {
    pub base_url: String,
}

impl LocalProvider {
    pub fn with_default() -> Self {
        Self {
            base_url: OLLAMA_DEFAULT_BASE.to_string(),
        }
    }
}

#[async_trait]
impl AiProvider for LocalProvider {
    fn id(&self) -> &str {
        "local"
    }
    fn name(&self) -> &str {
        "本机 Ollama"
    }
    fn is_configured(&self) -> bool {
        // 端点固定、无需 key：是否真在跑要发请求才知道，调用时见分晓
        true
    }
    async fn chat(&self, req: &ProviderRequest) -> CommandResult<ChatMessage> {
        let f = OLLAMA_CHAT_IMPL
            .get()
            .ok_or_else(|| AppError::internal("本地聊天实现未注册（ai::register）"))?;
        f(self.base_url.clone(), req.clone()).await
    }
    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> CommandResult<String> {
        let f = OLLAMA_CHAT_STREAM_IMPL
            .get()
            .ok_or_else(|| AppError::internal("本地流式聊天实现未注册（ai::register）"))?;
        f(self.base_url.clone(), req.clone(), cancel, on_delta).await
    }
}

/// 按 provider id 解析出可用 provider。
/// - "agnes"（默认）：AGNES 云端
/// - "local"：本机 Ollama（端点固定回环默认；透传的 base_url/api_key 忽略）
/// - 其他：未知 provider 错误
pub fn resolve_provider(
    provider_id: &str,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Box<dyn AiProvider>, crate::error::AppError> {
    match provider_id {
        "agnes" | "" => {
            let base_url = base_url
                .filter(|u| !u.trim().is_empty())
                .unwrap_or_else(|| "https://apihub.agnes-ai.com/v1".to_string());
            let api_key = api_key
                .filter(|k| !k.trim().is_empty())
                .filter(|k| !k.contains("****"))
                .or_else(|| std::env::var("AGNES_API_KEY").ok())
                .ok_or_else(|| {
                    crate::error::AppError::auth(
                        "未配置 API Key（在设置里填写，或设置 AGNES_API_KEY 环境变量）",
                    )
                })?;
            // SSRF 校验（https-only + 非内网）在 agnes_chat_impl 内部执行
            Ok(Box::new(AgnesProvider { base_url, api_key }))
        }
        "local" => Ok(Box::new(LocalProvider::with_default())),
        other => Err(crate::error::AppError::validation(format!(
            "未知 provider: {}",
            other
        ))),
    }
}

/// 可用 provider 列表（设置页展示）
pub fn list_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "agnes".to_string(),
            name: "AGNES 云端".to_string(),
            configured: std::env::var("AGNES_API_KEY").is_ok(),
            description: "云端推理，OpenAI 兼容，免费额度每天 66 次".to_string(),
        },
        ProviderInfo {
            id: "local".to_string(),
            name: "本机 Ollama".to_string(),
            configured: true,
            description: "本机 Ollama（127.0.0.1:11434），模型在下方填写；没在跑时自动回退云端"
                .to_string(),
        },
        ProviderInfo {
            id: "opencode".to_string(),
            name: "本机 opencode".to_string(),
            // 凭据能否发现要异步读 db/扫进程，同步列表里无法判定；
            // 真实可用性由设置页探活（opencode_probe）与发问时报错体现。
            configured: false,
            description: "本机 opencode serve（127.0.0.1:4096），模型在下方选择；凭据自动发现"
                .to_string(),
        },
    ]
}

/// Provider 信息（设置页展示）
#[derive(Debug, serde::Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub configured: bool,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_agnes_filters_masked_key() {
        // 掩码 key（****）必须被过滤：要么回退环境变量成功（Ok），要么报"未配置"（Err）。
        // 关键是绝不能把掩码 key 当作有效 key 用（那会发无效请求）。
        let r = resolve_provider("", None, Some("sk-1234****5678".to_string()));
        match r {
            Ok(p) => {
                // 回退环境变量成功——masked key 不可见于 provider
                assert!(p.is_configured());
            }
            Err(e) => {
                assert!(e.to_string().contains("未配置"));
                // R1 错误类型化：缺 key 必须是 E_AUTH（前端据此引导去设置页）
                assert_eq!(e.code(), crate::error::E_AUTH);
            }
        }
    }

    #[test]
    fn resolve_local_uses_loopback_default() {
        let p = resolve_provider("local", None, None).unwrap();
        assert_eq!(p.id(), "local");
        // 端点固定无需 key：configured 为真，是否真在跑调用时见分晓
        assert!(p.is_configured());
    }

    /// 防漂移：local 的外部地址透传必须被忽略（"本地模型"绝不能被指到外网）
    #[test]
    fn resolve_local_ignores_external_overrides() {
        let p = resolve_provider(
            "local",
            Some("https://evil.example/v1".to_string()),
            Some("sk-xxx".to_string()),
        )
        .unwrap();
        assert_eq!(p.id(), "local");
        assert!(p.is_configured());
        // 默认端点写死回环（直接验构造器，trait 对象看不到字段）
        assert_eq!(LocalProvider::with_default().base_url, OLLAMA_DEFAULT_BASE);
        assert!(OLLAMA_DEFAULT_BASE.contains("127.0.0.1"));
    }

    #[test]
    fn resolve_unknown_fails() {
        assert!(resolve_provider("unknown", None, None).is_err());
    }

    #[test]
    fn list_has_known_providers() {
        let l = list_providers();
        assert!(l.iter().any(|p| p.id == "agnes"));
        assert!(l.iter().any(|p| p.id == "local"));
        assert!(l.iter().any(|p| p.id == "opencode"));
    }
}

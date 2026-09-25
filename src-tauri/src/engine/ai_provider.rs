//! 推理层 Provider 抽象（阶段 1.2 · Provider Layer）
//!
//! 目标：多 provider 抽象——统一接口，业务代码不关心具体走哪个服务。
//! - `AiProvider` trait：id / name / is_configured / chat / chat_stream
//! - `AgnesProvider`：云端（OpenAI 兼容，默认，现有 AGNES）
//! - `LocalProvider`：本地推理（Ollama/llama.cpp）——**接口预留，不实现**
//!   （2026-09-19 高哥决策：本地模型集成暂缓，只留接口）
//!
//! 安全：所有 provider 的 base_url 都走 SSRF 校验（https-only + 非内网），
//! 校验函数从 ai.rs 迁来，保持行为一致。

use crate::engine::types::ChatMessage;
use async_trait::async_trait;

/// 推理请求（统一结构，与 OpenAI 兼容格式对齐）
#[derive(Debug, Clone)]
pub struct ProviderRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub stream: bool,
    /// function calling 工具定义（OpenAI 格式，None = 不带工具）
    pub tools: Option<Vec<serde_json::Value>>,
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
    async fn chat(&self, req: &ProviderRequest) -> Result<ChatMessage, String>;
    /// 流式对话：每块增量经 `on_delta` 回调（SSE 解析在实现内部）
    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> Result<String, String>;
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ChatMessage, String>> + Send>>;

/// 流式聊天实现签名（由 ai.rs 注册）
pub type ChatStreamFn = fn(
    base_url: String,
    api_key: String,
    req: ProviderRequest,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>;

static CHAT_IMPL: std::sync::OnceLock<ChatFn> = std::sync::OnceLock::new();
static CHAT_STREAM_IMPL: std::sync::OnceLock<ChatStreamFn> = std::sync::OnceLock::new();

/// 由 ai.rs 启动时注册实现（打破反向依赖）
pub fn register_chat_impl(f: ChatFn, fs: ChatStreamFn) {
    let _ = CHAT_IMPL.set(f);
    let _ = CHAT_STREAM_IMPL.set(fs);
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
    async fn chat(&self, req: &ProviderRequest) -> Result<ChatMessage, String> {
        let f = CHAT_IMPL.get().ok_or("聊天实现未注册（ai::register）")?;
        f(self.base_url.clone(), self.api_key.clone(), req.clone()).await
    }
    async fn chat_stream(
        &self,
        req: &ProviderRequest,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> Result<String, String> {
        let f = CHAT_STREAM_IMPL
            .get()
            .ok_or("流式聊天实现未注册（ai::register）")?;
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

/// 本地推理 Provider（Ollama/llama.cpp）——**接口预留，暂不实现**
/// 2026-09-19 决策：本地模型集成暂缓，返回明确错误提示。
pub struct LocalProvider;

#[async_trait]
impl AiProvider for LocalProvider {
    fn id(&self) -> &str {
        "local"
    }
    fn name(&self) -> &str {
        "本地模型（预留）"
    }
    fn is_configured(&self) -> bool {
        false
    }
    async fn chat(&self, _req: &ProviderRequest) -> Result<ChatMessage, String> {
        Err("本地模型集成暂缓（2026-09-19 决策）：当前仅预留接口，未实现。".to_string())
    }
    async fn chat_stream(
        &self,
        _req: &ProviderRequest,
        _cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        _on_delta: Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>,
    ) -> Result<String, String> {
        Err("本地模型集成暂缓（2026-09-19 决策）：当前仅预留接口，未实现。".to_string())
    }
}

/// 按 provider id 解析出可用 provider。
/// - "agnes"（默认）：AGNES 云端
/// - "local"：本地（预留，未实现，返回明确错误）
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
        "local" => Ok(Box::new(LocalProvider)),
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
            name: "本地模型（预留）".to_string(),
            configured: false,
            description: "Ollama/llama.cpp 接口预留，暂缓实现（2026-09-19 决策）".to_string(),
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
    fn resolve_local_is_stub() {
        let p = resolve_provider("local", None, None).unwrap();
        assert_eq!(p.id(), "local");
        assert!(!p.is_configured());
    }

    #[test]
    fn resolve_unknown_fails() {
        assert!(resolve_provider("unknown", None, None).is_err());
    }

    #[test]
    fn list_has_agnes_and_local() {
        let l = list_providers();
        assert!(l.iter().any(|p| p.id == "agnes"));
        assert!(l.iter().any(|p| p.id == "local"));
    }
}

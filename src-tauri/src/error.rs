//! 统一应用错误类型（R1 · 错误类型化，2026-09-24）
//!
//! 历史上各边界（Tauri 命令 / 本地 API / 模型网关）失败时只返回裸字符串，
//! 调用方无法按错误类别分流（重试 / 重新鉴权 / 修正参数）。本模块定义稳定错误码：
//!
//! | code          | 含义                           | 调用方典型动作           |
//! |---------------|-------------------------------|--------------------------|
//! | `E_DB`        | 持久层失败（SQLite 等）         | 提示 + 稍后重试          |
//! | `E_PERM`      | 策略拒绝（只读模式/功能禁用/SSRF）| 不改参数重试无意义       |
//! | `E_TIMEOUT`   | 超时                           | 可重试                   |
//! | `E_AUTH`      | 鉴权/凭据缺失或无效             | 重新取 token/key         |
//! | `E_RATELIMIT` | 服务端限流（429）               | 等待后重试，不要立即重试 |
//! | `E_NET`       | 网络/外部服务（含 MCP）失败     | 可重试                   |
//! | `E_VALIDATION`| 请求参数非法/资源不存在/未知命令| 修正参数，不要原样重试   |
//! | `E_INTERNAL`  | 服务端内部状态异常（兜底）       | 记日志，一般为缺陷       |
//!
//! 兼容性（R1 第一批只落地 api/ 边界）：
//! - 本地 API 信封：`{"ok":false,"code":"E_VALIDATION","error":"<人读消息>"}`
//!   `error`（消息）字段与旧客户端完全兼容，新增的 `code` 为纯增量；
//! - OpenAI 风格信封（模型网关）：`{"error":{"code":"...","message":"..."}}`，
//!   `message` 兼容，`code` 增量。

use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Db(String),
    #[error("{0}")]
    Permission(String),
    #[error("{0}")]
    Timeout(String),
    #[error("{0}")]
    Auth(String),
    /// 服务端限流（HTTP 429）：调用方应等待后重试，不要立即重试
    #[error("{0}")]
    RateLimited(String),
    #[error("{0}")]
    Net(String),
    #[error("{0}")]
    Validation(String),
    /// 内部兜底：Tauri state 缺失、oneshot 通道丢失等服务端状态异常
    #[error("{0}")]
    Internal(String),
}

pub const E_DB: &str = "E_DB";
pub const E_PERM: &str = "E_PERM";
pub const E_TIMEOUT: &str = "E_TIMEOUT";
pub const E_AUTH: &str = "E_AUTH";
pub const E_RATELIMIT: &str = "E_RATELIMIT";
pub const E_NET: &str = "E_NET";
pub const E_VALIDATION: &str = "E_VALIDATION";
pub const E_INTERNAL: &str = "E_INTERNAL";

impl AppError {
    /// 稳定错误码字符串（跨语言契约，改名需走契约评审）
    pub fn code(&self) -> &'static str {
        match self {
            AppError::Db(_) => E_DB,
            AppError::Permission(_) => E_PERM,
            AppError::Timeout(_) => E_TIMEOUT,
            AppError::Auth(_) => E_AUTH,
            AppError::RateLimited(_) => E_RATELIMIT,
            AppError::Net(_) => E_NET,
            AppError::Validation(_) => E_VALIDATION,
            AppError::Internal(_) => E_INTERNAL,
        }
    }

    /// 本地 API 错误响应（信封：ok/code/error；error 为旧客户端消费的消息字段）
    pub fn error_json(&self) -> Value {
        json!({"ok": false, "code": self.code(), "error": self.to_string()})
    }

    /// OpenAI 风格错误对象（模型网关心信封：error.{code,message}）
    pub fn gateway_error_json(&self) -> Value {
        json!({"error": {"code": self.code(), "message": self.to_string()}})
    }

    pub fn db(msg: impl Into<String>) -> Self {
        AppError::Db(msg.into())
    }
    pub fn permission(msg: impl Into<String>) -> Self {
        AppError::Permission(msg.into())
    }
    pub fn timeout(msg: impl Into<String>) -> Self {
        AppError::Timeout(msg.into())
    }
    pub fn auth(msg: impl Into<String>) -> Self {
        AppError::Auth(msg.into())
    }
    pub fn ratelimited(msg: impl Into<String>) -> Self {
        AppError::RateLimited(msg.into())
    }
    pub fn net(msg: impl Into<String>) -> Self {
        AppError::Net(msg.into())
    }
    pub fn validation(msg: impl Into<String>) -> Self {
        AppError::Validation(msg.into())
    }
    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }
}

// ============================================================
// Tauri 命令面错误（R1 错误类型化 · 命令边界批，2026-09-25）
// ============================================================

/// Tauri 命令（invoke）统一错误负载。
///
/// serde 序列化为 `{"code":"E_NET","message":"..."}` 到达前端；
/// 前端 `invokeOk()` 按 code 分流（见 src/utils/invoke.ts）。
///
/// 兼容策略：命令签名从 `Result<T, String>` 迁到 `Result<T, CommandError>` 时，
/// 前端 reject 值由字符串变为对象——迁移中的命令必须成对改前端消费侧
/// （统一走 invokeOk / errText）。未迁移的旧命令仍以裸字符串 reject，
/// 前端 toAppError 兜底为 E_INTERNAL，两端互不阻塞。
#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for CommandError {}

impl From<AppError> for CommandError {
    fn from(e: AppError) -> Self {
        Self {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

/// CommandError → AppError：本地 API handler 调用命令后透传错误码
/// （此前 handler 一律硬编码 E_NET/E_INTERNAL，命令侧分类信息丢失）
impl From<CommandError> for AppError {
    fn from(e: CommandError) -> Self {
        let msg = e.message;
        match e.code {
            E_DB => AppError::Db(msg),
            E_PERM => AppError::Permission(msg),
            E_TIMEOUT => AppError::Timeout(msg),
            E_AUTH => AppError::Auth(msg),
            E_RATELIMIT => AppError::RateLimited(msg),
            E_NET => AppError::Net(msg),
            E_VALIDATION => AppError::Validation(msg),
            _ => AppError::Internal(msg),
        }
    }
}

/// 裸字符串兜底进 E_INTERNAL：迁移长尾里尚未分类的 `Err("...".into())`
/// 先以第七码过编译，分类在后续模块批逐个修正。
impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self {
            code: E_INTERNAL,
            message,
        }
    }
}

impl From<&str> for CommandError {
    fn from(message: &str) -> Self {
        Self {
            code: E_INTERNAL,
            message: message.to_string(),
        }
    }
}

/// 命令返回类型别名：`Result<T, CommandError>`
pub type CommandResult<T> = Result<T, CommandError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_mapping_is_stable() {
        assert_eq!(AppError::db("x").code(), "E_DB");
        assert_eq!(AppError::permission("x").code(), "E_PERM");
        assert_eq!(AppError::timeout("x").code(), "E_TIMEOUT");
        assert_eq!(AppError::auth("x").code(), "E_AUTH");
        assert_eq!(AppError::ratelimited("x").code(), "E_RATELIMIT");
        assert_eq!(AppError::net("x").code(), "E_NET");
        assert_eq!(AppError::validation("x").code(), "E_VALIDATION");
        assert_eq!(AppError::internal("x").code(), "E_INTERNAL");
    }

    #[test]
    fn api_envelope_keeps_legacy_error_field() {
        let v = AppError::validation("缺少 url").error_json();
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "E_VALIDATION");
        // 旧客户端只读 error：消息原样保留
        assert_eq!(v["error"], "缺少 url");
    }

    #[test]
    fn api_envelope_preserves_dynamic_message() {
        let v = AppError::db("启停失败: disk full").error_json();
        assert_eq!(v["code"], "E_DB");
        assert_eq!(v["error"], "启停失败: disk full");
    }

    #[test]
    fn gateway_envelope_uses_nested_code_and_message() {
        let v = AppError::auth("bad key").gateway_error_json();
        assert_eq!(v["error"]["code"], "E_AUTH");
        assert_eq!(v["error"]["message"], "bad key");
        // OpenAI 客户端读 error.message 不受影响
        assert!(v.get("ok").is_none());
    }

    #[test]
    fn command_error_serializes_to_code_message_pair() {
        let e = CommandError::from(AppError::auth("未配置 API Key"));
        let v = serde_json::to_value(&e).unwrap();
        // 前端 invokeOk 契约：恰好 {code,message} 两个字段
        assert_eq!(v, json!({"code": "E_AUTH", "message": "未配置 API Key"}));
    }

    #[test]
    fn command_error_from_bare_string_falls_back_to_internal() {
        let e = CommandError::from("磁盘只读".to_string());
        assert_eq!(e.code, E_INTERNAL);
        assert_eq!(e.message, "磁盘只读");
        let e2 = CommandError::from("state missing");
        assert_eq!(e2.code, E_INTERNAL);
    }

    #[test]
    fn command_error_display_matches_legacy_string_message() {
        // 旧前端/toast 直接展示 message：Display 必须等于裸字符串时代的内容
        let e = CommandError::from(AppError::net("AI 请求失败: timeout"));
        assert_eq!(format!("{}", e), "AI 请求失败: timeout");
    }

    #[test]
    fn command_error_to_app_error_preserves_code_and_message() {
        // API handler 透传：命令侧分类的错误码不得在边界丢失（此前硬编码 E_NET 的缺陷）
        for code in [E_DB, E_PERM, E_TIMEOUT, E_AUTH, E_RATELIMIT, E_NET, E_VALIDATION, E_INTERNAL] {
            let e = CommandError {
                code,
                message: "m".to_string(),
            };
            assert_eq!(AppError::from(e).code(), code, "code {} 透传丢失", code);
        }
        let e = CommandError {
            code: E_PERM,
            message: "只读模式".to_string(),
        };
        let app_err = AppError::from(e);
        assert_eq!(app_err.code(), E_PERM);
        assert_eq!(app_err.to_string(), "只读模式");
    }
}

//! 引擎层共享类型（P0-2 补完 · 2026-09-22）
//!
//! 目的：打破 `engine::ai_provider` ↔ `ai` 的循环依赖。
//! `ChatMessage` 等被双方共用的**数据类型**下沉到本模块，
//! 业务逻辑仍留在各自模块（`ai.rs` 提供实现，`ai_provider` 通过回调调用）。

use serde::{Deserialize, Serialize};

/// 对话消息（原 `ai::ChatMessage`，下沉以解耦）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// 工具结果回传时填（role=tool 时必填，对应 assistant 的 tool_call.id）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// assistant 消息请求工具调用时填
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<crate::ai::ToolCall>>,
}

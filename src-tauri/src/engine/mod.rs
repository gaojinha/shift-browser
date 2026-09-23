//! 引擎抽象层（战略阶段 0 · 批次 0.2 的第一块砖）
//!
//! 目的：把 WebKitGTK 的原生调用逐步收敛到本模块，业务模块不再散落直接依赖引擎细节，
//! 为将来的引擎升级/替换留出边界。
//!
//! 当前内容：内容过滤器（网络层规则拦截，`WebKitUserContentFilterStore`）。
//!
//! 说明：gtk-rs 的 `webkit2gtk` 绑定**缺少**内容过滤器 API（`auto/` 下只有
//! `user_content_manager`，且 `add_filter` 被注释为 Ignored），因此这里手写 FFI
//! 直接对接系统已安装的 `libwebkit2gtk-4.1`（2.52.3 实测符号齐全）。

pub mod agent;
pub mod ai_provider;
pub mod content;
pub mod content_filter;
pub mod layout;
pub mod mcp;
pub mod platform;
pub mod types;
pub mod llmstxt;
pub mod webmcp;
pub mod webview;
pub mod window_resize;

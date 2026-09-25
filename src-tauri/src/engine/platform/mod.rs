//! 平台适配层（P0-2 · 2026-09-22）
//!
//! 目标：把散落在 `tabs.rs` / `lib.rs` 里的 WebKitGTK/GTK 直接调用收敛到本层，
//! 为跨平台（Windows WebView2 / Android 系统 WebView）留出边界。
//!
//! 约定：**业务模块不直接 use webkit2gtk/gtk**，一切经 `engine::webview` 或本层。

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(target_os = "linux"))]
pub mod unsupported;

// 统一转发（业务模块只认 engine::platform::xxx，不关心平台）
#[cfg(target_os = "linux")]
pub use linux::{
    apply_cookie_policy, clear_web_data, download_dir, init_cookie_persistence,
    setup_web_extensions_dir, web_extensions_dir,
};

#[cfg(not(target_os = "linux"))]
pub use unsupported::{
    apply_cookie_policy, clear_web_data, download_dir, init_cookie_persistence,
    setup_memory_pressure, setup_web_extensions_dir, web_extensions_dir,
};

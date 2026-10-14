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

// ── 下载目录用户覆盖（跨平台状态，供 GTK 同步回调读）─────────────────────
// 用户可在设置里指定自定义下载目录；空串 = 未覆盖，回退系统 XDG 目录。
// 用 RwLock 而非原子量：路径是变长字符串，且写频极低（设置保存时才写一次）。
use std::sync::RwLock;

static DOWNLOAD_DIR_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// 设置下载目录覆盖值（空串 = 清除覆盖，回退系统默认）
pub fn set_download_dir_override(path: &str) {
    let trimmed = path.trim();
    *DOWNLOAD_DIR_OVERRIDE.write().unwrap() =
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
}

/// 获取下载目录覆盖值（None = 未设置，调用方回退系统默认）
pub fn get_download_dir_override() -> Option<String> {
    DOWNLOAD_DIR_OVERRIDE.read().unwrap().clone()
}

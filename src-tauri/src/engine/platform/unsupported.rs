//! 非 Linux 平台的占位实现（P0-2）
//!
//! Windows/Android 移植时在此实现 WebView2 / 系统 WebView 版本。

pub fn init_cookie_persistence(_data_home: &std::path::Path) -> Option<std::path::PathBuf> {
    None
}

pub fn setup_memory_pressure(_total_mb: u32) {}

pub fn clear_web_data(_cookies: bool, _cache: bool, _storage: bool) {}

pub fn download_dir() -> Option<std::path::PathBuf> {
    None
}

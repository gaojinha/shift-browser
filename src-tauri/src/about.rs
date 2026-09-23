//! 「关于」页数据：版本 / 运行时 / 环境 / 项目信息（只读，打开即显示，不依赖自检）

use serde::Serialize;
use tauri::Manager;

/// 关于页展示信息
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AboutInfo {
    /// 应用名
    pub app_name: String,
    /// 版本号（来自 Cargo.toml / tauri.conf.json，不写死）
    pub version: String,
    /// WebKitGTK 运行时版本（引擎升级/兼容排查第一手信息）
    pub webkit_version: String,
    /// 操作系统 + 架构，如 "linux x86_64"
    pub os_arch: String,
    /// 应用数据目录（备份/排查用）
    pub data_dir: String,
    /// 项目主页
    pub repo_url: String,
    /// 构建类型：debug / release
    pub profile: String,
}

/// 读取关于页信息（纯只读，失败时字段给占位值，不抛错）
pub fn collect(app: &tauri::AppHandle) -> AboutInfo {
    let webkit = unsafe {
        (
            webkit2gtk::ffi::webkit_get_major_version(),
            webkit2gtk::ffi::webkit_get_minor_version(),
            webkit2gtk::ffi::webkit_get_micro_version(),
        )
    };
    let data_dir = app
        .path()
        .app_data_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|_| "(无法获取)".to_string());
    let profile = if cfg!(debug_assertions) {
        "debug".to_string()
    } else {
        "release".to_string()
    };

    AboutInfo {
        app_name: "Shift Browser".to_string(),
        version: app.package_info().version.to_string(),
        webkit_version: format!("{}.{}.{}", webkit.0, webkit.1, webkit.2),
        os_arch: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        data_dir,
        repo_url: "https://gitcode.com/gaojinha/shiftbrowser-tauri".to_string(),
        profile,
    }
}
//! 「关于」页数据：版本 / 运行时 / 环境 / 项目信息（只读，打开即显示，不依赖自检）

use base64::Engine as _;
use serde::Serialize;
use tauri::Manager;

/// 应用图标（编译期嵌入 128x128，与托盘/窗口图标同源同图；base64 data URL 供前端直接显示）
const APP_ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");

/// 应用名（单一来源）
const APP_NAME: &str = "Shift Browser";

/// 项目主页（单一来源）
const REPO_URL: &str = "https://gitcode.com/gaojinha/shiftbrowser-tauri";

/// 应用图标 base64 data URL（data:image/png;base64,...）
fn icon_data_url() -> String {
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(APP_ICON_PNG)
    )
}

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
    /// 应用图标 data URL（data:image/png;base64,...）
    pub icon_data_url: String,
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
        app_name: APP_NAME.to_string(),
        version: app.package_info().version.to_string(),
        webkit_version: format!("{}.{}.{}", webkit.0, webkit.1, webkit.2),
        os_arch: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        data_dir,
        repo_url: REPO_URL.to_string(),
        profile,
        icon_data_url: icon_data_url(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_data_url_roundtrips_to_embedded_png() {
        let url = icon_data_url();
        let b64 = url
            .strip_prefix("data:image/png;base64,")
            .expect("应为 PNG base64 data URL 前缀");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        // 解码后与嵌入图标逐字节一致
        assert_eq!(bytes, APP_ICON_PNG);
        // PNG 魔数兜底：嵌入文件被替换/损坏时立刻暴露
        assert_eq!(
            &bytes[..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
    }

    #[test]
    fn about_info_serializes_camel_case() {
        let info = AboutInfo {
            app_name: APP_NAME.to_string(),
            version: "5.8.3".into(),
            webkit_version: "2.48.1".into(),
            os_arch: "linux x86_64".into(),
            data_dir: "/tmp/data".into(),
            repo_url: REPO_URL.to_string(),
            profile: "debug".into(),
            icon_data_url: icon_data_url(),
        };
        let obj = serde_json::to_value(&info)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        // 前端按 camelCase 取键（rename_all = "camelCase"），漂移即红
        for key in [
            "appName",
            "version",
            "webkitVersion",
            "osArch",
            "dataDir",
            "repoUrl",
            "profile",
            "iconDataUrl",
        ] {
            assert!(obj.contains_key(key), "缺 camelCase 键 {key}");
        }
        // 不允许 snake_case 残留
        assert!(obj.get("app_name").is_none());
        assert!(obj.get("webkit_version").is_none());
        assert!(obj.get("os_arch").is_none());
    }

    #[test]
    fn constants_are_stable_facts() {
        assert_eq!(APP_NAME, "Shift Browser");
        assert!(REPO_URL.starts_with("https://"), "主页必须 https");
    }
}

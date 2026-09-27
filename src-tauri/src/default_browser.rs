//! 默认浏览器设置（Linux xdg-settings / mimeapps.list）
//!
//! 职责：
//! 1. 查询当前默认浏览器（`xdg-settings get default-web-browser`）
//! 2. 设置 Shift Browser 为默认浏览器（写用户级 desktop 文件 + `xdg-settings set`）
//!
//! 为什么写用户级 desktop 文件：系统级（/usr/share/applications/Shift Browser.desktop）
//! 由 deb 打包生成，旧版本没有 MimeType 声明（xdg 不认它能开网页），且改它需要 root。
//! 用户级文件（~/.local/share/applications/）优先级更高、无需 root，补上：
//!   - `MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;`（xdg 认它为网页浏览器）
//!   - `Exec=shift-browser %U`（外部链接把 URL 传给应用，启动流程据此开新标签）

use serde::Serialize;
use std::path::PathBuf;

/// 本浏览器在 applications 目录下的 desktop-id（带空格，xdg 按文件名识别）
pub const DESKTOP_ID: &str = "Shift Browser.desktop";

/// 用户级 applications 目录下的 desktop 文件路径
fn user_desktop_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".local/share/applications")
            .join(DESKTOP_ID),
    )
}

/// 用户级 desktop 文件内容（带 MimeType + %U，让 xdg 认作网页浏览器）
/// 与打包模板 src-tauri/bundle/linux/Shift Browser.desktop 保持一致
const USER_DESKTOP_CONTENT: &str = "[Desktop Entry]
Type=Application
Name=Shift Browser
Comment=Shift Browser - AI 增强型浏览器（Tauri 重构版）
Exec=shift-browser %U
Icon=shift-browser
Terminal=false
Categories=Network;WebBrowser;
MimeType=x-scheme-handler/http;x-scheme-handler/https;text/html;
StartupWMClass=shift-browser
";

/// 运行 xdg-settings 命令（返回 stdout 去掉首尾空白）
fn xdg_settings(args: &[&str]) -> Result<String, String> {
    let out = std::process::Command::new("xdg-settings")
        .args(args)
        .output()
        .map_err(xdg_spawn_err)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(format!(
            "xdg-settings {:?} 失败: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// xdg-settings 起不来的原因翻译：缺命令直说装啥，别让用户猜
fn xdg_spawn_err(e: std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::NotFound {
        "系统缺少 xdg-settings（请安装 xdg-utils 后重试）".to_string()
    } else {
        format!("xdg-settings 执行失败: {}", e)
    }
}

/// 默认浏览器状态（前端展示用）
#[derive(Serialize)]
pub struct DefaultBrowserStatus {
    /// Shift Browser 是否已是默认浏览器
    pub is_default: bool,
    /// 当前默认浏览器 desktop-id（如 google-chrome.desktop；查询失败为空串）
    pub current: String,
    /// 本浏览器 desktop-id
    pub desktop_id: String,
    /// 用户级 desktop 文件是否就绪（存在且带 x-scheme-handler/http 声明）
    pub desktop_ready: bool,
}

/// 查询默认浏览器状态（只读）
#[tauri::command]
pub fn default_browser_status() -> DefaultBrowserStatus {
    let current = xdg_settings(&["get", "default-web-browser"]).unwrap_or_default();
    let desktop_ready = user_desktop_path()
        .map(|p| {
            p.exists()
                && std::fs::read_to_string(&p)
                    .map(|c| c.contains("x-scheme-handler/http"))
                    .unwrap_or(false)
        })
        .unwrap_or(false);
    DefaultBrowserStatus {
        is_default: current == DESKTOP_ID,
        current,
        desktop_id: DESKTOP_ID.to_string(),
        desktop_ready,
    }
}

/// 确保用户级 desktop 文件存在（带 MimeType + %U）。幂等：已就绪则直接返回。
fn ensure_user_desktop() -> Result<(), String> {
    let Some(path) = user_desktop_path() else {
        return Err("无法确定用户主目录".to_string());
    };
    let Some(dir) = path.parent() else {
        return Err("desktop 文件路径非法".to_string());
    };
    std::fs::create_dir_all(dir).map_err(|e| format!("创建 applications 目录失败: {}", e))?;
    std::fs::write(&path, USER_DESKTOP_CONTENT)
        .map_err(|e| format!("写入 desktop 文件失败: {}", e))?;
    log::info!(
        "[default-browser] 用户级 desktop 文件已写入: {}",
        path.display()
    );
    // 刷新 desktop 数据库（可选，失败不影响主流程）
    if let Ok(home) = std::env::var("HOME") {
        let _ = std::process::Command::new("update-desktop-database")
            .arg(format!("{}/.local/share/applications", home))
            .output();
    }
    Ok(())
}

/// 设置 Shift Browser 为默认浏览器
#[tauri::command]
pub fn set_default_browser() -> Result<DefaultBrowserStatus, String> {
    ensure_user_desktop()?;
    xdg_settings(&["set", "default-web-browser", DESKTOP_ID])?;
    log::info!("[default-browser] 已设为默认浏览器");
    Ok(default_browser_status())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_缺命令直说装啥() {
        let e = std::io::Error::new(std::io::ErrorKind::NotFound, "no such file");
        assert!(xdg_spawn_err(e).contains("xdg-utils"));
    }

    #[test]
    fn xdg_其他错误透出原文() {
        let e = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let m = xdg_spawn_err(e);
        assert!(m.contains("执行失败"));
        assert!(!m.contains("xdg-utils"));
    }
}

//! file:// → shift:// 自定义协议转换（WebKitGTK UserScript 注入需要）

use tauri::{AppHandle, Manager, WebviewUrl};

/// 解析目标 URL；file:// 的内部页（newtab/worldnav）转 shift:// 自定义协议，
/// 让 WebKitGTK 的 UserScript（IPC桥）能注入；其余 file:// 保持 External。
pub(super) fn resolve_webview_url(app: &AppHandle, url: &str) -> Result<WebviewUrl, String> {
    let parsed_url = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("URL 解析失败: {}", e))?;

    // file:// URL 用 shift:// 自定义协议替代，让 WebKitGTK 的 UserScript（IPC桥）能注入
    let webview_url = if parsed_url.scheme() == "file" {
        let path = parsed_url.path();
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("无法获取 app_data_dir: {}", e))?;
        let newtab_dir = app_data.join("newtab");
        let newtab_index = newtab_dir.join("index.html");
        let worldnav = newtab_dir.join("world-nav.html");
        if path == newtab_index.to_string_lossy() || path.ends_with("newtab/index.html") {
            log::info!("[tabs] file URL → shift://newtab");
            tauri::WebviewUrl::CustomProtocol(
                "shift://newtab"
                    .parse()
                    .map_err(|e| format!("URL 解析失败: {}", e))?,
            )
        } else if path == worldnav.to_string_lossy() || path.ends_with("newtab/world-nav.html")
        {
            log::info!("[tabs] file URL → shift://worldnav");
            tauri::WebviewUrl::CustomProtocol(
                "shift://worldnav"
                    .parse()
                    .map_err(|e| format!("URL 解析失败: {}", e))?,
            )
        } else {
            log::info!("[tabs] file URL 保持 External: {}", path);
            WebviewUrl::External(parsed_url)
        }
    } else {
        WebviewUrl::External(parsed_url)
    };
    Ok(webview_url)
}

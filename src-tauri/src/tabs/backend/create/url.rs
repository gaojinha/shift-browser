//! file:// → shift:// 自定义协议转换（WebKitGTK UserScript 注入需要）

use tauri::{AppHandle, Manager, WebviewUrl};

/// 解析目标 URL；file:// 的内部页（newtab/worldnav）转 shift:// 自定义协议，
/// 让 WebKitGTK 的 UserScript（IPC桥）能注入；其余 file:// 保持 External。
///
/// 无痕（private）例外（2026-09-25 修复）：`shift://` 自定义协议只注册在**默认 WebContext**，
/// 无痕标签走 incognito 的 ephemeral WebContext → `shift://newtab` 加载失败（"The URL can't
/// be shown"、页面空白）。故无痕标签的 file:// 内部页**保持 External 直接加载**，
/// 保证能显示（代价：无痕下 newtab 的 IPC 桥不可用，符合"无痕不落数据"预期）。
pub(super) fn resolve_webview_url(
    app: &AppHandle,
    url: &str,
    private: bool,
) -> Result<WebviewUrl, String> {
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
        let is_newtab =
            path == newtab_index.to_string_lossy() || path.ends_with("newtab/index.html");
        let is_worldnav =
            path == worldnav.to_string_lossy() || path.ends_with("newtab/world-nav.html");
        if private && (is_newtab || is_worldnav) {
            // 无痕：ephemeral context 无 shift:// 协议 → 直接 file:// 加载
            log::info!("[tabs] 无痕标签 file URL 保持 External（ephemeral context 无自定义协议）");
            WebviewUrl::External(parsed_url)
        } else if is_newtab {
            log::info!("[tabs] file URL → shift://newtab");
            tauri::WebviewUrl::CustomProtocol(
                "shift://newtab"
                    .parse()
                    .map_err(|e| format!("URL 解析失败: {}", e))?,
            )
        } else if is_worldnav {
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

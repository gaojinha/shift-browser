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
        match file_internal_page(path, &app_data, private) {
            // 无痕：ephemeral context 无 shift:// 协议 → 直接 file:// 加载
            Some(_) if private => {
                log::info!("[tabs] 无痕标签 file URL 保持 External（ephemeral context 无自定义协议）");
                WebviewUrl::External(parsed_url)
            }
            Some("newtab") => {
                log::info!("[tabs] file URL → shift://newtab");
                tauri::WebviewUrl::CustomProtocol(
                    "shift://newtab"
                        .parse()
                        .map_err(|e| format!("URL 解析失败: {}", e))?,
                )
            }
            Some(_) => {
                log::info!("[tabs] file URL → shift://worldnav");
                tauri::WebviewUrl::CustomProtocol(
                    "shift://worldnav"
                        .parse()
                        .map_err(|e| format!("URL 解析失败: {}", e))?,
                )
            }
            None => {
                log::info!("[tabs] file URL 保持 External: {}", path);
                WebviewUrl::External(parsed_url)
            }
        }
    } else {
        WebviewUrl::External(parsed_url)
    };
    Ok(webview_url)
}

/// 判断 file:// 路径是否指向内部页，返回 "newtab" / "worldnav"（None = 普通文件）。
///
/// 纯函数可单测：同时接受「app_data 拼出的完整路径」与「任意目录下 ends_with」两种形态
/// （后者兼容 app_data_dir 变更/打包路径差异的历史口径）。无痕例外由调用方处理。
fn file_internal_page(path: &str, app_data: &std::path::Path, _private: bool) -> Option<&'static str> {
    let newtab_index = app_data.join("newtab").join("index.html");
    let worldnav = app_data.join("newtab").join("world-nav.html");
    let is_newtab = path == newtab_index.to_string_lossy() || path.ends_with("newtab/index.html");
    let is_worldnav =
        path == worldnav.to_string_lossy() || path.ends_with("newtab/world-nav.html");
    if is_newtab {
        Some("newtab")
    } else if is_worldnav {
        Some("worldnav")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 内部页判定命中newtab与worldnav两种形态() {
        let app_data = std::path::Path::new("/home/u/.local/share/com.shiftbrowser.app");
        let full = app_data.join("newtab").join("index.html").to_string_lossy().to_string();
        assert_eq!(file_internal_page(&full, app_data, false), Some("newtab"));
        // 任意目录下按文件名后缀命中（历史口径）
        assert_eq!(
            file_internal_page("/some/other/dir/newtab/index.html", app_data, false),
            Some("newtab")
        );
        let full_world = app_data
            .join("newtab")
            .join("world-nav.html")
            .to_string_lossy()
            .to_string();
        assert_eq!(file_internal_page(&full_world, app_data, false), Some("worldnav"));
        assert_eq!(
            file_internal_page("/x/newtab/world-nav.html", app_data, false),
            Some("worldnav")
        );
    }

    #[test]
    fn 普通文件与相似路径不算内部页() {
        let app_data = std::path::Path::new("/data");
        assert_eq!(
            file_internal_page("/data/newtab/other.html", app_data, false),
            None,
            "同目录其他文件不是内部页"
        );
        assert_eq!(
            file_internal_page("/data/newtab/index.html.bak", app_data, false),
            None,
            "仅后缀相似不命中"
        );
        assert_eq!(file_internal_page("/data/index.html", app_data, false), None);
        // 空 app_data 时 ends_with 口径仍兜底
        assert_eq!(file_internal_page("/a/newtab/index.html", std::path::Path::new(""), false), Some("newtab"));
    }

    #[test]
    fn url解析失败报错不panic() {
        // 不构造 AppHandle（GTK 依赖），解析失败分支先行返回，可用 None 探测错误文案
        let bad = "ht tp://bad url";
        let err = bad
            .parse::<tauri::Url>()
            .err()
            .map(|e| format!("URL 解析失败: {}", e));
        assert!(err.is_some(), "非法 URL 必须解析失败");
        let msg = err.unwrap();
        assert!(msg.contains("URL 解析失败"), "错误文案口径不变: {msg}");
    }
}

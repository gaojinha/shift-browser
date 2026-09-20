//! WebView 基础操作（引擎抽象层 · 批次 0.2 第二项迁移）
//!
//! 目的：把业务模块里直接调用的 WebKit API 收敛到一处
//! ① 行为统一（默认设置/JS 执行的错误处理口径一致）
//! ② 升级或替换引擎时只改本模块，业务代码不动
//!
//! 现状收敛对象：`Settings` 默认值、`evaluate_javascript`（原先散落 tabs.rs 3 处 + extensions.rs 6 处）

use webkit2gtk::{SettingsExt, WebView, WebViewExt};

/// 页面 UA。WebKitGTK 默认 UA 残缺（无 Safari 标记），Bing 等站点会误判为未知浏览器
/// 返回简化版单屏页面（无法滚动）。
/// 版本号保守取 Chrome 120（2026-09-18 决策）：UA 是唯一直接影响服务端响应的指纹项，
/// 个别站点对高版本 UA 有兼容分支，宁保守勿激进。
pub const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 新建 child webview 时应用页面默认设置。
/// 注意：**不能新建 Settings 替换**（会丢失 Tauri 默认设置导致链接点击失效），
/// 只能在现有 Settings 上改属性。
pub fn apply_default_settings(wv: &WebView) {
    let Some(settings) = webkit2gtk::WebViewExt::settings(wv) else {
        log::warn!("[engine] 获取 WebView Settings 失败，跳过默认设置");
        return;
    };
    // 允许 JS 自动开窗（target=_blank / window.open → 触发 on_new_window 建新标签）
    settings.set_javascript_can_open_windows_automatically(true);
    // 开发者工具：F12 / Ctrl+Shift+I（快捷键在 GTK 窗口层兜底拦截）
    settings.set_enable_developer_extras(true);
    settings.set_user_agent(Some(DEFAULT_USER_AGENT));
}

/// 执行 JS，忽略返回值。
/// 失败只记 debug：页面未加载完时 evaluate 必然失败，属正常情形（注入类脚本有 UserScript 兜底）。
pub fn eval_js(wv: &WebView, script: &str) {
    wv.evaluate_javascript(script, None, None, None::<&gtk::gio::Cancellable>, |res| {
        if let Err(e) = res {
            log::debug!("[engine] eval_js 跳过（页面未就绪）: {}", e);
        }
    });
}

/// 执行 JS 并把结果转成字符串回调（回调在主线程执行）。
/// 用于需要读取页面信息的场景（favicon 提取、滚动诊断等）。
pub fn eval_js_with<F>(wv: &WebView, script: &str, cb: F)
where
    F: FnOnce(Result<String, String>) + 'static,
{
    wv.evaluate_javascript(
        script,
        None,
        None,
        None::<&gtk::gio::Cancellable>,
        move |res| match res {
            Ok(v) => cb(Ok(v.to_string())),
            Err(e) => cb(Err(e.to_string())),
        },
    );
}

/// 开启页面 console 输出到 stdout（诊断 JS 错误用）。
/// 任何加载事件都设置，不依赖 Finished（部分页面 Finished 不触发）。
pub fn enable_console_output(wv: &WebView) {
    let Some(settings) = webkit2gtk::WebViewExt::settings(wv) else {
        return;
    };
    settings.set_enable_write_console_messages_to_stdout(true);
}

/// 刷新页面（等价于工具栏刷新按钮）。
pub fn reload(wv: &WebView) {
    webkit2gtk::WebViewExt::reload(wv);
}

/// 查询导航状态（能否后退/前进）。
pub fn navigation_state(wv: &WebView) -> (bool, bool) {
    (
        webkit2gtk::WebViewExt::can_go_back(wv),
        webkit2gtk::WebViewExt::can_go_forward(wv),
    )
}

/// 打开开发者工具（Web Inspector）。
/// 注意：调用方须保证在主线程（GTK 信号处理器里需异步派发，避免主线程重入）。
pub fn open_inspector(wv: &WebView) {
    if let Some(inspector) = webkit2gtk::WebViewExt::inspector(wv) {
        // 显式用 WebInspectorExt，避免与 gtk WidgetExt::show 歧义
        webkit2gtk::WebInspectorExt::show(&inspector);
    }
}

/// 静音/取消静音（WebKitGTK 2.36+ 支持 per-webview 静音，`webkit_web_view_set_is_muted`）。
/// 注意：调用方须保证在主线程（异步派发）。
pub fn set_muted(wv: &WebView, muted: bool) {
    webkit2gtk::WebViewExt::set_is_muted(wv, muted);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ua_looks_like_chrome() {
        // UA 必须同时含 Safari 标记与 Chrome 版本号，否则部分站点返回简化页
        assert!(DEFAULT_USER_AGENT.contains("Safari/537.36"));
        assert!(DEFAULT_USER_AGENT.contains("Chrome/120"));
        assert!(DEFAULT_USER_AGENT.starts_with("Mozilla/5.0"));
    }
}

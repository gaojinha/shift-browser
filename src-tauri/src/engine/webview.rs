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
/// 开发者工具开关（安全 #6 · 2026-09-22）：默认关，由设置 enable_devtools 决定。
/// 开启后 F12/Ctrl+Shift+I 可用（本地风险低，但共享/kiosk 场景应关闭）。
pub static DEVTOOLS_ENABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

// 同模块单测断言 UA 形态（lib 构建期无业务引用，保留作契约常量）
#[allow(dead_code)]
pub const DEFAULT_USER_AGENT: &str = crate::useragent::DEFAULT_UA;

/// 新建 child webview 时应用页面默认设置。
/// 注意：**不能新建 Settings 替换**（会丢失 Tauri 默认设置导致链接点击失效），
/// 只能在现有 Settings 上改属性。
pub fn apply_default_settings(wv: &WebView, host: &str) {
    let Some(settings) = webkit2gtk::WebViewExt::settings(wv) else {
        log::warn!("[engine] 获取 WebView Settings 失败，跳过默认设置");
        return;
    };
    // 允许 JS 自动开窗（target=_blank / window.open → 触发 on_new_window 建新标签）
    settings.set_javascript_can_open_windows_automatically(true);
    // 开发者工具：F12 / Ctrl+Shift+I（快捷键在 GTK 窗口层兜底拦截）
    // 安全 #6：开发者工具按开关（默认关）
    settings.set_enable_developer_extras(
        DEVTOOLS_ENABLED.load(std::sync::atomic::Ordering::Relaxed),
    );
    // UA 动态取（站点覆盖 > 全局 > 默认）
    settings.set_user_agent(Some(&crate::useragent::ua_for(host)));
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

// ============================================================
// P0-2 补完（2026-09-22）：WebView 后端抽象 trait（真抽象版）
// ============================================================
//
// 目的：业务模块（tabs.rs 等）不再直接依赖 `webkit2gtk::WebView` 具体类型。
// 关键设计：trait 方法**不出现任何 WebKitGTK 类型**——业务侧只需持有
// `impl WebViewBackend`（当前即 `webkit2gtk::WebView`，由 `pwv.inner()` 取得），
// 调用 trait 方法即可，**无需 `use webkit2gtk`**。
//
// 移植时：为 WebView2/系统 WebView 各提供一份 `impl WebViewBackend`，业务代码零改动。

/// WebView 后端能力抽象
// 跨平台（R5 WebView2）预留的统一后端面：部分方法当前仅经自由函数调用，
// 待业务层全切到 trait 方法后各项即被消费
#[allow(dead_code)]
pub trait WebViewBackend {
    /// 重新加载
    fn reload(&self);
    /// 后退
    fn go_back(&self);
    /// 前进
    fn go_forward(&self);
    /// 导航状态 (can_back, can_forward)
    fn nav_state(&self) -> (bool, bool);
    /// 当前 URI
    fn uri(&self) -> Option<String>;
    /// 当前 host（UA 覆盖等用）
    fn host(&self) -> String;
    /// 设置静音
    fn set_muted(&self, muted: bool);
    /// 打开开发者工具
    fn open_inspector(&self);
    /// 应用页面默认设置（UA/DevTools/JS 弹窗）
    fn apply_default_settings(&self, host: &str);
    /// 执行 JS（忽略返回值）
    fn eval_js(&self, script: &str);
    /// 开启 console 输出到 stdout
    fn enable_console_output(&self);
    /// 挂载下载策略（不可显示 MIME → 下载）
    fn attach_download_policy(&self);
}

/// WebKitGTK 后端的 trait 实现（唯一出现 WebKitGTK 类型的地方）
impl WebViewBackend for WebView {
    fn reload(&self) {
        webkit2gtk::WebViewExt::reload(self);
    }
    fn go_back(&self) {
        webkit2gtk::WebViewExt::go_back(self);
    }
    fn go_forward(&self) {
        webkit2gtk::WebViewExt::go_forward(self);
    }
    fn nav_state(&self) -> (bool, bool) {
        (
            webkit2gtk::WebViewExt::can_go_back(self),
            webkit2gtk::WebViewExt::can_go_forward(self),
        )
    }
    fn uri(&self) -> Option<String> {
        webkit2gtk::WebViewExt::uri(self).map(|s| s.to_string())
    }
    fn host(&self) -> String {
        webkit2gtk::WebViewExt::uri(self)
            .and_then(|u| url::Url::parse(u.as_str()).ok())
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_default()
    }
    fn set_muted(&self, muted: bool) {
        webkit2gtk::WebViewExt::set_is_muted(self, muted);
    }
    fn open_inspector(&self) {
        open_inspector(self);
    }
    fn apply_default_settings(&self, host: &str) {
        apply_default_settings(self, host);
    }
    fn eval_js(&self, script: &str) {
        eval_js(self, script);
    }
    fn enable_console_output(&self) {
        enable_console_output(self);
    }
    fn attach_download_policy(&self) {
        attach_download_policy(self);
    }
}


/// 下载策略：主框架导航到"不可显示的 MIME"（apk/zip/exe…）时转为下载。
/// 从 tabs.rs 下沉（P0-2 补完）：业务模块不再直接 use webkit2gtk 的 PolicyDecision 类型。
pub fn attach_download_policy(wv: &WebView) {
    use gtk::glib::prelude::Cast;
    use webkit2gtk::{
        PolicyDecisionExt, PolicyDecisionType, ResponsePolicyDecision, ResponsePolicyDecisionExt,
    };
    wv.connect_decide_policy(|_wv, decision, decision_type| {
        if decision_type == PolicyDecisionType::Response {
            if let Some(resp) = decision.downcast_ref::<ResponsePolicyDecision>() {
                if !resp.is_mime_type_supported() {
                    log::info!("[downloads] 不可显示 MIME → 转下载流程");
                    decision.download();
                    return true;
                }
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ua_looks_like_chrome() {
        // UA 必须同时含 Safari 标记与 Chrome 版本号，否则部分站点返回简化页
        assert!(DEFAULT_USER_AGENT.contains("Safari/537.36"));
        assert!(DEFAULT_USER_AGENT.starts_with("Mozilla/5.0"));
        // 版本号需 ≥ 131（2026-09-22：原断言写死 120，UA 升级后过时；
        // 改为"主版本号 >= 131"的区间断言，避免每次升版本都要改测试）
        let major: u32 = DEFAULT_USER_AGENT
            .split("Chrome/")
            .nth(1)
            .and_then(|s| s.split('.').next())
            .and_then(|s| s.parse().ok())
            .expect("UA 应含 Chrome/<主版本>");
        assert!(major >= 131, "Chrome 主版本过低: {}", major);
    }
}

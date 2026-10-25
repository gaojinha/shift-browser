//! CEF Views 单栈外壳（2026-10-20 起，替代 GTK + CEF 子窗口方案）
//!
//! 结构：一个 **CEF 自己创建的顶层窗口**，内部用 BoxLayout 上下分两块：
//!
//! ```text
//! ┌──────────────────────────────────────────┐
//! │ 工具栏 BrowserView（本地 WebUI，48px）    │  ← HTML/CSS 画，跟网页同一套栈
//! ├──────────────────────────────────────────┤
//! │ 内容 BrowserView（真网页，flex=1）        │
//! └──────────────────────────────────────────┘
//! ```
//!
//! 为什么不嵌 GTK：窗口、输入、焦点、IME 全在 CEF 内部，不存在两个工具包
//! 抢一个窗口的问题（Brave 就是这个路子：Views + WebUI，同一栈）。
//!
//! 线程：**全部函数只能在 CEF UI 线程（GTK 主线程）调用。**

use cef::*;
use std::cell::RefCell;

use super::browser::BROWSERS;

/// 内容页的固定 tab id（导航命令按这个取句柄）
pub const CONTENT_ID: &str = "shell-content";
/// 工具栏页的固定 tab id
pub const TOOLBAR_ID: &str = "shell-toolbar";
/// 工具栏高度（像素）
pub const TOOLBAR_HEIGHT: i32 = 48;

thread_local! {
    /// Client 保活：CEF 在浏览器存活期间会引用 client，提前释放会崩
    static SHELL_CLIENTS: RefCell<Vec<Client>> = const { RefCell::new(Vec::new()) };

    /// 当前单栈窗口句柄（窗口创建回调里登记，供延迟 show 用）
    static SHELL_WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };
}

/// 打开单栈窗口。**必须在 CEF UI 线程调用。**
///
/// `content_url`：初始网页；`toolbar_url`：本地工具栏页（file:// 或 data:）
pub fn open(content_url: &str, toolbar_url: &str) -> Result<(), String> {
    let content_client: Client = ShellClient::new(CONTENT_ID.to_string(), false);
    let toolbar_client: Client = ShellClient::new(TOOLBAR_ID.to_string(), true);

    let clone1 = content_client.clone();
    let clone2 = toolbar_client.clone();
    let clone3 = content_client.clone();
    let clone4 = toolbar_client.clone();
    // 保活一份（之后每次创建 browser 时 CEF 还会再引用一次）
    SHELL_CLIENTS.with(|v| {
        let mut v = v.borrow_mut();
        v.push(clone1);
        v.push(clone2);
    });

    let mut content_client = clone3;
    let mut toolbar_client = clone4;

    // 两个视图都必须是 ALLOY 风格，否则 CEF 拒绝把它们加进 Alloy 窗口
    // （报错：Cannot add Chrome style BrowserView to Alloy style Window）
    let mut content_delegate = ShellViewDelegate::new(RuntimeStyle::ALLOY, 0);
    let mut toolbar_delegate = ShellViewDelegate::new(RuntimeStyle::ALLOY, TOOLBAR_HEIGHT);

    let content_view = browser_view_create(
        Some(&mut content_client),
        Some(&CefString::from(content_url)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut content_delegate),
    )
    .ok_or_else(|| "内容 BrowserView 创建失败".to_string())?;

    let toolbar_view = browser_view_create(
        Some(&mut toolbar_client),
        Some(&CefString::from(toolbar_url)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut toolbar_delegate),
    )
    .ok_or_else(|| "工具栏 BrowserView 创建失败".to_string())?;

    // 工具栏高度由 ViewDelegate::preferred_size 固定（flex=0 时生效）

    let mut delegate = ShellWindowDelegate::new(
        RefCell::new(Some(content_view)),
        RefCell::new(Some(toolbar_view)),
    );

    let window = window_create_top_level(Some(&mut delegate));
    if window.is_none() {
        return Err("window_create_top_level 返回空".to_string());
    }

    log::info!("[cef] 单栈窗口已创建：content={content_url} toolbar={toolbar_url}");
    Ok(())
}

/// 延迟映射窗口。
///
/// 实测：在 `on_window_created` 回调栈内调用 `show()` 不会真正 map，
/// 必须在回调返回后（主循环下一轮）再调一次。调用方用 glib 定时器触发。
pub fn force_show() {
    log::info!("[cef] force_show 被调用");
    SHELL_WINDOW.with(|w| {
        if let Some(window) = w.borrow().as_ref() {
            log::info!("[cef] force_show: 调 window.show() 之前");
            window.show();
            log::info!("[cef] force_show: window.show() 已返回");
            // 兜底：CEF 的 show() 在 X11 + 无 GPU 组合下只登记不映射，
            // 直接用 X11 把窗口 map 出来（窗口句柄由 CEF 提供，同一连接由 CEF 持有）
            let handle = window.window_handle();
            log::info!("[cef] 延迟 show() 已执行，window_handle=0x{handle:x}");
            if handle > 1 {
                let dpy = cef::get_xdisplay();
                if !dpy.is_null() {
                    unsafe {
                        let dpy = dpy as *mut x11::xlib::Display;
                        let win = handle as x11::xlib::Window;

                        // Mutter 对没有 WM_CLASS 的 X11 窗口会停在 Iconic 状态不映射，
                        // CEF 自己又不设，只能这里补上
                        let mut name = b"shift\0".to_vec();
                        let mut class = b"Shift\0".to_vec();
                        let mut hint = x11::xlib::XClassHint {
                            res_name: name.as_mut_ptr() as *mut std::os::raw::c_char,
                            res_class: class.as_mut_ptr() as *mut std::os::raw::c_char,
                        };
                        x11::xlib::XSetClassHint(dpy, win, &mut hint);

                        x11::xlib::XMapRaised(dpy, win);
                        x11::xlib::XFlush(dpy);
                    }
                    log::info!("[cef] XSetClassHint + XMapRaised 已调用");
                }
            }
        } else {
            log::warn!("[cef] force_show：没有窗口句柄");
        }
    });
}

/// 让内容页导航（工具栏 JS 经 shell_msg 调 "nav" 时实际走 CefShellMsgHandler，本函数为
/// 命令式入口预留，P2 外部导航 API 用）。
#[allow(dead_code)]
pub fn navigate(url: &str) {
    BROWSERS.with(|m| {
        let map = m.borrow();
        let Some(browser) = map.get(CONTENT_ID) else {
            log::warn!("[cef] navigate：内容浏览器还没创建好");
            return;
        };
        let Some(frame) = browser.main_frame() else {
            return;
        };
        frame.load_url(Some(&CefString::from(url)));
    });
}

// ============================================================
// 窗口 / 视图
// ============================================================

// 工具栏视图委托：把首选高度钉死在 48px。
// BoxLayout 里 flex=0 的子视图按首选尺寸分配，正好用这个控制高度。
wrap_browser_view_delegate! {
    struct ShellViewDelegate {
        runtime_style: RuntimeStyle,
        preferred_height: i32,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size {
                width: 100,
                height: self.preferred_height,
            }
        }
    }

    impl BrowserViewDelegate {
        fn browser_runtime_style(&self) -> RuntimeStyle {
            self.runtime_style
        }
    }
}

wrap_window_delegate! {
    struct ShellWindowDelegate {
        content: RefCell<Option<BrowserView>>,
        toolbar: RefCell<Option<BrowserView>>,
    }

    impl ViewDelegate {
        fn preferred_size(&self, _view: Option<&mut View>) -> Size {
            Size { width: 1280, height: 860 }
        }
    }

    impl PanelDelegate {}

    impl WindowDelegate {
        fn on_window_created(&self, window: Option<&mut Window>) {
            let Some(window) = window else { return };

            let mut content = self.content.borrow_mut().take();
            let mut toolbar = self.toolbar.borrow_mut().take();
            let (Some(content), Some(toolbar)) = (content.as_mut(), toolbar.as_mut()) else {
                log::error!("[cef] 窗口创建时缺少视图");
                return;
            };

            let mut content_view = View::from(&*content);
            let mut toolbar_view = View::from(&*toolbar);

            // 先加工具栏（上），再加内容（下）——BoxLayout 按添加顺序排
            window.add_child_view(Some(&mut toolbar_view));
            window.add_child_view(Some(&mut content_view));

            let settings = BoxLayoutSettings {
                horizontal: 0,
                between_child_spacing: 0,
                // 交叉轴（横向）拉伸：不加这条，工具栏只会占 preferred_size 的 100px 宽
                cross_axis_alignment: AxisAlignment::STRETCH,
                ..Default::default()
            };
            if let Some(layout) = window.set_to_box_layout(Some(&settings)) {
                // 工具栏不伸缩，内容吃满剩余空间
                layout.set_flex_for_view(Some(&mut toolbar_view), 0);
                layout.set_flex_for_view(Some(&mut content_view), 1);
            } else {
                log::error!("[cef] set_to_box_layout 失败");
            }

            // 点击穿透根因修复（2026-10-25 真机实测）：本回调里 BoxLayout 还没首跑，
            // content 初始 bounds 覆盖整窗（0,0,1280x860），又因晚于 toolbar 添加而
            // z 序更高——首帧布局前的窗口期内，点工具栏区域全部落进内容页。
            // 这里强制同步布局，保证窗口 map 出可点击时 bounds 已就位
            //（toolbar 0..48 / content 48..）。
            window.layout();

            // 键盘焦点（2026-10-25 真机实测补充）：CEF 的 BrowserView 默认 **不可聚焦**，
            // 只调 browser.set_focus(1) 不足以让网页收到键盘事件——View 层也要
            // set_focusable + request_focus，否则鼠标点击停在 View 层、键盘进不去网页
            // （表现：点地址栏有反应但打字无输入）。
            toolbar_view.set_focusable(1);
            toolbar_view.request_focus();
            log::info!("[cef] 工具栏 View 已设为可聚焦并请求焦点");

            // 布局后两视图实际 bounds 记录一次（windowless Views 无其它观测手段）
            let tb = toolbar_view.bounds();
            let ct = content_view.bounds();
            let wn = window.bounds();
            log::info!(
                "[cef] bounds 实测: toolbar=({}, {}, {}x{}) content=({}, {}, {}x{}) window=({}, {}, {}x{})",
                tb.x, tb.y, tb.width, tb.height,
                ct.x, ct.y, ct.width, ct.height,
                wn.x, wn.y, wn.width, wn.height
            );

            window.set_title(Some(&CefString::from("Shift")));
            window.center_window(Some(&Size { width: 1280, height: 860 }));

            // 窗口句柄留一份：实测在 on_window_created 里 show() 只登记不映射
            // （xwininfo 显示 Map State: IsUnMapped），需要出回调后再 show 一次
            SHELL_WINDOW.with(|w| {
                *w.borrow_mut() = Some(window.clone());
            });
            // 注意：不要在这里调 window.show()。
            // 实测它会卡死在回调里（窗口句柄/标题/尺寸都生效了，但 show() 不返回，
            // 后面的日志和延迟映射都不执行）。改由主循环里的 force_show() 用
            // XMapRaised 直接映射。
            log::info!("[cef] 单栈窗口已创建（等待延迟映射）");
        }

        fn on_window_destroyed(&self, _window: Option<&mut Window>) {
            self.content.borrow_mut().take();
            self.toolbar.borrow_mut().take();
            log::info!("[cef] 单栈窗口已销毁");
        }

        fn can_close(&self, _window: Option<&mut Window>) -> i32 {
            1
        }

        fn initial_show_state(&self, _window: Option<&mut Window>) -> ShowState {
            ShowState::NORMAL
        }

        fn window_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

// ============================================================
// Client / Handler
// ============================================================

wrap_client! {
    struct ShellClient {
        tab_id: String,
        toolbar: bool,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(ShellLifeSpan::new(self.tab_id.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(ShellRequestHandler::new(self.tab_id.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            // 只有工具栏需要接收 JS 传上来的动作
            if self.toolbar {
                Some(ShellToolbarDisplay::new())
            } else {
                None
            }
        }
    }
}

wrap_life_span_handler! {
    struct ShellLifeSpan {
        tab_id: String,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            let Some(b) = browser else { return };
            if let Some(h) = b.host() {
                log::info!(
                    "[cef] {} on_after_created window_handle=0x{:x}",
                    self.tab_id,
                    h.window_handle()
                );
            }
            BROWSERS.with(|m| {
                m.borrow_mut().insert(self.tab_id.clone(), b.clone());
            });

            // 键盘焦点（2026-10-25 真机实测：点击不穿透后，地址栏仍收不到键盘输入）：
            // CEF 的 BrowserView 只是视图容器，网页要接收键盘事件必须先 browser.set_focus(1)。
            // 初始焦点给工具栏（地址栏），点内容页时 CEF 会按点击位置自行切换。
            if self.tab_id == TOOLBAR_ID {
                super::browser::focus(b);
                log::info!("[cef] 工具栏已获得键盘焦点（地址栏可输入）");
            }
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            BROWSERS.with(|m| {
                m.borrow_mut().remove(&self.tab_id);
            });
        }
    }
}

// 工具栏 → 浏览器进程的动作通道。
//
// 临时用 console.log 桥（MVP）：工具栏 JS 打 `SHIFT_ACT:nav|<url>`，
// 这里解析后驱动内容页。等下一版换成正式 MessageRouter（cefQuery）。
wrap_display_handler! {
    struct ShellToolbarDisplay;

    impl DisplayHandler {
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: std::os::raw::c_int,
        ) -> i32 {
            let Some(msg) = message else { return 0 };
            let msg = msg.to_string();
            let Some(action) = msg.strip_prefix("SHIFT_ACT:") else {
                return 0;
            };
            handle_action(action.trim());
            0
        }
    }
}

/// 执行工具栏动作
fn handle_action(action: &str) {
    let (cmd, arg) = match action.split_once('|') {
        Some((c, a)) => (c, a),
        None => (action, ""),
    };

    BROWSERS.with(|m| {
        let map = m.borrow();
        let Some(browser) = map.get(CONTENT_ID) else {
            return;
        };
        match cmd {
            "nav" => {
                let url = normalize_url(arg);
                if url.is_empty() {
                    return;
                }
                log::info!("[cef] 工具栏导航 -> {url}");
                let Some(frame) = browser.main_frame() else {
                    return;
                };
                frame.load_url(Some(&CefString::from(url.as_str())));
            }
            "back" => browser.go_back(),
            "forward" => browser.go_forward(),
            "reload" => browser.reload(),
            "stop" => browser.stop_load(),
            other => log::warn!("[cef] 未知工具栏动作: {other}"),
        }
    });
}

/// 地址栏输入补全：没有 scheme 就补 https://
fn normalize_url(input: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return String::new();
    }
    if s.contains("://") || s.starts_with("about:") || s.starts_with("data:") {
        return s.to_string();
    }
    // 看着像域名/IP 才补 https
    if s.contains('.') && !s.contains(' ') {
        format!("https://{s}")
    } else {
        format!("https://www.google.com/search?q={}", urlencode(s))
    }
}

/// 极简 URL 编码（只用于搜索词，够用即可）
fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{normalize_url, urlencode};

    // ---- normalize_url：地址栏补全规则（P0 实测逻辑，防改坏）----

    #[test]
    fn normalize_url_empty_or_blank_returns_empty() {
        assert_eq!(normalize_url(""), "");
        assert_eq!(normalize_url("   "), "");
    }

    #[test]
    fn normalize_url_keeps_explicit_scheme() {
        assert_eq!(normalize_url("https://example.com"), "https://example.com");
        assert_eq!(
            normalize_url("http://example.com:8080"),
            "http://example.com:8080"
        );
        assert_eq!(normalize_url("about:blank"), "about:blank");
        assert_eq!(normalize_url("data:text/html,hi"), "data:text/html,hi");
        // 自定义 scheme（cef 内置 page）
        assert_eq!(normalize_url("shift://settings"), "shift://settings");
    }

    #[test]
    fn normalize_url_bare_domain_gets_https() {
        assert_eq!(normalize_url("example.com"), "https://example.com");
        assert_eq!(
            normalize_url("example.com/path?a=1"),
            "https://example.com/path?a=1"
        );
        assert_eq!(normalize_url("192.168.1.1"), "https://192.168.1.1");
    }

    #[test]
    fn normalize_url_words_go_to_search() {
        assert_eq!(normalize_url("abc"), "https://www.google.com/search?q=abc");
        // 带空格 = 搜索词，不是域名
        assert_eq!(
            normalize_url("shift browser"),
            "https://www.google.com/search?q=shift+browser"
        );
    }

    #[test]
    fn normalize_url_trims_surrounding_whitespace() {
        assert_eq!(normalize_url("  example.com  "), "https://example.com");
    }

    // ---- urlencode：搜索词编码（+ 与百分号编码）----

    #[test]
    fn urlencode_space_becomes_plus() {
        assert_eq!(urlencode("a b c"), "a+b+c");
    }

    #[test]
    fn urlencode_unreserved_pass_through() {
        assert_eq!(urlencode("A-z0-9_.~"), "A-z0-9_.~");
    }

    #[test]
    fn urlencode_other_bytes_percent_encoded() {
        assert_eq!(urlencode("中"), "%E4%B8%AD");
    }
}

wrap_request_handler! {
    struct ShellRequestHandler {
        tab_id: String,
    }

    impl RequestHandler {
        /// 单栈窗口的渲染进程终止取证（2026-10-25 实测：导航到 nodeseek 后
        /// 整个窗口消失、浏览器进程存活、无 panic —— 先让原因可见）。
        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: i32,
            error_string: Option<&CefString>,
        ) {
            log::error!(
                "[cef] 渲染进程终止（单栈）tab={} status={:?} code={} detail={}",
                self.tab_id,
                status,
                error_code,
                error_string.map(|s| s.to_string()).unwrap_or_default()
            );
        }
    }
}

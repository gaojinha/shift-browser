//! 单个 CEF 浏览器实例的封装（对标现有 `webkit2gtk::WebView`）。
//!
//! 线程模型（重要）：
//! - CEF 的 `Browser` 句柄只能在 **UI 线程**使用，本模块所有函数都要求调用方在 UI 线程；
//! - 跨线程通信只走 `Sender<CefEvent>`，事件由 Tauri 侧在主线程消费。
//!
//! 嵌入方式：`WindowInfo::set_as_child(parent_xid, &rect)`，parent 是 GTK 容器的 X11 窗口 ID。
//! 实测（2026-10-20）：CEF 窗口会作为 X11 子窗口挂在指定坐标，尺寸与 rect 一致。

use cef::*;
use std::collections::HashMap;
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use super::cef_ui::{cef_ui, cef_ui_call};
use super::shell::lock_state;

mod browser_inject;
mod console_route;
mod events;
mod ops;

pub use ops::focus;
// eval_js 是 P2 快照/Agent 动作的预留注入口，当前只有接口没有调用方；
// 一旦接入就删掉这行 allow（不是遮羞，是接口先行的过渡标记）。
#[allow(unused_imports)]
pub use ops::eval_js;

pub use events::CefEvent;

/// 已创建的浏览器句柄（按 tab_id 索引）。**句柄方法只能在 CEF UI 线程调用**，
/// 存储本身跨线程安全（F-CEF-24b：thread_local → 全局 Mutex）。
///
/// 用异步创建（`browser_host_create_browser`）时，句柄在
/// `LifeSpanHandler::on_after_created` 里才能拿到，因此统一放这里，
/// 由 Tauri 侧按 tab_id 取用。
static BROWSERS: OnceLock<Mutex<HashMap<String, Browser>>> = OnceLock::new();

/// 浏览器句柄表访问器（HashMap 非 const 构造，走 OnceLock 惰性初始化）
pub fn browsers() -> &'static Mutex<HashMap<String, Browser>> {
    BROWSERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 保活：CEF 在浏览器生命周期内会引用 client，这里持有一份防止提前释放
static CLIENTS_ALIVE: Mutex<Vec<Client>> = Mutex::new(Vec::new());

/// 待应用的初始尺寸（tab_id → (宽, 高)）。
///
/// X11 后端下 `set_as_child()` 的 bounds 不生效（实测创建出来是 0×0），
/// 必须等 `on_after_created` 拿到 `window_handle` 后主动 `XMoveResizeWindow`。
static PENDING_SIZE: OnceLock<Mutex<HashMap<String, (i32, i32)>>> = OnceLock::new();

/// 待应用尺寸表访问器（HashMap 非 const 构造，走 OnceLock 惰性初始化）
fn pending_size() -> &'static Mutex<HashMap<String, (i32, i32)>> {
    PENDING_SIZE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 创建浏览器并嵌入到指定 X11 窗口（**异步**）。
///
/// 为什么用异步而不是 `_sync`：实测 `_sync` 变体下 CEF 不创建 X11 子窗口，
/// `window_handle()` 返回 1，导致后续无法 resize（详见排查记录）。
/// 异步变体与 cefsimple 一致，预计会正常创建子窗口。
///
/// 创建完成后句柄会出现在 [`BROWSERS`] 里；调用方需要轮询或等事件。
/// **必须在 UI 线程调用**。
pub fn create_browser(
    tab_id: String,
    url: &str,
    parent_xid: u64,
    bounds: (i32, i32, i32, i32),
    tx: Sender<(String, CefEvent)>,
) -> bool {
    // F-CEF-24c：`browser_host_create_browser` 是 CEF 调用，统一在 CEF UI 线程执行。
    // 同步拿 bool（调用方在 GTK 主线程，recv 阻塞安全；已在 UI 线程则短路原地跑）。
    let url = url.to_string();
    cef_ui_call(move || create_browser_inner(tab_id, &url, parent_xid, bounds, tx))
}

fn create_browser_inner(
    tab_id: String,
    url: &str,
    parent_xid: u64,
    bounds: (i32, i32, i32, i32),
    tx: Sender<(String, CefEvent)>,
) -> bool {
    {
        let mut m = lock_state(pending_size());
        {
            m.insert(tab_id.clone(), (bounds.2, bounds.3));
        }
    };

    let client: Client = CefTabClient::new(tab_id, tx);

    let rect = Rect {
        x: bounds.0,
        y: bounds.1,
        width: bounds.2,
        height: bounds.3,
    };
    let window_info = WindowInfo::default().set_as_child(parent_xid, &rect);
    let settings = BrowserSettings::default();
    let url = CefString::from(url);

    let mut client = client;
    // 保活一份：CEF 在浏览器存活期间会引用 client
    {
        let mut v = lock_state(&CLIENTS_ALIVE);
        v.push(client.clone())
    };

    let ret = browser_host_create_browser(
        Some(&window_info),
        Some(&mut client),
        Some(&url),
        Some(&settings),
        None,
        None,
    );
    ret == 1
}

/// 通知 CEF 视图尺寸变化（GTK 容器 resize 时调用）
///
/// **不能用 `was_resized()`**——官方文档写明它只在 windowless（OSR）模式下有效
/// （`cef_browser.h`: "This function is only used when window rendering is disabled"）。
///
/// windowed 嵌入下，CEF 子窗口是独立 X11 窗口，**父窗口 resize 不会自动带动它**，
/// 必须自己 `XMoveResizeWindow` 到 `host.window_handle()`。
/// 屏幕/缩放变化再补一次 `notify_screen_info_changed()`。
pub fn resize(browser: &Browser, width: i32, height: i32) {
    let Some(host) = browser.host() else {
        return;
    };
    let handle = host.window_handle();
    // handle <= 1 说明 CEF 没走 X11 后端（会返回 1 这种无效值），无从下手。
    // 正常情况下 app.rs 已强制 --ozone-platform=x11，这里应拿到真实 XID。
    if handle <= 1 {
        log::warn!("[cef] resize 跳过：window_handle={handle}（CEF 可能没走 X11 后端）");
        return;
    }

    let dpy = cef::get_xdisplay();
    if dpy.is_null() {
        log::warn!("[cef] resize 失败：get_xdisplay 返回空");
        return;
    }
    log::info!("[cef] resize -> handle=0x{handle:x} {width}x{height}");

    // 用 Chromium 自己的 X 连接操作，避免跨连接竞态
    unsafe {
        let dpy = dpy as *mut x11::xlib::Display;
        let win = handle as x11::xlib::Window;
        x11::xlib::XMoveResizeWindow(dpy, win, 0, 0, width.max(1) as u32, height.max(1) as u32);
        x11::xlib::XFlush(dpy);
    }

    // 显示器切换 / 缩放变化时让 screenX/Y、devicePixelRatio 正确
    // F-CEF-24c：句柄方法须在 CEF UI 线程（X11 腿留调用线程）
    cef_ui(move || host.notify_screen_info_changed());
}

/// document_start 阶段的注入标记（F-CEF-03 最小集）。
///
/// 供 `CefTabLoadHandler` 与 `ShellContentLoadHandler` 共用——**同一类判定只许一份实现**。
/// 脚本内容由 [`browser_inject::build_inject_script`] 生成（纯函数、可单测）；
/// 本函数只负责"主框架判定 + 塞进 frame"。
/// 只对主框架生效（子 frame 注入会重复且无意义）。
pub fn inject_marker(frame: &mut Frame) -> bool {
    if frame.is_main() != 1 {
        return false;
    }
    let brands = crate::useragent::brands_json_for_engine();
    // F-CEF-11：同一次执行里带上预计算的扩展/隐私/广告拦截脚本（不注入两遍）。
    let extra = crate::extensions::cef_inject::cached_inject_js();
    let script = browser_inject::build_inject_script(&brands, extra.as_deref());
    frame.execute_java_script(
        Some(&CefString::from(script.as_str())),
        Some(&CefString::from("shift://inject")),
        -1,
    );
    true
}

/// 关闭浏览器（异步，`do_close` 生效）
pub fn close(browser: &Browser, force: bool) {
    // F-CEF-24c：`close_browser` 是句柄方法，投递到 CEF UI 线程
    let host = browser.host();
    if let Some(host) = host {
        cef_ui(move || host.close_browser(if force { 1 } else { 0 }));
    }
}

// ============================================================
// Client / Handler 实现
// ============================================================

wrap_client! {
    pub struct CefTabClient {
        tab_id: String,
        tx: Sender<(String, CefEvent)>,
    }

    impl Client {
        fn life_span_handler(&self) -> Option<LifeSpanHandler> {
            Some(CefTabLifeSpanHandler::new(self.tab_id.clone(), self.tx.clone()))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            Some(CefTabLoadHandler::new(self.tab_id.clone(), self.tx.clone()))
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            Some(CefTabDisplayHandler::new(self.tab_id.clone(), self.tx.clone()))
        }

        fn request_handler(&self) -> Option<RequestHandler> {
            Some(CefTabRequestHandler::new(self.tab_id.clone(), self.tx.clone()))
        }
    }
}

wrap_life_span_handler! {
    struct CefTabLifeSpanHandler {
        tab_id: String,
        tx: Sender<(String, CefEvent)>,
    }

    impl LifeSpanHandler {
        fn on_after_created(&self, browser: Option<&mut Browser>) {
            // 异步创建：句柄只在这里拿得到，先登记再发事件
            if let Some(b) = browser {
                if let Some(h) = b.host() {
                    log::info!("[cef] on_after_created: window_handle=0x{:x}", h.window_handle());
                }

                // X11 下 set_as_child 的 bounds 不生效，创建出来是 0×0，
                // 句柄此时才可用，立刻按登记尺寸补一刀
                let pending = { let mut m = lock_state(pending_size()); m.remove(&self.tab_id) };
                if let Some((w, h)) = pending {
                    log::info!("[cef] 应用初始尺寸 {w}x{h}");
                    resize(b, w, h);
                }

                let b = b.clone();
                { let mut m = lock_state(browsers()); m.insert(self.tab_id.clone(), b) };
            }
            let _ = self.tx.send((self.tab_id.clone(), CefEvent::AfterCreated));
        }

        fn do_close(&self, _browser: Option<&mut Browser>) -> i32 {
            // 0 = 让 CEF 走默认关闭流程（先触发 on_before_close）
            0
        }

        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            let _ = self.tx.send((self.tab_id.clone(), CefEvent::BeforeClose));
        }
    }
}

wrap_load_handler! {
    struct CefTabLoadHandler {
        tab_id: String,
        tx: Sender<(String, CefEvent)>,
    }

    impl LoadHandler {
        /// document_start 阶段的 JS 注入口（F-CEF-03 最小集）。
        ///
        /// CEF 侧对 WebKit `UserContentManager` document_start 注入的等价通道：
        /// 只在 **主框架** 首次加载开始时执行一段标记脚本，先验证「CEF 能往页面注入 JS」
        /// 这条链路（后续 F-CEF-08 补 `Google Chrome` brand、广告拦截等全部搭这条通道）。
        /// 回调在 UI 线程，`execute_java_script` 可直接调用；本轮只注入一行，不做重活。
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            let Some(frame) = frame else { return };
            if inject_marker(frame) {
                log::info!("[cef] 注入通道 document_start 标记已执行 tab={}", self.tab_id);
            }
        }

        // CEF 回调参数名跟随 C API 风格（camelCase），不能改（签名对齐），scoped 放行
        #[allow(non_snake_case)]
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            isLoading: i32,
            _canGoBack: i32,
            _canGoForward: i32,
        ) {
            if isLoading != 0 {
                let _ = self.tx.send((self.tab_id.clone(), CefEvent::LoadStart(String::new())));
            } else {
                let _ = self.tx.send((self.tab_id.clone(), CefEvent::LoadEnd(String::new())));
            }
        }

        fn on_load_error(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            error_code: Errorcode,
            error_text: Option<&CefString>,
            failed_url: Option<&CefString>,
        ) {
            let _ = self.tx.send((self.tab_id.clone(), CefEvent::LoadError {
                url: failed_url.map(|s| s.to_string()).unwrap_or_default(),
                code: *error_code.as_ref() as i32,
                text: error_text.map(|s| s.to_string()).unwrap_or_default(),
            }));
        }
    }
}

wrap_display_handler! {
    struct CefTabDisplayHandler {
        tab_id: String,
        tx: Sender<(String, CefEvent)>,
    }

    impl DisplayHandler {
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            let _ = self.tx.send((self.tab_id.clone(), CefEvent::TitleChanged(
                title.map(|s| s.to_string()).unwrap_or_default(),
            )));
        }

        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            let _ = self.tx.send((self.tab_id.clone(), CefEvent::UrlChanged(
                url.map(|s| s.to_string()).unwrap_or_default(),
            )));
        }

        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: i32,
        ) -> i32 {
            let raw = message.map(|s| s.to_string()).unwrap_or_default();
            // P1-5：eval 前缀路由在 console_route.rs（同一类判定只许一份实现），
            // 命中走 EvalResult、不命中走 Console；只转发不阻塞（UI 线程铁律）。
            let ev = console_route::route_console_message(&format!("{level:?}"), &raw);
            let _ = self.tx.send((self.tab_id.clone(), ev));
            0
        }
    }
}

wrap_request_handler! {
    struct CefTabRequestHandler {
        tab_id: String,
        tx: Sender<(String, CefEvent)>,
    }

    impl RequestHandler {
        /// 渲染进程终止（崩溃 / OOM / 被系统杀掉）。
        ///
        /// CEF 不会自动重建渲染进程，用户侧表现是标签白屏或整个 BrowserView 消失。
        /// 2026-10-25 实测：CDP 导航到 nodeseek.com 后单栈窗口整个消失、浏览器进程仍存活、
        /// 日志无 panic —— 加这条回调是为了让"谁杀的、什么原因"留下证据（M2/M4 崩溃取证）。
        fn on_render_process_terminated(
            &self,
            _browser: Option<&mut Browser>,
            status: TerminationStatus,
            error_code: i32,
            error_string: Option<&CefString>,
        ) {
            let detail = error_string.map(|s| s.to_string()).unwrap_or_default();
            log::error!(
                "[cef] 渲染进程终止 tab={} status={:?} code={} detail={}",
                self.tab_id,
                status,
                error_code,
                detail
            );
            let _ = self.tx.send((
                self.tab_id.clone(),
                CefEvent::LoadError {
                    url: String::new(),
                    code: error_code,
                    text: format!("渲染进程终止({status:?}): {detail}"),
                },
            ));
        }
    }
}

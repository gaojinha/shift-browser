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
use std::sync::atomic::{AtomicI32, Ordering};

use super::browser::BROWSERS;

/// 内容页的固定 tab id（导航命令按这个取句柄）
pub const CONTENT_ID: &str = "shell-content";
/// 工具栏页的固定 tab id
pub const TOOLBAR_ID: &str = "shell-toolbar";
/// 工具栏高度（像素）= 标签栏 40 + 地址栏 48 + 书签栏 34（2026-10-26 起标签栏并入
/// 同一 WebUI，省一层 BrowserView；标签栏 DOM 由 `window.__shiftTabs()` 刷新）。
/// 书签栏开关（F-CEF-09，Ctrl+Shift+B）两态：展开 122 / 收起 88（书签栏占 34px）。
/// 当前态放进程内存原子 `TOOLBAR_H`，不持久化，默认展开。
pub const TOOLBAR_HEIGHT_EXPANDED: i32 = 122;
pub const TOOLBAR_HEIGHT_COLLAPSED: i32 = 88;

static TOOLBAR_H: AtomicI32 = AtomicI32::new(122);

/// 当前应使用的工具栏高度（书签栏展开或收起，F-CEF-09）。
fn toolbar_height() -> i32 {
    TOOLBAR_H.load(Ordering::Relaxed)
}

/// 一个标签页：视图 + 标题（浏览器句柄另存在 `BROWSERS`，按 `id` 取）
struct ShellTab {
    id: String,
    view: View,
    title: String,
    /// 最近一次地址（on_address_change 维护），关闭时进恢复栈
    url: String,
}

thread_local! {
    /// Client 保活：CEF 在浏览器存活期间会引用 client，提前释放会崩
    static SHELL_CLIENTS: RefCell<Vec<Client>> = const { RefCell::new(Vec::new()) };

    /// 当前单栈窗口句柄（窗口创建回调里登记，供延迟 show 用）
    static SHELL_WINDOW: RefCell<Option<Window>> = const { RefCell::new(None) };

    /// 窗口的 BoxLayout（新建标签时要往里 add_child_view + set_flex）
    static SHELL_LAYOUT: RefCell<Option<BoxLayout>> = const { RefCell::new(None) };

    /// 工具栏 View 句柄（窗口创建回调里登记，供书签栏开关 set_bounds 用，F-CEF-09）
    static SHELL_TOOLBAR_VIEW: RefCell<Option<View>> = const { RefCell::new(None) };

    /// 标签表（顺序即标签栏顺序）与当前活动索引
    static SHELL_TABS: RefCell<Vec<ShellTab>> = const { RefCell::new(Vec::new()) };
    static ACTIVE_TAB: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// 新标签 id 自增序号
    static TAB_SEQ: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// 工具栏/启动页所在目录（open 时从 toolbar_url 反推，供新建标签取启动页）
    static SHELL_CACHE_DIR: RefCell<Option<String>> = const { RefCell::new(None) };

    /// 最近关闭标签的 URL 栈（上限 20），供 Ctrl+Shift+T 恢复
    static CLOSED_URLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };

    /// 正在关闭某个标签：期间拦掉 CEF 顺带发起的窗口关闭请求
    static TAB_CLOSING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// 书签（title, url），由 cef_cmd 从主应用数据库读入
    static BOOKMARKS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

/// 打开单栈窗口。**必须在 CEF UI 线程调用。**
///
/// `content_url`：初始网页；`toolbar_url`：本地工具栏页（file:// 或 data:）
pub fn open(content_url: &str, toolbar_url: &str) -> Result<(), String> {
    // 记下 shell 目录（启动页和工具栏同目录），供 Ctrl+T 新建标签时取
    if let Some(p) = toolbar_url.strip_prefix("file://") {
        if let Some(dir) = std::path::Path::new(p).parent() {
            SHELL_CACHE_DIR.with(|d| *d.borrow_mut() = Some(dir.to_string_lossy().into_owned()));
        }
    }

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
    let mut toolbar_delegate = ShellViewDelegate::new(RuntimeStyle::ALLOY, TOOLBAR_HEIGHT_EXPANDED);

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

    // 首个标签登记进标签表（视图所有权仍在 ShellWindowDelegate，这里持一份引用）
    let first_view = View::from(&content_view);
    SHELL_TABS.with(|t| {
        t.borrow_mut().push(ShellTab {
            id: CONTENT_ID.to_string(),
            view: first_view,
            title: "新标签页".to_string(),
            url: content_url.to_string(),
        });
    });
    ACTIVE_TAB.with(|a| a.set(0));

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

                        // 去掉系统标题栏（2026-10-26）：Chrome 的窗口按钮画在标签栏里，
                        // 没有独立标题栏。_MOTIF_WM_HINTS = { flags=2(装饰), decorations=0 }
                        {
                            let atom = x11::xlib::XInternAtom(
                                dpy,
                                c"_MOTIF_WM_HINTS".as_ptr(),
                                0,
                            );
                            // X11 的 32 位属性在 64 位平台上用 long 数组承载（只读低 32 位）
                            let hints: [std::os::raw::c_long; 5] = [2, 0, 0, 0, 0];
                            x11::xlib::XChangeProperty(
                                dpy,
                                win,
                                atom,
                                atom, // 类型也用 motif atom（用 XA_ATOM 会被 xprop 按 atom 名解析）
                                32,
                                0, // PropModeReplace
                                hints.as_ptr() as *const u8,
                                5,
                            );
                            // 只写属性；unmap/remap 放到 force_show 里做——
                            // 在 on_window_created 里做会等 WM 响应而阻塞，
                            // 导致 open() 不返回、后续 force_show 与书签推送全不执行。
                            x11::xlib::XFlush(dpy);
                        }

                        // 无装饰窗口要显式声明「接受键盘焦点」，否则 Mutter 会把
                        // windowfocus 请求转给别的窗口（实测焦点被踢回 0x600003）。
                        {
                            let mut wh = x11::xlib::XWMHints {
                                flags: x11::xlib::InputHint,
                                input: 1,
                                ..std::mem::zeroed()
                            };
                            x11::xlib::XSetWMHints(dpy, win, &mut wh);
                        }

                        // 去掉系统装饰需要 remap 一次（属性已在 on_window_created 写好）
                        x11::xlib::XUnmapWindow(dpy, win);
                        x11::xlib::XMapRaised(dpy, win);
                        x11::xlib::XFlush(dpy);

                        // 窗口映射后补推一次书签与标签栏：cef_cmd 的数据库查询
                        // 可能在窗口创建之前就完成了，那一次 execute_java_script
                        // 会打到还不存在的页面上（实测：库里 64 条，UI 仍是静态兜底 7 条）。
                        // 推三轮：工具栏页面（file://）加载时机不定，单次推送容易
                        // 打在脚本还没就绪的窗口期（实测 800ms 一次时 __shiftBookmarks
                        // 已存在但 UI 未更新，说明推送早于脚本执行）。
                        for delay in [800u64, 2000, 4000] {
                            gtk::glib::timeout_add_local_once(
                                std::time::Duration::from_millis(delay),
                                || {
                                    refresh_bookmarks_ui();
                                    refresh_tabbar();
                                },
                            );
                        }
                    }
                    log::info!("[cef] XSetClassHint + XMapRaised 已调用");
                }
            }
        } else {
            log::warn!("[cef] force_show：没有窗口句柄");
        }
    });
}

/// 窗口控制：最小化 / 最大化切换 / 关闭（标签栏右侧按钮走这里）。
pub fn window_minimize() {
    SHELL_WINDOW.with(|w| {
        if let Some(win) = w.borrow().as_ref() {
            win.minimize();
        }
    });
}

pub fn window_toggle_maximize() {
    SHELL_WINDOW.with(|w| {
        if let Some(win) = w.borrow().as_ref() {
            // CEF 没有 is_maximized 查询，用窗口尺寸对比近似的做法不可靠；
            // 这里统一走 maximize()，连续点两下由 WM 自己处理（多数 WM 会 toggle）。
            win.maximize();
        }
    });
}

pub fn window_close() {
    SHELL_WINDOW.with(|w| {
        if let Some(win) = w.borrow().as_ref() {
            win.close();
        }
    });
}

/// 拖动窗口：发 `_NET_WM_MOVERESIZE` 给窗口管理器，由 WM 接管拖动
/// （比自己在 Rust 里跟鼠标稳，GTK/Qt 都这么做）。
pub fn window_begin_drag() {
    let handle = SHELL_WINDOW.with(|w| w.borrow().as_ref().map(|x| x.window_handle()));
    let Some(handle) = handle else { return };
    if handle <= 1 {
        return;
    }
    let dpy = cef::get_xdisplay();
    if dpy.is_null() {
        return;
    }
    unsafe {
        let dpy = dpy as *mut x11::xlib::Display;
        let win = handle as x11::xlib::Window;
        let root = x11::xlib::XDefaultRootWindow(dpy);

        // 取当前指针位置（_NET_WM_MOVERESIZE 要求 root 坐标）
        let mut rx = 0;
        let mut ry = 0;
        let mut wx = 0;
        let mut wy = 0;
        let mut mask = 0;
        let mut child: x11::xlib::Window = 0;
        x11::xlib::XQueryPointer(
            dpy, root, &mut root.clone(), &mut child, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask,
        );

        let atom = x11::xlib::XInternAtom(
            dpy,
            c"_NET_WM_MOVERESIZE".as_ptr(),
            0,
        );
        let mut ev = x11::xlib::XEvent {
            client_message: x11::xlib::XClientMessageEvent {
                type_: x11::xlib::ClientMessage,
                serial: 0,
                send_event: 1,
                display: dpy,
                window: win,
                message_type: atom,
                format: 32,
                data: x11::xlib::ClientMessageData::from([rx as i64, ry as i64, 8, 1, 1]),
            },
        };
        x11::xlib::XSendEvent(
            dpy,
            root,
            0,
            (1 << 20) | (1 << 19), // SubstructureRedirectMask | SubstructureNotifyMask
            &mut ev,
        );
        x11::xlib::XFlush(dpy);
    }
    log::info!("[cef] 已请求窗口管理器接管拖动");
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
            // 注意：width 给 0 会让内容区渲染空白（2026-10-26 实测），
            // 这里维持 100；多标签下隐藏标签仍参与宽度分配（活动标签被挤成半屏）
            // 是已知问题，待用 set_bounds 手动布局解决，见 WORKBOARD。
            // F-CEF-09：preferred_height 初值 0 是内容视图、非 0 是工具栏视图；
            // 工具栏高度改读 toolbar_height()（书签栏状态），这样窗口 resize 触发
            // BoxLayout 重算时结果仍然正确，不会被回写回 122。
            let height = if self.preferred_height == 0 {
                0
            } else {
                toolbar_height()
            };
            Size {
                width: 100,
                height,
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
        // 窗口按钮（2026-10-26）：不显式声明时 CEF 的 Linux 顶层窗口只给
        // 最小化 + 关闭，没有最大化。三个能力都返回 1，交给窗口管理器画齐。
        fn can_maximize(&self, _window: Option<&mut Window>) -> std::os::raw::c_int {
            1
        }

        fn can_minimize(&self, _window: Option<&mut Window>) -> std::os::raw::c_int {
            1
        }

        fn can_resize(&self, _window: Option<&mut Window>) -> std::os::raw::c_int {
            1
        }


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
            // 工具栏 View 句柄留一份：书签栏开关要 set_bounds 改它的高度（F-CEF-09）
            SHELL_TOOLBAR_VIEW.with(|v| {
                *v.borrow_mut() = Some(toolbar_view.clone());
            });

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
                // 存一份给新建标签用（add_child_view 后要补 flex）
                SHELL_LAYOUT.with(|l| *l.borrow_mut() = Some(layout));
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
            // 关闭标签时 CEF 会顺带请求关窗（实测 close_browser 的 force 0/1 都一样，
            // 表现为整个窗口消失）。这里拦下；用户真点窗口关闭按钮时标志为 false，正常放行。
            if TAB_CLOSING.with(|f| f.get()) {
                0
            } else {
                1
            }
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

        fn load_handler(&self) -> Option<LoadHandler> {
            // 内容页需要加载态（标签栏转圈）；工具栏页不需要
            if self.toolbar {
                None
            } else {
                Some(ShellContentLoadHandler::new(self.tab_id.clone()))
            }
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(ShellDownloadHandler::new())
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            // 快捷键在工具栏和内容页都要生效（Ctrl+T 在地址栏里按也该开新标签）
            Some(ShellKeyboardHandler::new())
        }

        fn display_handler(&self) -> Option<DisplayHandler> {
            // 工具栏：接收 JS 传上来的动作（console 桥）
            // 内容页：接收标题变化，回写标签栏（2026-10-26 多标签）
            if self.toolbar {
                Some(ShellToolbarDisplay::new())
            } else {
                Some(ShellContentDisplay::new(self.tab_id.clone()))
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

// 内容页 → 标签栏：标题变化回写。
//
// CEF 的 on_title_change 每页只回调一次（首次拿到标题时），
// 后续 SPA 内改标题需要 LoadEnd 后补一次读取（待 M2 完善）。
wrap_display_handler! {
    struct ShellContentDisplay {
        tab_id: String,
    }

    impl DisplayHandler {
        /// 地址变化（含重定向）→ 同步到地址栏输入框。
        /// 2026-10-26：多标签下只有活动标签的地址才该显示在地址栏。
        fn on_address_change(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            url: Option<&CefString>,
        ) {
            let Some(u) = url else { return };
            let active_id = SHELL_TABS.with(|t| {
                let tabs = t.borrow();
                tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|x| x.id.clone())
            });
            if active_id.as_deref() != Some(self.tab_id.as_str()) {
                return;
            }
            let text = u.to_string();
            SHELL_TABS.with(|tabs| {
                let mut tabs = tabs.borrow_mut();
                if let Some(tab) = tabs.iter_mut().find(|x| x.id == self.tab_id) {
                    tab.url = text.clone();
                }
            });
            run_toolbar_js(&format!(
                "if (window.__shiftUrl) {{ window.__shiftUrl({}); }}",
                serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".to_string())
            ));
        }

        fn on_title_change(
            &self,
            _browser: Option<&mut Browser>,
            title: Option<&CefString>,
        ) {
            let Some(t) = title else { return };
            let text = t.to_string();
            if text.is_empty() {
                return;
            }
            SHELL_TABS.with(|tabs| {
                let mut tabs = tabs.borrow_mut();
                if let Some(tab) = tabs.iter_mut().find(|x| x.id == self.tab_id) {
                    tab.title = text;
                }
            });
            refresh_tabbar();
            // 书签补推：glib 定时器在 CEF 回调链里注册后实测不触发，
            // 改挂在这个确定会执行的回调上（原子标记保证只推一次）。
            static BOOKMARKS_PUSHED: std::sync::atomic::AtomicBool =
                std::sync::atomic::AtomicBool::new(false);
            if !BOOKMARKS_PUSHED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                refresh_bookmarks_ui();
            }
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

/// 启动页 URL（CEF 自包含版本；拿不到目录时回退 about:blank）
fn newtab_url() -> String {
    SHELL_CACHE_DIR.with(|d| {
        if let Some(dir) = d.borrow().as_ref() {
            if let Ok(url) = super::newtab::ensure_newtab_html(std::path::Path::new(dir)) {
                return url;
            }
        }
        "about:blank".to_string()
    })
}

/// 新建标签页（视图隐藏加入布局，随后切换过去）。
fn new_tab(url: &str) -> Result<(), String> {
    let seq = TAB_SEQ.with(|c| {
        let v = c.get() + 1;
        c.set(v);
        v
    });
    let id = format!("shell-tab-{seq}");

    let client: Client = ShellClient::new(id.clone(), false);
    SHELL_CLIENTS.with(|v| v.borrow_mut().push(client.clone()));
    let mut client = client;
    let mut delegate = ShellViewDelegate::new(RuntimeStyle::ALLOY, 0);
    let browser_view = browser_view_create(
        Some(&mut client),
        Some(&CefString::from(url)),
        Some(&BrowserSettings::default()),
        None,
        None,
        Some(&mut delegate),
    )
    .ok_or_else(|| "新标签 BrowserView 创建失败".to_string())?;

    let mut view = View::from(&browser_view);
    view.set_visible(0);

    let mut attached = false;
    SHELL_WINDOW.with(|w| {
        if let Some(window) = w.borrow().as_ref() {
            window.add_child_view(Some(&mut view));
            attached = true;
        }
    });
    if !attached {
        return Err("窗口尚未创建，无法加视图".to_string());
    }
    SHELL_LAYOUT.with(|l| {
        if let Some(layout) = l.borrow().as_ref() {
            layout.set_flex_for_view(Some(&mut view), 1);
        }
    });
    // 新视图加进来后必须让 BoxLayout 重算一次，否则它的 bounds 停在
    // 创建时的默认尺寸（实测约 630px 宽，右侧一片空白）——同 on_window_created
    // 里那次 window.layout() 的理由。
    SHELL_WINDOW.with(|w| {
        if let Some(window) = w.borrow().as_ref() {
            window.layout();
        }
    });

    let idx = SHELL_TABS.with(|t| {
        let mut tabs = t.borrow_mut();
        tabs.push(ShellTab {
            id: id.clone(),
            view,
            title: "新标签页".to_string(),
            url: url.to_string(),
        });
        tabs.len() - 1
    });

    log::info!("[cef] 新标签已创建: {id} (idx={idx})");
    switch_tab(idx);
    refresh_tabbar();
    Ok(())
}

/// 切换活动标签（只改可见性，不动布局）
fn switch_tab(idx: usize) {
    let len = SHELL_TABS.with(|t| t.borrow().len());
    if idx >= len {
        log::warn!("[cef] 切换标签越界: {idx} >= {len}");
        return;
    }
    SHELL_TABS.with(|t| {
        let mut tabs = t.borrow_mut();
        for (i, tab) in tabs.iter_mut().enumerate() {
            if i == idx {
                tab.view.set_visible(1);
                let v = tab.view.clone();
                v.request_focus();
            } else {
                tab.view.set_visible(0);
                // 关键：BoxLayout 按 flex 分配，隐藏视图仍占一份宽度（实测活动标签
                // 只剩半屏）。把非活动视图的 bounds 压成 0，逼它不占位。
                tab.view.set_bounds(Some(&Rect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                }));
            }
        }
    });
    ACTIVE_TAB.with(|a| a.set(idx));
    // 切标签后活动视图换了，按当前书签栏态重排一次（收起态下内容视图 y 仍是新高度）
    apply_toolbar_height();
    // View 层 request_focus ≠ 网页能收键盘：CEF 的 browser 还要单独 set_focus(1)。
    // 2026-10-26 实测：只 request_focus 时 document.hasFocus() 一直 false。
    let active_id = SHELL_TABS.with(|t| t.borrow().get(idx).map(|x| x.id.clone()));
    if let Some(id) = active_id {
        BROWSERS.with(|m| {
            if let Some(b) = m.borrow().get(&id) {
                super::browser::focus(b);
            }
        });
    }
    log::info!("[cef] 切换到标签 {idx}");
    refresh_tabbar();
}

/// 关闭标签（至少保留一个；关闭后切到相邻标签）
fn close_tab(idx: usize) {
    let len = SHELL_TABS.with(|t| t.borrow().len());
    if idx >= len {
        return;
    }
    if len <= 1 {
        log::info!("[cef] 只剩一个标签，忽略关闭");
        return;
    }
    // 先记下这个标签的 URL（供 Ctrl+Shift+T 恢复），再摘表
    let closing_url = SHELL_TABS.with(|t| {
        t.borrow()
            .get(idx)
            .map(|x| x.url.clone())
            .unwrap_or_default()
    });
    if !closing_url.is_empty() && closing_url != "about:blank" {
        CLOSED_URLS.with(|v| {
            let mut v = v.borrow_mut();
            v.push(closing_url);
            if v.len() > 20 {
                v.remove(0);
            }
        });
    }

    let (id, view) = SHELL_TABS.with(|t| {
        let mut tabs = t.borrow_mut();
        let tab = tabs.remove(idx);
        (tab.id, tab.view)
    });
    // 关闭期间置标志，让 WindowDelegate::can_close 拦下 CEF 顺带的关窗请求；
    // close_browser 是异步的，200ms 后（且它已经发完关窗请求）再复位。
    TAB_CLOSING.with(|f| f.set(true));
    gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(200), || {
        TAB_CLOSING.with(|f| f.set(false));
    });
    BROWSERS.with(|m| {
        if let Some(b) = m.borrow().get(&id) {
            if let Some(h) = b.host() {
                log::info!("[cef] 关闭标签 {id}：close_browser(0)");
                h.close_browser(0);
            }
        }
    });
    // 不调 remove_child_view：2026-10-26 实测它会把整个窗口一起销毁
    // （点 × 后 8 个进程还在但窗口消失）。改为只隐藏，视图随 browser 关闭由 CEF 回收；
    // 代价是短时间内留一个不可见视图，先保证不崩，M2 迭代里再找安全的移除时机。
    view.set_visible(0);

    let new_idx = idx.saturating_sub(1).min(len.saturating_sub(2));
    ACTIVE_TAB.with(|a| a.set(new_idx));
    switch_tab(new_idx);
    log::info!("[cef] 标签已关闭: {id}，切到 {new_idx}");
}

/// 由 `cef_cmd` 在主线程之外查到书签后调用：存起来并派回主线程刷新 UI。
pub fn set_bookmarks(rows: Vec<(String, String)>) {
    log::info!("[cef] 书签已加载 {} 条", rows.len());
    BOOKMARKS.with(|b| *b.borrow_mut() = rows);
    gtk::glib::idle_add_local_once(refresh_bookmarks_ui);
}

/// 把书签表推给工具栏页面的书签栏（调 `window.__shiftBookmarks`）。
pub fn refresh_bookmarks_ui() {
    let json = BOOKMARKS.with(|b| {
        let list: Vec<serde_json::Value> = b
            .borrow()
            .iter()
            .map(|(t, u)| serde_json::json!({ "title": t, "url": u }))
            .collect();
        serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
    });
    let has_toolbar = BROWSERS.with(|m| m.borrow().contains_key(TOOLBAR_ID));
    log::info!(
        "[cef] 推书签 {} 条（toolbar browser 已注册={has_toolbar}）",
        json.matches("\"url\"").count()
    );
    run_toolbar_js(&format!(
        "if (window.__shiftBookmarks) {{ window.__shiftBookmarks({json}); }}"
    ));
}

/// 在工具栏页面里执行一段 JS（地址栏/加载态等 UI 更新统一走这里）
fn run_toolbar_js(code: &str) {
    BROWSERS.with(|m| {
        if let Some(b) = m.borrow().get(TOOLBAR_ID) {
            if let Some(frame) = b.main_frame() {
                frame.execute_java_script(
                    Some(&CefString::from(code)),
                    Some(&CefString::from("shift://ui")),
                    -1,
                );
            }
        }
    });
}

/// 书签栏开关（F-CEF-09，Ctrl+Shift+B）：翻转高度态（122↔88）→ 同步 DOM 显隐
/// → 触发 BoxLayout 重算（preferred_size 已改读 toolbar_height()，内容区自动吃满
/// 让出的 34px）。状态只存进程内存，默认展开。
fn toggle_bookmarks() {
    let next = if toolbar_height() == TOOLBAR_HEIGHT_COLLAPSED {
        TOOLBAR_HEIGHT_EXPANDED
    } else {
        TOOLBAR_HEIGHT_COLLAPSED
    };
    let visible = next == TOOLBAR_HEIGHT_EXPANDED;
    TOOLBAR_H.store(next, Ordering::Relaxed);
    run_toolbar_js(&format!(
        "window.__shiftBookmarksVisible && window.__shiftBookmarksVisible({});",
        visible
    ));
    apply_toolbar_height();
    log::info!("[cef] 书签栏切换: height={} visible={}", next, visible);
}

/// 按当前书签栏态手动重算「工具栏 + 活动内容视图」的 bounds（F-CEF-09）。
/// 为什么不用 window.layout()：BoxLayout 缓存了工具栏的首选尺寸（实测改原子后再
/// layout() 工具栏仍停 122），只能像 switch_tab 压 hidden 视图那样直接 set_bounds。
/// 非活动标签视图已被 switch_tab 压成 0，这里只搬工具栏与活动标签两块。
fn apply_toolbar_height() {
    let h = toolbar_height();
    let (win_w, win_h) = SHELL_WINDOW.with(|w| {
        w.borrow()
            .as_ref()
            .map(|w| (w.bounds().width, w.bounds().height))
            .unwrap_or((0, 0))
    });
    if (win_w, win_h) == (0, 0) {
        return;
    }
    if let Some(tb) = SHELL_TOOLBAR_VIEW.with(|v| v.borrow().clone()) {
        tb.set_bounds(Some(&Rect {
            x: 0,
            y: 0,
            width: win_w,
            height: h,
        }));
    }
    let active = ACTIVE_TAB.with(|a| a.get());
    SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        if let Some(tab) = tabs.get(active) {
            tab.view.set_bounds(Some(&Rect {
                x: 0,
                y: h,
                width: win_w,
                height: win_h - h,
            }));
        }
    });
}

/// 关闭除 `keep` 之外的所有标签（右键菜单「关闭其他」）。
/// 从大到小关，避免索引在删除过程中错位。
fn close_other_tabs(keep: usize) {
    let total = SHELL_TABS.with(|t| t.borrow().len());
    if total <= 1 {
        return;
    }
    for idx in (0..total).rev() {
        if idx != keep && idx < SHELL_TABS.with(|t| t.borrow().len()) {
            close_tab(idx);
        }
    }
    // 关完之后保留下来的那个可能已换位，切到它
    let cur = keep.min(SHELL_TABS.with(|t| t.borrow().len()).saturating_sub(1));
    switch_tab(cur);
    refresh_tabbar();
}

/// 关闭 `from` 右侧的所有标签（右键菜单「关闭右侧」）。
fn close_tabs_right_of(from: usize) {
    let total = SHELL_TABS.with(|t| t.borrow().len());
    if from + 1 >= total {
        return;
    }
    for idx in (from + 1..total).rev() {
        if idx < SHELL_TABS.with(|t| t.borrow().len()) {
            close_tab(idx);
        }
    }
    switch_tab(from);
    refresh_tabbar();
}

/// 把标签表同步到工具栏页面的标签栏（调 `window.__shiftTabs(tabs, active)`）
pub fn refresh_tabbar() {
    let (list_json, active) = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        let list: Vec<serde_json::Value> = tabs
            .iter()
            .map(|tab| serde_json::json!({ "title": tab.title, "id": tab.id }))
            .collect();
        (
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string()),
            ACTIVE_TAB.with(|a| a.get()),
        )
    });
    let code = format!(
        "if (window.__shiftTabs) {{ window.__shiftTabs({}, {}); }}",
        list_json, active
    );
    BROWSERS.with(|m| {
        if let Some(b) = m.borrow().get(TOOLBAR_ID) {
            if let Some(frame) = b.main_frame() {
                frame.execute_java_script(
                    Some(&CefString::from(code.as_str())),
                    Some(&CefString::from("shift://tabs")),
                    -1,
                );
            }
        }
    });
}

/// 执行工具栏动作
fn handle_action(action: &str) {
    let (cmd, arg) = match action.split_once('|') {
        Some((c, a)) => (c, a),
        None => (action, ""),
    };

    // 标签相关动作先处理（不需要当前 content 浏览器）
    match cmd {
        "tabnew" => {
            let url = newtab_url();
            if let Err(e) = new_tab(&url) {
                log::error!("[cef] 新建标签失败: {e}");
            }
            return;
        }
        "tab" => {
            if let Ok(idx) = arg.parse::<usize>() {
                switch_tab(idx);
            }
            return;
        }
        "tabclose" => {
            if let Ok(idx) = arg.parse::<usize>() {
                close_tab(idx);
            }
            return;
        }
        "tabcloseothers" => {
            if let Ok(idx) = arg.parse::<usize>() {
                close_other_tabs(idx);
            }
            return;
        }
        "needbookmarks" => {
            log::info!("[cef] 工具栏请求书签");
            refresh_bookmarks_ui();
            return;
        }
        "winmin" => {
            window_minimize();
            return;
        }
        "winmax" => {
            window_toggle_maximize();
            return;
        }
        "winclose" => {
            window_close();
            return;
        }
        "windrag" => {
            window_begin_drag();
            return;
        }
        "tabcloseright" => {
            if let Ok(idx) = arg.parse::<usize>() {
                close_tabs_right_of(idx);
            }
            return;
        }
        _ => {}
    }

    let active_id = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        let idx = ACTIVE_TAB.with(|a| a.get());
        tabs.get(idx).map(|tab| tab.id.clone())
    });
    let Some(active_id) = active_id else {
        log::warn!("[cef] 无活动标签，忽略动作 {cmd}");
        return;
    };

    BROWSERS.with(|m| {
        let map = m.borrow();
        let Some(browser) = map.get(&active_id) else {
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

wrap_load_handler! {
    struct ShellContentLoadHandler {
        tab_id: String,
    }

    impl LoadHandler {
        /// document_start 注入标记（F-CEF-03）：CEF 独立窗口走的是 ShellClient，
        /// 与 CefTabClient 是两条路径，注入点必须各挂一份（共用 `inject_marker`）。
        fn on_load_start(
            &self,
            _browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _transition_type: TransitionType,
        ) {
            let Some(frame) = frame else { return };
            if super::browser::inject_marker(frame) {
                log::info!(
                    "[cef] 注入通道 document_start 标记已执行（shell）tab={}",
                    self.tab_id
                );
            }
        }

        // CEF 回调参数名跟随 C API 风格（camelCase），不改签名
        #[allow(non_snake_case)]
        fn on_loading_state_change(
            &self,
            _browser: Option<&mut Browser>,
            isLoading: i32,
            _canGoBack: i32,
            _canGoForward: i32,
        ) {
            // 多标签下只有活动标签影响 UI，否则后台标签加载会闪活动标签的图标
            let active_id = SHELL_TABS.with(|t| {
                let tabs = t.borrow();
                tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|x| x.id.clone())
            });
            if active_id.as_deref() != Some(self.tab_id.as_str()) {
                return;
            }
            run_toolbar_js(&format!(
                "if (window.__shiftLoading) {{ window.__shiftLoading({}); }}",
                isLoading != 0
            ));
        }
    }
}

wrap_keyboard_handler! {
    struct ShellKeyboardHandler;

    impl KeyboardHandler {
        fn on_pre_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut cef::sys::XEvent>,
            is_keyboard_shortcut: Option<&mut std::os::raw::c_int>,
        ) -> std::os::raw::c_int {
            handle_shortcut(event, is_keyboard_shortcut)
        }

        fn on_key_event(
            &self,
            _browser: Option<&mut Browser>,
            event: Option<&KeyEvent>,
            _os_event: Option<&mut cef::sys::XEvent>,
        ) -> std::os::raw::c_int {
            handle_shortcut(event, None)
        }
    }
}

/// 处理 Ctrl 系快捷键。返回 1 = 已消费，不再传给页面。
fn handle_shortcut(
    event: Option<&KeyEvent>,
    is_shortcut: Option<&mut std::os::raw::c_int>,
) -> std::os::raw::c_int {
    let Some(ev) = event else { return 0 };
    let t = ev.type_.get_raw();
    // 只处理按下，避免 keyup 重复触发
    if t != KeyEventType::RAWKEYDOWN.get_raw() && t != KeyEventType::KEYDOWN.get_raw() {
        return 0;
    }
    // CEF EVENTFLAG_CONTROL_DOWN = 1 << 2 / EVENTFLAG_SHIFT_DOWN = 1 << 1
    let ctrl = ev.modifiers & (1 << 2) != 0;
    let shift = ev.modifiers & (1 << 1) != 0;
    if !ctrl {
        return 0;
    }
    // Ctrl+Shift+T 恢复最近关闭的标签
    if shift && ev.windows_key_code == 84 {
        let url = CLOSED_URLS.with(|v| v.borrow_mut().pop());
        if let Some(u) = url {
            if let Err(e) = new_tab(&u) {
                log::error!("[cef] 恢复标签失败: {e}");
            }
        }
        if let Some(flag) = is_shortcut {
            *flag = 1;
        }
        return 1;
    }
    // Ctrl+Shift+B 切换书签栏（F-CEF-09；B = 0x42 = 66）
    if shift && ev.windows_key_code == 66 {
        toggle_bookmarks();
        if let Some(flag) = is_shortcut {
            *flag = 1;
        }
        return 1;
    }
    log::info!(
        "[cef] Ctrl 快捷键候选: key={} type={:?} mods={}",
        ev.windows_key_code,
        ev.type_.get_raw(),
        ev.modifiers
    );
    let consumed = match ev.windows_key_code {
        84 => {
            // Ctrl+T 新标签
            let url = newtab_url();
            if let Err(e) = new_tab(&url) {
                log::error!("[cef] Ctrl+T 新建标签失败: {e}");
            }
            1
        }
        87 => {
            // Ctrl+W 关标签
            let idx = ACTIVE_TAB.with(|a| a.get());
            close_tab(idx);
            1
        }
        9 => {
            // Ctrl+Tab 下一个标签
            let len = SHELL_TABS.with(|t| t.borrow().len());
            if len > 1 {
                let cur = ACTIVE_TAB.with(|a| a.get());
                switch_tab((cur + 1) % len);
            }
            1
        }
        _ => 0,
    };
    if consumed == 1 {
        if let Some(flag) = is_shortcut {
            *flag = 1;
        }
    }
    consumed
}

wrap_download_handler! {
    struct ShellDownloadHandler;

    impl DownloadHandler {
        /// 下载开始前决定落盘路径：直接存 `~/Downloads`，不弹保存对话框
        /// （CEF 的 Views 模式没有系统保存对话框，不接管会静默失败）。
        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            _download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> std::os::raw::c_int {
            let name = suggested_name
                .map(|s| s.to_string())
                .unwrap_or_else(|| "download".to_string());
            let safe = sanitize_filename(&name);
            let path = download_dir().join(&safe);
            log::info!("[cef] 下载开始: {name} -> {}", path.display());
            if let Some(cb) = callback {
                cb.cont(Some(&CefString::from(path.to_string_lossy().as_ref())), 0);
            }
            1
        }

        fn on_download_updated(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            _callback: Option<&mut DownloadItemCallback>,
        ) {
            if let Some(it) = download_item {
                if it.is_complete() != 0 {
                    log::info!("[cef] 下载完成（进度 {}%）", it.percent_complete());
                }
            }
        }
    }
}

/// 默认下载目录：`$HOME/Downloads`（不存在则创建）
fn download_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = std::path::PathBuf::from(home).join("Downloads");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// 只保留文件名本身：剥掉路径分隔、控制字符、首尾点（防 `../` 逃逸与隐藏文件）
fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("download");
    let cleaned: String = base.chars().filter(|c| !c.is_control()).collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed
    }
}

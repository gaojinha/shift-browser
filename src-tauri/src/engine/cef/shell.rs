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
        + PW_EXTRA.load(Ordering::Relaxed)
        + BM_EXTRA.load(Ordering::Relaxed)
        + DL_EXTRA.load(Ordering::Relaxed)
        + MENU_EXTRA.load(Ordering::Relaxed)
        + FIND_EXTRA.load(Ordering::Relaxed)
        + HIST_EXTRA.load(Ordering::Relaxed)
        + SITE_EXTRA.load(Ordering::Relaxed)
        + SETTINGS_EXTRA.load(Ordering::Relaxed)
        + BM_MANAGE_EXTRA_V.load(Ordering::Relaxed)
        + CERT_EXTRA.load(Ordering::Relaxed)
        + US_EXTRA.load(Ordering::Relaxed)
}

/// 8 个根类面板的注册表（F-CEF-21）：根 class 与撑高原子的唯一对应。
/// 互斥只认这里：开任一面板 = 其余 7 个的撑高全部归零（`clear_all_extras_except`）。
/// `BM_EXTRA`（书签文件夹下拉，360）不在此表：JS 侧是局部元素 `#bmfolders.open`，
/// Rust 侧是根级撑高，两侧口径本就不对称，F-CEF-21 只如实标注、不硬统一；
/// 它在 `clear_all_extras_except` 里始终被清（开任一根类面板都收下拉），
/// `set_bm_popup` 自身只让位查找栏，保持原行为。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Panel {
    Pw,
    Dl,
    Menu,
    Hist,
    Site,
    Settings,
    BmManage,
    Find,
    Cert,
    Us,
}

impl Panel {
    pub(crate) const ALL: [Panel; 10] = [
        Panel::Pw,
        Panel::Dl,
        Panel::Menu,
        Panel::Hist,
        Panel::Site,
        Panel::Settings,
        Panel::BmManage,
        Panel::Find,
        Panel::Cert,
        Panel::Us,
    ];

    /// 对应 `html.<class>` 根类名（须与 `toolbar::PANEL_CLASSES` 同序同值，有单测钉住）
    pub(crate) const fn class(self) -> &'static str {
        match self {
            Panel::Pw => "pw-open",
            Panel::Dl => "dl-open",
            Panel::Menu => "menu-open",
            Panel::Hist => "hist-open",
            Panel::Site => "site-open",
            Panel::Settings => "settings-open",
            Panel::BmManage => "bmmanage-open",
            Panel::Find => "find-open",
            Panel::Cert => "cert-open",
            Panel::Us => "us-open",
        }
    }

    pub(crate) const fn extra_px(self) -> i32 {
        match self {
            Panel::Pw => PW_PANEL_EXTRA,
            Panel::Dl => DL_PANEL_EXTRA,
            Panel::Menu => MENU_PANEL_EXTRA,
            Panel::Hist => HIST_PANEL_EXTRA,
            Panel::Site => SITE_PANEL_EXTRA,
            Panel::Settings => SETTINGS_PANEL_EXTRA,
            Panel::BmManage => BM_MANAGE_PANEL_EXTRA,
            Panel::Find => FIND_BAR_EXTRA,
            Panel::Cert => CERT_PANEL_EXTRA,
            Panel::Us => US_PANEL_EXTRA,
        }
    }

    fn atom(self) -> &'static AtomicI32 {
        match self {
            Panel::Pw => &PW_EXTRA,
            Panel::Dl => &DL_EXTRA,
            Panel::Menu => &MENU_EXTRA,
            Panel::Hist => &HIST_EXTRA,
            Panel::Site => &SITE_EXTRA,
            Panel::Settings => &SETTINGS_EXTRA,
            Panel::BmManage => &BM_MANAGE_EXTRA_V,
            Panel::Find => &FIND_EXTRA,
            Panel::Cert => &CERT_EXTRA,
            Panel::Us => &US_EXTRA,
        }
    }
}

/// 把除 `keep` 外的 7 个面板撑高和书签下拉撑高全部归零
fn clear_all_extras_except(keep: Panel) {
    for p in Panel::ALL {
        if p != keep {
            p.atom().store(0, Ordering::Relaxed);
        }
    }
    BM_EXTRA.store(0, Ordering::Relaxed);
}

/// 面板开合的撑高部分（不触发重排，便于单测）：开 = 自己置位且其余清零，关 = 自己归零
fn set_panel_extra(panel: Panel, open: bool) {
    panel.atom().store(if open { panel.extra_px() } else { 0 }, Ordering::Relaxed);
    if open {
        clear_all_extras_except(panel);
    }
}

/// 密码面板（F-CEF-12）展开时工具栏视图额外加高的像素，收起为 0
const PW_PANEL_EXTRA: i32 = 260;
static PW_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 书签文件夹下拉展开时额外加高：CEF 的工具栏是独立视图，`position:fixed` 的
/// 下拉框照样被 122px 视口裁掉（2026-10-26 实测截图里看不到），只能临时把
/// 视图撑高；面板收起即还原（内容区跟着上下让位，跟密码面板同一套做法）。
const BM_POPUP_EXTRA: i32 = 360;
static BM_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 下载面板（F-CEF-15）展开时工具栏视图额外加高的像素，收起为 0
const DL_PANEL_EXTRA: i32 = 300;
static DL_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 下载面板开合：与密码面板、书签下拉互斥，同一时刻只留一个撑高来源
pub fn set_dl_panel(open: bool) {
    set_panel_extra(Panel::Dl, open);
    apply_toolbar_height();
}

/// 主菜单（F-CEF-16）展开时工具栏视图额外加高的像素
const MENU_PANEL_EXTRA: i32 = 340;
static MENU_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 主菜单开合：同样与其它面板互斥
pub fn set_menu_panel(open: bool) {
    set_panel_extra(Panel::Menu, open);
    apply_toolbar_height();
}

/// 历史面板（F-CEF-19）展开时工具栏视图额外加高的像素
const HIST_PANEL_EXTRA: i32 = 340;
static HIST_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 历史面板开合：与其它面板互斥
pub fn set_hist_panel(open: bool) {
    set_panel_extra(Panel::Hist, open);
    apply_toolbar_height();
}

/// 站点信息面板（F-CEF-19）展开时工具栏视图额外加高的像素
const SITE_PANEL_EXTRA: i32 = 150;
static SITE_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 站点信息面板开合：与其它面板互斥
pub fn set_site_panel(open: bool) {
    set_panel_extra(Panel::Site, open);
    apply_toolbar_height();
}

/// 设置面板（F-CEF-19）展开时工具栏视图额外加高的像素
const SETTINGS_PANEL_EXTRA: i32 = 320;
static SETTINGS_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 设置面板开合：与其它面板互斥
pub fn set_settings_panel(open: bool) {
    set_panel_extra(Panel::Settings, open);
    apply_toolbar_height();
}

/// 书签管理面板（F-CEF-20）展开时工具栏视图额外加高的像素
const BM_MANAGE_PANEL_EXTRA: i32 = 380;
static BM_MANAGE_EXTRA_V: AtomicI32 = AtomicI32::new(0);

/// 书签管理面板开合：与其它面板互斥
pub fn set_bm_manage_panel(open: bool) {
    set_panel_extra(Panel::BmManage, open);
    apply_toolbar_height();
}

/// 用户脚本面板（F-CEF-23）展开时工具栏视图额外加高的像素（照 cert 口径 360）
const US_PANEL_EXTRA: i32 = 360;
static US_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 用户脚本面板开合：与其它面板互斥
pub fn set_userscript_panel(open: bool) {
    set_panel_extra(Panel::Us, open);
    apply_toolbar_height();
}

/// 把用户脚本列表推给工具栏面板（F-CEF-23，同 `push_certs` 模式）
pub(crate) fn push_userscripts() {
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    let json = super::userscripts_ui::userscripts_json(app);
    run_toolbar_js(&format!(
        "window.__shiftUserscripts && window.__shiftUserscripts({json})"
    ));
}

/// 证书面板（F-CEF-21）展开时工具栏视图额外加高的像素
const CERT_PANEL_EXTRA: i32 = 360;
static CERT_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 证书面板开合：与其它面板互斥
pub fn set_cert_panel(open: bool) {
    set_panel_extra(Panel::Cert, open);
    apply_toolbar_height();
}

/// 密码面板开合：加/减工具栏视图高度并重排（只动 PW_EXTRA，不碰书签栏态）
pub fn set_pw_panel(open: bool) {
    set_panel_extra(Panel::Pw, open);
    apply_toolbar_height();
}

/// 书签下拉开合：临时加高/还原工具栏视图（下拉要画在书签栏下方）。
/// `need` 是页面算出的下拉高度（含间距），钳在 80..360，防止异常值把窗口撑坏。
fn set_bm_popup(open: bool, need: i32) {
    let extra = if open {
        need.clamp(80, BM_POPUP_EXTRA)
    } else {
        0
    };
    BM_EXTRA.store(extra, Ordering::Relaxed);
    if open {
        FIND_EXTRA.store(0, Ordering::Relaxed);
    }
    apply_toolbar_height();
}

/// 页内查找栏（F-CEF-18）展开时工具栏视图额外加高的像素，收起为 0。
/// 查找栏是地址栏下方的浮层（44px），工具栏独立视图会被固定视口裁掉，
/// 所以开合都走撑高（跟密码面板同一套做法）。
const FIND_BAR_EXTRA: i32 = 44;
static FIND_EXTRA: AtomicI32 = AtomicI32::new(0);

/// 查找栏开合：与其它面板互斥，同一时刻只留一个撑高来源
fn set_find_bar(open: bool) {
    set_panel_extra(Panel::Find, open);
    apply_toolbar_height();
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

    /// 各标签的缩放档位（tab_id → zoom level，F-CEF-18）。
    /// CEF 154 的 C API 只有 `set_zoom_level`、没有 getter（C++ API 才有），
    /// 而档位只会被本模块改动，所以自己记账；未记录 = 0.0 = 100%。
    /// （`HashMap::new` 非 const，不能用 `const {}` 块，走惰性初始化。）
    static ZOOM_LEVELS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());

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
            // 焦点兜底走 X11（CEF 的 Window 没有 set_focus；主窗口不创建后
            // 必须自己拿焦点，否则 getactivewindow 为空、点不了）。
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
                        // X11 层兜底：直接给输入焦点（CEF 的 set_focus 不一定同步到 X）
                        x11::xlib::XSetInputFocus(
                            dpy,
                            win,
                            x11::xlib::RevertToParent,
                            x11::xlib::CurrentTime,
                        );

                        // 去掉系统标题栏（2026-10-26）：Chrome 的窗口按钮画在标签栏里，
                        // 没有独立标题栏。_MOTIF_WM_HINTS = { flags=2(装饰), decorations=0 }
                        {
                            let atom = x11::xlib::XInternAtom(
                                dpy,
                                c"_MOTIF_WM_HINTS".as_ptr(),
                                0,
                            );
                            // X11 的 32 位属性在 64 位平台上用 long 数组承载（只读低 32 位）
                            let hints: [std::os::raw::c_long; 5] = [0, 0, 0, 0, 0]; // 实验：先不抢装饰，验证输入
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
            Some(ShellRequestHandler::new(
                self.tab_id.clone(),
                !self.toolbar,
            ))
        }

        fn load_handler(&self) -> Option<LoadHandler> {
            // 内容页需要加载态（标签栏转圈）；工具栏页不需要
            if self.toolbar {
                None
            } else {
                Some(ShellContentLoadHandler::new(self.tab_id.clone()))
            }
        }

        /// 页面右键菜单（F-CEF-16）：只挂内容页，工具栏页不接管。
        fn context_menu_handler(&self) -> Option<ContextMenuHandler> {
            if self.toolbar {
                None
            } else {
                Some(ShellContextMenuHandler::new())
            }
        }

        fn download_handler(&self) -> Option<DownloadHandler> {
            Some(ShellDownloadHandler::new())
        }

        fn keyboard_handler(&self) -> Option<KeyboardHandler> {
            // 快捷键在工具栏和内容页都要生效（Ctrl+T 在地址栏里按也该开新标签）
            Some(ShellKeyboardHandler::new())
        }

        /// 页内查找（F-CEF-18）：find 作用于内容页浏览器，FindHandler 只挂内容页
        fn find_handler(&self) -> Option<FindHandler> {
            if self.toolbar {
                None
            } else {
                Some(ShellFindHandler::new(self.tab_id.clone()))
            }
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
/// 内容页自绘右键菜单（F-CEF-16）。
///
/// 为什么自绘：CEF 单栈窗口用 Alloy 运行时，2026-10-26 实测 `GetContextMenuHandler`
/// 根本不被调用，原生菜单弹不出来。改成内容页内注入 DOM 菜单。
///
/// 只发白名单动作（ctxback/ctxforward/ctxreload/ctxcopy/ctxselectall/ctxviewsource），
/// `on_console_message` 侧逐条比对；页面脚本伪造 `SHIFT_ACT:` 也触发不了敏感命令。
const CONTEXT_MENU_JS: &str = r#"
(function () {
  if (window.__shiftCtxMenuInstalled) return;
  window.__shiftCtxMenuInstalled = true;
  var menu = null;
  function close() { if (menu) { menu.remove(); menu = null; } }
  function item(label, act) {
    var d = document.createElement('div');
    d.textContent = label;
    d.style.cssText = 'padding:6px 10px;border-radius:4px;cursor:default;';
    d.onmouseenter = function () { d.style.background = '#3c4043'; };
    d.onmouseleave = function () { d.style.background = 'transparent'; };
    d.onclick = function () { console.log('SHIFT_ACT:' + act); close(); };
    return d;
  }
  function sep() {
    var s = document.createElement('div');
    s.style.cssText = 'height:1px;background:#3c4043;margin:6px 0;';
    return s;
  }
  document.addEventListener('contextmenu', function (e) {
    e.preventDefault();
    close();
    menu = document.createElement('div');
    menu.id = '__shift_ctxmenu';
    menu.style.cssText = 'position:fixed;z-index:2147483647;background:#292a2d;border:1px solid #3c4043;border-radius:8px;padding:6px;min-width:180px;box-shadow:0 8px 24px rgba(0,0,0,.5);font:12px system-ui,sans-serif;color:#e8eaed;';
    menu.appendChild(item('后退', 'ctxback'));
    menu.appendChild(item('前进', 'ctxforward'));
    menu.appendChild(item('重新加载', 'ctxreload'));
    menu.appendChild(sep());
    menu.appendChild(item('复制', 'ctxcopy'));
    menu.appendChild(item('全选', 'ctxselectall'));
    menu.appendChild(sep());
    menu.appendChild(item('查看网页源代码', 'ctxviewsource'));
    menu.style.left = Math.min(e.clientX, window.innerWidth - 200) + 'px';
    menu.style.top = Math.min(e.clientY, window.innerHeight - 240) + 'px';
    document.body.appendChild(menu);
  }, true);
  document.addEventListener('click', close, true);
  document.addEventListener('scroll', close, true);
  window.addEventListener('blur', close);
})();
"#;

/// 对当前活动标签的主 frame 执行一个动作（右键菜单用）。
fn with_active_frame(f: impl FnOnce(&mut Frame)) {
    let active_id = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|x| x.id.clone())
    });
    let Some(id) = active_id else { return };
    BROWSERS.with(|m| {
        if let Some(b) = m.borrow().get(&id) {
            if let Some(mut frame) = b.main_frame() {
                f(&mut frame);
            }
        }
    });
}

/// 当前活动标签的 (url, title)，取不到给空串。
fn active_tab_url_title() -> (String, String) {
    SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        tabs.get(ACTIVE_TAB.with(|a| a.get()))
            .map(|t| (t.url.clone(), t.title.clone()))
            .unwrap_or_default()
    })
}

/// LIKE 通配符转义：用户输入的 `%` / `_` / `\` 不能当通配符用。
fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 地址栏下拉建议（F-CEF-17）：历史 + 书签各取 8 条，按 URL 去重（书签优先）。
///
/// 查询在 tokio 线程，结果切回主线程推给工具栏页面。
fn suggest_omni(query: &str) {
    let q = query.trim();
    if q.is_empty() {
        run_toolbar_js("window.__shiftOmni && window.__shiftOmni([])");
        return;
    }
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    let Some(app) = crate::cef_cmd::app_handle() else {
        return;
    };
    let like = format!("%{}%", escape_like(q));
    tauri::async_runtime::spawn(async move {
        let hist = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT url, title FROM history \
             WHERE url LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\' \
             ORDER BY last_visit_at DESC LIMIT 8",
        )
        .bind(&like)
        .bind(&like)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        let bms = sqlx::query_as::<_, (String, String)>(
            "SELECT url, title FROM bookmarks \
             WHERE url LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\' \
             ORDER BY sort_order, id LIMIT 8",
        )
        .bind(&like)
        .bind(&like)
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for (url, title) in bms {
            if seen.insert(url.clone()) {
                out.push(serde_json::json!({"type":"bookmark","url":url,"title":title}));
            }
        }
        for (url, title) in hist {
            if seen.insert(url.clone()) {
                out.push(serde_json::json!({
                    "type":"history","url":url,"title":title.unwrap_or_default()
                }));
            }
        }
        let json = serde_json::to_string(&out).unwrap_or_else(|_| "[]".to_string());
        let _ = app.run_on_main_thread(move || {
            run_toolbar_js(&format!(
                "window.__shiftOmni && window.__shiftOmni({json})"
            ));
        });
    });
}

/// 星标（F-CEF-17）：已收藏则删除，未收藏则加入书签，然后刷新书签栏与星标。
fn toggle_star() {
    let (url, title) = active_tab_url_title();
    if url.is_empty() {
        return;
    }
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    let Some(app) = crate::cef_cmd::app_handle() else {
        return;
    };
    let title = if title.trim().is_empty() {
        url.clone()
    } else {
        title
    };
    tauri::async_runtime::spawn(async move {
        let existing: Option<i64> =
            sqlx::query_scalar("SELECT id FROM bookmarks WHERE url = ? LIMIT 1")
                .bind(&url)
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        let res = if let Some(id) = existing {
            sqlx::query("DELETE FROM bookmarks WHERE id = ?")
                .bind(id)
                .execute(&pool)
                .await
        } else {
            sqlx::query(
                "INSERT INTO bookmarks (title, url, folder_id, sort_order) \
                 VALUES (?, ?, NULL, COALESCE((SELECT MAX(sort_order) + 1 FROM bookmarks WHERE folder_id IS NULL), 0))",
            )
            .bind(&title)
            .bind(&url)
            .execute(&pool)
            .await
        };
        match res {
            Ok(_) => {
                log::info!("[cef] 星标切换: {url}");
                let _ = app.run_on_main_thread(reload_bookmarks_from_db);
                refresh_star_state(&url);
            }
            Err(e) => log::warn!("[cef] 星标切换失败: {e}"),
        }
    });
}

/// 查当前 URL 是否已收藏，推给工具栏星标。
fn refresh_star_state(url: &str) {
    let url = url.to_string();
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    let Some(app) = crate::cef_cmd::app_handle() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let on = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM bookmarks WHERE url = ?")
            .bind(&url)
            .fetch_one(&pool)
            .await
            .unwrap_or(0)
            > 0;
        let _ = app.run_on_main_thread(move || {
            run_toolbar_js(&format!(
                "window.__shiftStar && window.__shiftStar({on})"
            ));
        });
    });
}

/// 设置面板数据（F-CEF-19）：只推工具栏需要的四项。
fn send_settings() {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    tauri::async_runtime::spawn(async move {
        let mut map = serde_json::Map::new();
        for key in ["search_engine", "download_dir", "adblock_enabled", "gpc_header"] {
            let v: Option<String> =
                sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
                    .bind(key)
                    .fetch_optional(&pool)
                    .await
                    .ok()
                    .flatten();
            map.insert(key.to_string(), serde_json::Value::String(v.unwrap_or_default()));
        }
        let json = serde_json::Value::Object(map).to_string();
        let _ = app.run_on_main_thread(move || {
            run_toolbar_js(&format!(
                "window.__shiftSettings && window.__shiftSettings({json})"
            ));
        });
    });
}

/// 保存设置（只允许白名单四项；写库走 set_setting_inner，缓存/静态同步一起走）。
fn save_settings(json: &str) {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return;
    };
    let pairs: Vec<(String, String)> = ["search_engine", "download_dir", "adblock_enabled", "gpc_header"]
        .iter()
        .filter_map(|k| {
            v.get(*k)
                .and_then(|x| x.as_str())
                .map(|s| (k.to_string(), s.to_string()))
        })
        .collect();
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    tauri::async_runtime::spawn(async move {
        let mut errs = Vec::new();
        for (k, val) in pairs {
            if let Err(e) = crate::db::settings::set_setting_inner(&pool, &k, &val).await {
                errs.push(format!("{k}: {e}"));
            }
        }
        let msg = if errs.is_empty() {
            "已保存（广告拦截/GPC 重开窗口生效）".to_string()
        } else {
            format!("保存失败: {}", errs.join("; "))
        };
        let json_msg = serde_json::to_string(&msg).unwrap_or_else(|_| "\"\"".to_string());
        let _ = app.run_on_main_thread(move || {
            run_toolbar_js(&format!(
                "window.__shiftSettingsSaved && window.__shiftSettingsSaved({json_msg})"
            ));
        });
    });
}

/// 站点信息（F-CEF-19）：当前标签的 host / 协议 / 是否 HTTPS。
fn send_site_info() {
    let (url, _) = active_tab_url_title();
    let data = match url::Url::parse(&url) {
        Ok(u) => {
            let scheme = u.scheme().to_string();
            let secure = scheme == "https";
            serde_json::json!({
                "url": url,
                "host": u.host_str().unwrap_or(""),
                "scheme": scheme,
                "secure": secure,
            })
        }
        Err(_) => serde_json::json!({
            "url": url,
            "host": "",
            "scheme": "",
            "secure": false,
        }),
    };
    run_toolbar_js(&format!(
        "window.__shiftSiteInfo && window.__shiftSiteInfo({data})"
    ));
}

/// 把证书列表推给工具栏面板（F-CEF-21）。
pub(crate) fn push_certs() {
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    let json = super::certificates_ui::certificates_json(app);
    run_toolbar_js(&format!(
        "window.__shiftCerts && window.__shiftCerts({json})"
    ));
}

/// 历史面板数据（F-CEF-19）：空查询取最近 100 条，否则搜索 50 条。
fn send_history(query: &str) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    let Some(app) = crate::cef_cmd::app_handle() else {
        return;
    };
    let q = query.trim().to_string();
    tauri::async_runtime::spawn(async move {
        let res = if q.is_empty() {
            crate::db::history::list_recent(&pool, 100).await
        } else {
            crate::db::history::search(&pool, &q, 50).await
        };
        match res {
            Ok(rows) => {
                let json = serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string());
                let _ = app.run_on_main_thread(move || {
                    run_toolbar_js(&format!(
                        "window.__shiftHistory && window.__shiftHistory({json})"
                    ));
                });
            }
            Err(e) => log::warn!("[cef] 读历史失败: {e}"),
        }
    });
}

/// 清空全部历史（页面侧二次确认后再发）。
fn clear_history_all() {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match sqlx::query("DELETE FROM history").execute(&pool).await {
            Ok(r) => {
                log::info!("[cef] 历史已清空：{} 行", r.rows_affected());
                send_history("");
            }
            Err(e) => log::warn!("[cef] 清空历史失败: {e}"),
        }
    });
}

/// 删除单条历史。
fn delete_history_item(id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match sqlx::query("DELETE FROM history WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
        {
            Ok(r) if r.rows_affected() > 0 => {
                log::info!("[cef] 历史记录已删除 id={id}");
                send_history("");
            }
            Ok(_) => log::warn!("[cef] 历史记录不存在 id={id}"),
            Err(e) => log::warn!("[cef] 删除历史失败 id={id}: {e}"),
        }
    });
}

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
            // 星标状态跟着地址走（F-CEF-17）
            refresh_star_state(&text);
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
                // 推新格式（folders + bookmarks）：页面启动时自己的 bootBookmarks
                // 实测没跑起来（手动调 renderBookmarks 有效，说明是启动时序问题），
                // 这里在标题回调里补一次，确定会执行。
                reload_bookmarks_from_db();
            }
        }

        /// 右键菜单动作（F-CEF-16）：只放行白名单，其余 console 消息一律不处理。
        fn on_console_message(
            &self,
            _browser: Option<&mut Browser>,
            _level: LogSeverity,
            message: Option<&CefString>,
            _source: Option<&CefString>,
            _line: std::os::raw::c_int,
        ) -> i32 {
            let Some(msg) = message else { return 0 };
            let text = msg.to_string();
            match text.trim() {
                "SHIFT_ACT:ctxback" => handle_action("back"),
                "SHIFT_ACT:ctxforward" => handle_action("forward"),
                "SHIFT_ACT:ctxreload" => handle_action("reload"),
                "SHIFT_ACT:ctxcopy" => with_active_frame(|f| f.copy()),
                "SHIFT_ACT:ctxselectall" => with_active_frame(|f| f.select_all()),
                "SHIFT_ACT:ctxviewsource" => with_active_frame(|f| f.view_source()),
                _ => return 0,
            }
            1
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
    if let Some(ref id) = active_id {
        BROWSERS.with(|m| {
            if let Some(b) = m.borrow().get(id) {
                super::browser::focus(b);
            }
        });
    }
    // 页内查找是标签级状态（F-CEF-18）：切走时停旧标签的查找并收起查找栏，
    // 否则旧计数会留在工具栏上
    let prev = ACTIVE_TAB.with(|a| a.get());
    if prev != idx && FIND_EXTRA.load(Ordering::Relaxed) != 0 {
        let prev_id = SHELL_TABS.with(|t| t.borrow().get(prev).map(|x| x.id.clone()));
        if let Some(pid) = prev_id {
            BROWSERS.with(|m| {
                if let Some(host) = m.borrow().get(&pid).and_then(|b| b.host()) {
                    host.stop_finding(1);
                }
            });
        }
        LAST_FIND_TEXT.with(|t| *t.borrow_mut() = String::new());
        set_find_bar(false);
        run_toolbar_js("window.__shiftFindClose && window.__shiftFindClose()");
    }
    // 切标签后同步活动标签的缩放档位显示（缩放是标签级状态，自管表为准）
    let zoom_pct = active_id.and_then(|id| {
        ZOOM_LEVELS.with(|z| z.borrow().get(&id).map(|l| zoom_pct_for_level(*l)))
    });
    if let Some(pct) = zoom_pct {
        run_toolbar_js(&format!("window.__shiftZoom && window.__shiftZoom({pct})"));
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

    // 档位表随标签清理（CEF 侧没有 get_zoom_level，表是唯一事实源）
    ZOOM_LEVELS.with(|z| z.borrow_mut().remove(&id));
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

/// 从库重查书签并推给页面（操作增删改后调用）。
fn reload_bookmarks_from_db() {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else {
        log::warn!("[cef] 书签刷新失败：全局连接池未注册");
        return;
    };
    let pool = pool.clone();
    let rows = tauri::async_runtime::block_on(async move {
        let bms = sqlx::query_as::<_, (i64, String, String, Option<i64>)>(
            "SELECT id, title, url, folder_id FROM bookmarks ORDER BY sort_order, id LIMIT 300",
        )
        .fetch_all(&pool)
        .await?;
        let folders = sqlx::query_as::<_, (i64, String, Option<i64>)>(
            "SELECT id, name, parent_id FROM bookmark_folders ORDER BY sort_order, id",
        )
        .fetch_all(&pool)
        .await?;
        Ok::<_, sqlx::Error>((bms, folders))
    });
    match rows {
        Ok((bms, folders)) => {
            let list: Vec<(String, String)> = bms
                .iter()
                .map(|(_, t, u, _)| (t.clone(), u.clone()))
                .collect();
            BOOKMARKS.with(|b| *b.borrow_mut() = list);
            let bm_json: Vec<serde_json::Value> = bms
                .iter()
                .map(|(id, t, u, f)| serde_json::json!({ "id": id, "title": t, "url": u, "folderId": f }))
                .collect();
            let f_json: Vec<serde_json::Value> = folders
                .iter()
                .map(|(id, n, p)| serde_json::json!({ "id": id, "name": n, "parentId": p }))
                .collect();
            let json = serde_json::json!({ "folders": f_json, "bookmarks": bm_json }).to_string();
            run_toolbar_js(&format!(
                "if (window.__shiftBookmarks) {{ window.__shiftBookmarks({json}); }}"
            ));
            log::info!("[cef] 书签已刷新：{} 条 / {} 个文件夹", bms.len(), folders.len());
        }
        Err(e) => log::warn!("[cef] 书签刷新失败: {e}"),
    }
}

/// 新建书签文件夹并刷新。`parent_id` 为 None 表示顶层。
fn create_folder(name: &str, parent_id: Option<i64>) {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else { return };
    let pool = pool.clone();
    let name = name.to_string();
    let name_for_db = name.clone();
    let res = tauri::async_runtime::block_on(async move {
        sqlx::query(
            "INSERT INTO bookmark_folders (name, parent_id, sort_order, created_at) \
             VALUES (?, ?, 0, datetime('now'))",
        )
        .bind(&name_for_db)
        .bind(parent_id)
        .execute(&pool)
        .await
    });
    match res {
        Ok(r) => {
            log::info!("[cef] 文件夹已创建「{name}」parent={parent_id:?}（{} 行）", r.rows_affected());
            reload_bookmarks_from_db();
        }
        Err(e) => log::warn!("[cef] 新建文件夹失败: {e}"),
    }
}

/// 移动书签到文件夹（`folder_id` 为 None 表示移回书签栏）。
fn move_bookmark(id: i64, folder_id: Option<i64>) {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else { return };
    let pool = pool.clone();
    let res = tauri::async_runtime::block_on(async move {
        sqlx::query("UPDATE bookmarks SET folder_id = ? WHERE id = ?")
            .bind(folder_id)
            .bind(id)
            .execute(&pool)
            .await
    });
    match res {
        Ok(r) => {
            log::info!("[cef] 书签已移动 id={id} -> folder={folder_id:?}（{} 行）", r.rows_affected());
            reload_bookmarks_from_db();
        }
        Err(e) => log::warn!("[cef] 移动书签失败: {e}"),
    }
}

/// 改书签标题并刷新（右键菜单「编辑标题」）。
fn update_bookmark_title(id: i64, title: &str) {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else { return };
    let pool = pool.clone();
    let title = title.to_string();
    let res = tauri::async_runtime::block_on(async move {
        sqlx::query("UPDATE bookmarks SET title = ? WHERE id = ?")
            .bind(&title)
            .bind(id)
            .execute(&pool)
            .await
    });
    match res {
        Ok(r) => {
            log::info!("[cef] 书签标题已更新 id={id}（{} 行）", r.rows_affected());
            reload_bookmarks_from_db();
        }
        Err(e) => log::warn!("[cef] 更新书签失败: {e}"),
    }
}

/// 删除书签并刷新（右键菜单「删除」）。
fn delete_bookmark_and_refresh(id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else { return };
    let pool = pool.clone();
    let res = tauri::async_runtime::block_on(async move {
        sqlx::query("DELETE FROM bookmarks WHERE id = ?")
            .bind(id)
            .execute(&pool)
            .await
    });
    match res {
        Ok(r) => {
            log::info!("[cef] 书签已删除 id={id}（影响 {} 行）", r.rows_affected());
            reload_bookmarks_from_db();
        }
        Err(e) => log::warn!("[cef] 删除书签失败: {e}"),
    }
}

/// 把当前页加为书签（右键菜单「新建书签」）。
/// 删除书签文件夹（级联删子夹与夹内书签；F-CEF-20）。
fn delete_folder_and_refresh(id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        match crate::db::bookmarks::delete_bookmark_folder_inner(&pool, id).await {
            Ok(()) => {
                log::info!("[cef] 书签文件夹已删除 id={id}");
                if let Some(app) = crate::cef_cmd::app_handle() {
                    let _ = app.run_on_main_thread(reload_bookmarks_from_db);
                }
            }
            Err(e) => log::warn!("[cef] 删除书签文件夹失败 id={id}: {e}"),
        }
    });
}

fn add_bookmark_from_current() {
    let Some(pool) = crate::db::GLOBAL_POOL.get() else { return };
    // 取当前活动标签的 URL 与标题
    let (url, title) = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        let idx = ACTIVE_TAB.with(|a| a.get());
        tabs.get(idx)
            .map(|x| (x.url.clone(), x.title.clone()))
            .unwrap_or_default()
    });
    if url.is_empty() || url.starts_with("file://") {
        log::info!("[cef] 当前页不可收藏: {url}");
        return;
    }
    let pool = pool.clone();
    let (title_for_db, url_for_db) = (title.clone(), url.clone());
    let res = tauri::async_runtime::block_on(async move {
        sqlx::query(
            "INSERT INTO bookmarks (title, url, folder_id, sort_order, created_at) VALUES (?, ?, NULL, 0, datetime('now'))",
        )
        .bind(&title_for_db)
        .bind(&url_for_db)
        .execute(&pool)
        .await
    });
    match res {
        Ok(_) => {
            log::info!("[cef] 已收藏: {title} -> {url}");
            reload_bookmarks_from_db();
        }
        Err(e) => log::warn!("[cef] 收藏失败: {e}"),
    }
}

/// 把书签推给工具栏页面的书签栏。
///
/// 只此一份实现：必须带文件夹（`{folders,bookmarks}`），否则页面会把
/// 已分组的书签按平铺重画、文件夹被抹掉（2026-10-26 实测：另一条老路径
/// `refresh_bookmarks_ui` 推纯数组，启动后把 13 个文件夹覆盖成 65 条平铺）。
pub fn refresh_bookmarks_ui() {
    reload_bookmarks_from_db();
}

/// 在工具栏页面里执行一段 JS（地址栏/加载态等 UI 更新统一走这里）
pub(crate) fn run_toolbar_js(code: &str) {
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
    let next = if TOOLBAR_H.load(Ordering::Relaxed) == TOOLBAR_HEIGHT_COLLAPSED {
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

// ---- 缩放（F-CEF-18）----
// CEF 的 zoom level：0 = 100%，数值越大放得越大。档位对齐 Chrome 的
// 90/100/110/125/150/175/200（派单给的档位值，低档位与 1.2^level 换算吻合）。
// 只做当前标签；按站点记忆是 P2。
const ZOOM_STEPS: [f64; 7] = [-0.58, 0.0, 0.53, 1.14, 2.01, 2.65, 3.41];
const ZOOM_PCTS: [u32; 7] = [90, 100, 110, 125, 150, 175, 200];

/// 找最接近 `level` 的档位下标（同距离取较低档）
fn nearest_zoom_idx(level: f64) -> usize {
    (0..ZOOM_STEPS.len()).fold(0, |best, i| {
        if (ZOOM_STEPS[i] - level).abs() < (ZOOM_STEPS[best] - level).abs() {
            i
        } else {
            best
        }
    })
}

/// 把实际 zoom level 归到最近档位，得到工具栏显示的百分比
fn zoom_pct_for_level(level: f64) -> u32 {
    ZOOM_PCTS[nearest_zoom_idx(level)]
}

/// 上一档/下一档/复位的目标档位。`dir`：+1 放大、-1 缩小、0 回 100%。
fn zoom_step(level: f64, dir: i32) -> f64 {
    if dir == 0 {
        return ZOOM_STEPS[1];
    }
    let i = nearest_zoom_idx(level);
    let n = (i as i32 + dir).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize;
    ZOOM_STEPS[n]
}

/// 对活动标签执行缩放，并把档位百分比推给工具栏显示。
fn apply_zoom(dir: i32) {
    let active_id = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|tab| tab.id.clone())
    });
    let Some(active_id) = active_id else {
        return;
    };
    // CEF 侧没有 get_zoom_level（154 C API 只有 setter），档位以自管表为准
    let (cur, target) = ZOOM_LEVELS.with(|z| {
        let map = z.borrow();
        let cur = map.get(&active_id).copied().unwrap_or(0.0);
        (cur, zoom_step(cur, dir))
    });
    if target == cur {
        return;
    }
    ZOOM_LEVELS.with(|z| z.borrow_mut().insert(active_id.clone(), target));
    BROWSERS.with(|m| {
        if let Some(host) = m.borrow().get(&active_id).and_then(|b| b.host()) {
            host.set_zoom_level(target);
        }
    });
    let pct = zoom_pct_for_level(target);
    log::info!("[cef] 缩放 dir={dir:+} level {cur} -> {target}（{pct}%）");
    run_toolbar_js(&format!("window.__shiftZoom && window.__shiftZoom({pct})"));
}

// ---- 页内查找（F-CEF-18）----
// 查找是标签级状态：动作从工具栏发出（内容页 console 白名单不放行查找动作，
// 页面脚本伪造 SHIFT_ACT 触发不了），CEF FindHandler 回调只在来源标签是
// 活动标签时把 count/ordinal 推给工具栏。
// 最近一次的查找词：CEF `find` 每次调用都要带词，findnext/findprev 重发它。
// （UI 线程专用，跟本文件其它 UI 状态一样用 thread_local。）
thread_local! {
    static LAST_FIND_TEXT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 打开查找栏：撑高视图、工具栏页面显示并聚焦输入框、把键盘焦点切回工具栏
/// （查找栏是独立 BrowserView，不 set_focus 就收不到按键）
fn open_find_bar() {
    set_find_bar(true);
    run_toolbar_js("window.__shiftFindOpen && window.__shiftFindOpen()");
    BROWSERS.with(|m| {
        if let Some(b) = m.borrow().get(TOOLBAR_ID) {
            super::browser::focus(b);
        }
    });
}

/// 关闭查找栏：停活动标签的查找（清高亮）、还原视图高度、收起工具栏浮层
fn close_find_bar() {
    let active_id = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|tab| tab.id.clone())
    });
    if let Some(id) = active_id {
        BROWSERS.with(|m| {
            if let Some(host) = m.borrow().get(&id).and_then(|b| b.host()) {
                host.stop_finding(1);
            }
        });
    }
    LAST_FIND_TEXT.with(|t| *t.borrow_mut() = String::new());
    set_find_bar(false);
    run_toolbar_js("window.__shiftFindClose && window.__shiftFindClose()");
}

/// 在活动标签上发起/继续查找。关键词只记字符数进日志，不落全文。
fn do_find(text: &str, forward: i32, find_next: i32) {
    if text.trim().is_empty() {
        return;
    }
    let active_id = SHELL_TABS.with(|t| {
        let tabs = t.borrow();
        tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|tab| tab.id.clone())
    });
    let Some(active_id) = active_id else {
        return;
    };
    LAST_FIND_TEXT.with(|t| *t.borrow_mut() = text.to_string());
    BROWSERS.with(|m| {
        let map = m.borrow();
        match map.get(&active_id).and_then(|b| b.host()) {
            Some(host) => {
                host.find(Some(&CefString::from(text)), forward, 0, find_next);
                log::info!(
                    "[cef] 页内查找: {} 字, forward={forward}, find_next={find_next}",
                    text.chars().count()
                );
            }
            None => log::warn!("[cef] 页内查找: 活动标签 {active_id} 无 host"),
        }
    });
}

/// 执行工具栏动作
fn handle_action(action: &str) {
    let (cmd, arg) = match action.split_once('|') {
        Some((c, a)) => (c, a),
        None => (action, ""),
    };

    // 密码面板动作（F-CEF-12）
    if super::passwords_ui::handle(cmd, arg) {
        return;
    }
    // 证书面板动作（F-CEF-21）
    if super::certificates_ui::handle(cmd, arg) {
        return;
    }
    // 用户脚本面板动作（F-CEF-23）
    if super::userscripts_ui::handle(cmd, arg) {
        return;
    }

    // 下载面板动作（F-CEF-15）：面板开合 / 打开目录 / 重新下载 / 删记录
    match cmd {
        "menupanel" => {
            set_menu_panel(arg == "1");
            return;
        }
        "bmtoggle" => {
            toggle_bookmarks();
            return;
        }
        "omni" => {
            suggest_omni(arg);
            return;
        }
        "startoggle" => {
            toggle_star();
            return;
        }
        "bmmanage" => {
            set_bm_manage_panel(arg == "1");
            if arg == "1" {
                reload_bookmarks_from_db();
            }
            return;
        }
        "settingspanel" => {
            set_settings_panel(arg == "1");
            if arg == "1" {
                send_settings();
            }
            return;
        }
        "settingssave" => {
            save_settings(arg);
            return;
        }
        "siteinfo" => {
            send_site_info();
            return;
        }
        "sitepanel" => {
            set_site_panel(arg == "1");
            return;
        }
        "histpanel" => {
            set_hist_panel(arg == "1");
            if arg == "1" {
                send_history("");
            }
            return;
        }
        "histsearch" => {
            send_history(arg);
            return;
        }
        "histclear" => {
            clear_history_all();
            return;
        }
        "histdel" => {
            if let Ok(id) = arg.parse::<i64>() {
                delete_history_item(id);
            }
            return;
        }
        "dlpanel" => {
            set_dl_panel(arg == "1");
            return;
        }
        // 缩放（F-CEF-18）：工具栏 −/+/百分比 控件与主菜单共用
        "zoomin" => {
            apply_zoom(1);
            return;
        }
        "zoomout" => {
            apply_zoom(-1);
            return;
        }
        "zoomreset" => {
            apply_zoom(0);
            return;
        }
        // 页内查找（F-CEF-18）：动作只从工具栏来（内容页 console 白名单不含这些）
        "findopen" => {
            open_find_bar();
            return;
        }
        "find" => {
            do_find(arg, 1, 0);
            return;
        }
        "findnext" => {
            let t = LAST_FIND_TEXT.with(|t| t.borrow().clone());
            if !t.is_empty() {
                do_find(&t, 1, 1);
            }
            return;
        }
        "findprev" => {
            let t = LAST_FIND_TEXT.with(|t| t.borrow().clone());
            if !t.is_empty() {
                do_find(&t, 0, 1);
            }
            return;
        }
        "findclose" => {
            close_find_bar();
            return;
        }
        "dlopen" => {
            if let Ok(id) = arg.parse::<i64>() {
                open_download_location(id);
            }
            return;
        }
        "dlretry" => {
            if let Ok(id) = arg.parse::<i64>() {
                retry_download(id);
            }
            return;
        }
        "dldel" => {
            if let Ok(id) = arg.parse::<i64>() {
                delete_download_record(id);
            }
            return;
        }
        _ => {}
    }

    // 标签相关动作先处理（不需要 current content 浏览器）
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
        "bmedit" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(arg) {
                let id = v["id"].as_str().and_then(|s| s.parse::<i64>().ok());
                let title = v["title"].as_str().unwrap_or("").to_string();
                if let Some(id) = id {
                    update_bookmark_title(id, &title);
                }
            }
            return;
        }
        "bmfoldopen" => {
            let need = arg.parse::<i32>().unwrap_or(BM_POPUP_EXTRA);
            set_bm_popup(true, need);
            return;
        }
        "bmfoldclose" => {
            set_bm_popup(false, 0);
            return;
        }
        "bmnewfolder" => {
            // 两种入参：纯名称（顶层）或 {name,parentId}（子文件夹）
            if arg.starts_with('{') {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(arg) {
                    let name = v["name"].as_str().unwrap_or("").trim().to_string();
                    let parent = v["parentId"].as_i64();
                    if !name.is_empty() {
                        create_folder(&name, parent);
                    }
                }
            } else if !arg.trim().is_empty() {
                create_folder(arg.trim(), None);
            }
            return;
        }
        "bmdeletefolder" => {
            if let Ok(id) = arg.parse::<i64>() {
                delete_folder_and_refresh(id);
            }
            return;
        }
        "bmmove" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(arg) {
                let id = v["id"]
                    .as_i64()
                    .or_else(|| v["id"].as_str().and_then(|s| s.parse::<i64>().ok()));
                let folder = v["folderId"].as_i64();
                if let Some(id) = id {
                    move_bookmark(id, folder);
                }
            }
            return;
        }
        "bmdel" => {
            if let Ok(id) = arg.parse::<i64>() {
                delete_bookmark_and_refresh(id);
            }
            return;
        }
        "bmadd" => {
            add_bookmark_from_current();
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
            log::warn!(
                "[cef] BROWSERS 查不到 active_id={active_id}，现有 key={:?}",
                map.keys().collect::<Vec<_>>()
            );
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
    use super::{
        escape_like, is_attachment_disposition, nearest_zoom_idx, normalize_url, urlencode,
        zoom_pct_for_level, zoom_step, CONTEXT_MENU_JS,
    };

    // ---- 面板互斥矩阵（F-CEF-21）：撑高原子是进程级全局，全部断言放同一个测试里串行跑 ----

    #[test]
    fn panel_registry_values_match_contract() {
        use super::Panel;
        let want = [
            (Panel::Pw, "pw-open", 260),
            (Panel::Dl, "dl-open", 300),
            (Panel::Menu, "menu-open", 340),
            (Panel::Hist, "hist-open", 340),
            (Panel::Site, "site-open", 150),
            (Panel::Settings, "settings-open", 320),
            (Panel::BmManage, "bmmanage-open", 380),
            (Panel::Find, "find-open", 44),
            (Panel::Cert, "cert-open", 360),
            (Panel::Us, "us-open", 360),
        ];
        assert_eq!(Panel::ALL.len(), want.len());
        for (p, c, px) in want {
            assert_eq!((p.class(), p.extra_px()), (c, px), "{p:?}");
        }
    }

    #[test]
    fn opening_any_panel_zeroes_all_other_extras_90_pairs() {
        use super::{set_panel_extra, toolbar_height, Panel, BM_EXTRA, TOOLBAR_H};
        use std::sync::atomic::Ordering;
        let base = TOOLBAR_H.load(Ordering::Relaxed);
        let mut pairs = 0;
        for a in Panel::ALL {
            for b in Panel::ALL {
                if a == b {
                    continue;
                }
                set_panel_extra(a, true);
                BM_EXTRA.store(360, Ordering::Relaxed);
                set_panel_extra(b, true);
                for p in Panel::ALL {
                    let v = p.atom().load(Ordering::Relaxed);
                    assert_eq!(v, if p == b { b.extra_px() } else { 0 }, "{a:?}->{b:?} 残留 {p:?}");
                }
                assert_eq!(BM_EXTRA.load(Ordering::Relaxed), 0, "{a:?}->{b:?} 书签下拉未清");
                assert_eq!(toolbar_height(), base + b.extra_px(), "{a:?}->{b:?}");
                set_panel_extra(b, false);
                assert_eq!(toolbar_height(), base);
                pairs += 1;
            }
        }
        assert_eq!(pairs, 90);
    }

    // ---- normalize_url：地址栏补全规则（P0 实测逻辑，防改坏）----

    // ---- 缩放档位（F-CEF-18）----

    #[test]
    fn zoom_step_steps_through_chrome_levels_and_clamps() {
        // 逐档：100 -> 110 -> 125 -> 150 -> 175 -> 200（档位值以 ZOOM_STEPS 为准）
        assert_eq!(zoom_step(0.0, 1), 0.53);
        assert_eq!(zoom_step(0.53, 1), 1.14);
        assert_eq!(zoom_step(1.14, 1), 2.01);
        assert_eq!(zoom_step(2.01, 1), 2.65);
        assert_eq!(zoom_step(2.65, 1), 3.41);
        // 高档钳制：200% 再放大不越界
        assert_eq!(zoom_step(3.41, 1), 3.41);
        // 逐档缩小：110 -> 100 -> 90
        assert_eq!(zoom_step(0.53, -1), 0.0);
        assert_eq!(zoom_step(0.0, -1), -0.58);
        // 低档钳制：90% 再缩小不越界
        assert_eq!(zoom_step(-0.58, -1), -0.58);
    }

    #[test]
    fn zoom_step_reset_always_returns_100pct() {
        assert_eq!(zoom_step(0.0, 0), 0.0);
        assert_eq!(zoom_step(-0.58, 0), 0.0);
        assert_eq!(zoom_step(3.41, 0), 0.0);
    }

    #[test]
    fn zoom_pct_maps_each_level_to_its_percent() {
        assert_eq!(zoom_pct_for_level(-0.58), 90);
        assert_eq!(zoom_pct_for_level(0.0), 100);
        assert_eq!(zoom_pct_for_level(0.53), 110);
        assert_eq!(zoom_pct_for_level(1.14), 125);
        assert_eq!(zoom_pct_for_level(2.01), 150);
        assert_eq!(zoom_pct_for_level(2.65), 175);
        assert_eq!(zoom_pct_for_level(3.41), 200);
    }

    #[test]
    fn zoom_pct_nearest_rounding_is_deterministic() {
        // 0.0 与 0.53 的中点 0.265 之下归 100%，之上归 110%
        assert_eq!(zoom_pct_for_level(0.26), 100);
        assert_eq!(zoom_pct_for_level(0.27), 110);
        // 越界值归到最近端档
        assert_eq!(nearest_zoom_idx(-1.0), 0);
        assert_eq!(nearest_zoom_idx(5.0), 6);
    }

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

    // ---- 下载：Content-Disposition 判定（F-CEF-14/15）----

    #[test]
    fn attachment_disposition_matches_attachment_only() {
        assert!(is_attachment_disposition("attachment"));
        assert!(is_attachment_disposition("ATTACHMENT"));
        assert!(is_attachment_disposition("Attachment; filename=\"a.zip\""));
        assert!(is_attachment_disposition(
            "attachment;filename*=UTF-8''%E6%B5%8B.zip"
        ));
        assert!(!is_attachment_disposition("inline"));
        assert!(!is_attachment_disposition(""));
        assert!(!is_attachment_disposition("x-attachment-foo"));
        assert!(!is_attachment_disposition("inline; filename=\"a.zip\""));
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("a%b_c\\d"), "a\\%b\\_c\\\\d");
        assert_eq!(escape_like("正常"), "正常");
        assert_eq!(escape_like("100%"), "100\\%");
    }

    /// 右键菜单只许发白名单动作，页面脚本伪造 `SHIFT_ACT:` 也不能越权。
    #[test]
    fn context_menu_js_only_emits_whitelisted_actions() {
        for act in [
            "ctxback",
            "ctxforward",
            "ctxreload",
            "ctxcopy",
            "ctxselectall",
            "ctxviewsource",
        ] {
            assert!(
                CONTEXT_MENU_JS.contains(&format!("'{act}'")),
                "右键菜单缺少动作 {act}"
            );
        }
        // 不允许出现标签/窗口/书签/密码等敏感动作
        for forbidden in ["tabnew", "tabclose", "winclose", "bmadd", "pwdel"] {
            assert!(
                !CONTEXT_MENU_JS.contains(forbidden),
                "右键菜单混入了越权动作 {forbidden}"
            );
        }
    }
}

// ---- 页面右键菜单（F-CEF-16）----
// Chrome 页面右键最小集：导航 + 编辑 + 查看源代码。
// 命令 ID 从 1001 起，避开 CEF 内置 ID。
const CMD_CTX_BACK: i32 = 1001;
const CMD_CTX_FORWARD: i32 = 1002;
const CMD_CTX_RELOAD: i32 = 1003;
const CMD_CTX_COPY: i32 = 1004;
const CMD_CTX_SELECT_ALL: i32 = 1005;
const CMD_CTX_VIEW_SOURCE: i32 = 1006;

wrap_context_menu_handler! {
    struct ShellContextMenuHandler;

    impl ContextMenuHandler {
        fn on_before_context_menu(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            model: Option<&mut MenuModel>,
        ) {
            let Some(model) = model else { return };
            // CEF 传进来的 model 可能带默认项；先清空，按 Chrome 的顺序重建。
            let _ = model.clear();
            let _ = model.add_item(CMD_CTX_BACK, Some(&CefString::from("后退")));
            let _ = model.add_item(CMD_CTX_FORWARD, Some(&CefString::from("前进")));
            let _ = model.add_item(CMD_CTX_RELOAD, Some(&CefString::from("重新加载")));
            let _ = model.add_separator();
            let _ = model.add_item(CMD_CTX_COPY, Some(&CefString::from("复制")));
            let _ = model.add_item(CMD_CTX_SELECT_ALL, Some(&CefString::from("全选")));
            let _ = model.add_separator();
            let _ = model.add_item(
                CMD_CTX_VIEW_SOURCE,
                Some(&CefString::from("查看网页源代码")),
            );
        }

        fn on_context_menu_command(
            &self,
            browser: Option<&mut Browser>,
            frame: Option<&mut Frame>,
            _params: Option<&mut ContextMenuParams>,
            command_id: ::std::os::raw::c_int,
            _event_flags: EventFlags,
        ) -> ::std::os::raw::c_int {
            match command_id {
                CMD_CTX_BACK => {
                    if let Some(b) = browser {
                        b.go_back();
                    }
                }
                CMD_CTX_FORWARD => {
                    if let Some(b) = browser {
                        b.go_forward();
                    }
                }
                CMD_CTX_RELOAD => {
                    if let Some(b) = browser {
                        b.reload();
                    }
                }
                CMD_CTX_COPY => {
                    if let Some(f) = frame {
                        f.copy();
                    }
                }
                CMD_CTX_SELECT_ALL => {
                    if let Some(f) = frame {
                        f.select_all();
                    }
                }
                CMD_CTX_VIEW_SOURCE => {
                    if let Some(f) = frame {
                        f.view_source();
                    }
                }
                _ => return 0,
            }
            1
        }
    }
}

wrap_request_handler! {
    struct ShellRequestHandler {
        tab_id: String,
        content: bool,
    }

    impl RequestHandler {
        /// 主框架导航挂一个资源请求处理器：只为在 `on_resource_response` 里看
        /// 响应头（`Content-Disposition: attachment`）决定是否转下载。
        /// 子资源（图片/JS）不挂，避免每个请求都过一次回调。
        fn resource_request_handler(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            is_navigation: std::os::raw::c_int,
            _is_download: std::os::raw::c_int,
            _request_initiator: Option<&CefString>,
            _disable_default_handling: Option<&mut std::os::raw::c_int>,
        ) -> Option<ResourceRequestHandler> {
            if self.content && is_navigation != 0 {
                Some(ShellResourceRequestHandler::new())
            } else {
                None
            }
        }

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

wrap_resource_request_handler! {
    struct ShellResourceRequestHandler;

    impl ResourceRequestHandler {
        /// CEF 请求继续开关。
        ///
        /// **必须显式返回 `ReturnValue::CONTINUE`**：cef crate 里
        /// `ReturnValue::default()` 实现为 `RV_CANCEL`，用它会把所有导航请求
        /// 直接取消（2026-10-26 实测：zip 导航变 about:blank、响应回调永不触发）。
        fn on_before_resource_load(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _request: Option<&mut Request>,
            _callback: Option<&mut Callback>,
        ) -> ReturnValue {
            ReturnValue::CONTINUE
        }

        /// 响应头到手、正文还没读：发现 `Content-Disposition: attachment` 就
        /// 取消本次导航并改走下载。
        ///
        /// 为什么不在 `on_before_resource_load` 做：那里只有请求头，看不到
        /// `Content-Disposition`（2026-10-26 实测：attach 响应会当二进制页面渲染，
        /// `Content-Type: application/octet-stream` 页面上白屏、标题为空）。
        fn on_resource_response(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            request: Option<&mut Request>,
            response: Option<&mut Response>,
        ) -> std::os::raw::c_int {
            let (Some(req), Some(resp)) = (request, response) else {
                return 0;
            };
            let disp = CefStringUtf16::from(&resp.header_by_name(Some(&CefString::from(
                "Content-Disposition",
            ))))
            .to_string();
            let url = CefStringUtf16::from(&req.url()).to_string();
            if !is_attachment_disposition(&disp) || url.is_empty() {
                return 0;
            }
            log::info!("[cef] attachment 转下载: {url}");
            // CEF 的 ResourceRequestHandler 跑在 IO 线程，`BrowserHost::start_download`
            // 必须在 UI 线程调用，所以投递回主线程再执行（2026-10-26 实测：直接调无效）。
            let Some(app) = crate::cef_cmd::app_handle() else {
                log::warn!("[cef] attachment 转下载失败：AppHandle 未初始化");
                return 1;
            };
            let url_for_ui = url.clone();
            let _ = app.run_on_main_thread(move || {
                BROWSERS.with(|m| {
                    let map = m.borrow();
                    let Some(b) = map.get(CONTENT_ID) else {
                        log::warn!("[cef] attachment 转下载失败：BROWSERS 里没有 {CONTENT_ID}");
                        return;
                    };
                    let Some(host) = b.host() else {
                        log::warn!("[cef] attachment 转下载失败：内容页无 host");
                        return;
                    };
                    host.start_download(Some(&CefString::from(url_for_ui.as_str())));
                });
            });
            1 // 取消本次导航（正文不再加载）
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
            // 右键菜单（F-CEF-16）：内容页自绘，Alloy 模式不弹原生菜单
            frame.execute_java_script(
                Some(&CefString::from(CONTEXT_MENU_JS)),
                Some(&CefString::from("shift://ctxmenu")),
                0,
            );
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

wrap_find_handler! {
    struct ShellFindHandler {
        tab_id: String,
    }

    impl FindHandler {
        /// CEF 查找结果回调（UI 线程）。count/ordinal 是内核真值，
        /// 转推给工具栏显示「3/17」；页面 JS 自己数不到这个数。
        /// 只有来源标签是活动标签才推，后台标签的查找结果不覆盖前台计数。
        fn on_find_result(
            &self,
            _browser: Option<&mut Browser>,
            _identifier: std::os::raw::c_int,
            count: std::os::raw::c_int,
            _selection_rect: Option<&Rect>,
            active_match_ordinal: std::os::raw::c_int,
            _final_update: std::os::raw::c_int,
        ) {
            let active_id = SHELL_TABS.with(|t| {
                let tabs = t.borrow();
                tabs.get(ACTIVE_TAB.with(|a| a.get())).map(|x| x.id.clone())
            });
            if active_id.as_deref() != Some(self.tab_id.as_str()) {
                return;
            }
            // C API 的 ordinal 是 0 基（-1 = 无匹配），原样透传给工具栏；
            // 1 基「第几条」由工具栏侧转（renderFindCount 已 +1 并钳制到 count），
            // Rust 侧再加 1 会把首条显示成 2/N（2026-10-26 真机发现）
            run_toolbar_js(&format!(
                "window.__shiftFind && window.__shiftFind({count}, {active_match_ordinal})"
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
        // F5 刷新（无 Ctrl；VK_F5 = 116）
        if ev.windows_key_code == 116 {
            handle_action("reload");
            if let Some(flag) = is_shortcut {
                *flag = 1;
            }
            return 1;
        }
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
    // Ctrl+Shift+O 书签管理器（F-CEF-20；O = 0x4F = 79）
    if shift && ev.windows_key_code == 79 {
        run_toolbar_js("setBmManage(true)");
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
        // Ctrl+R 刷新（F-CEF-19 快捷键补齐）
        82 => {
            handle_action("reload");
            1
        }
        // Ctrl+L 聚焦地址栏
        76 => {
            run_toolbar_js(
                "const a=document.getElementById('addr'); if(a){a.focus();a.select();}",
            );
            1
        }
        // Ctrl+H 历史面板
        72 => {
            run_toolbar_js("setHistPanel(true)");
            1
        }
        // Ctrl+J 下载面板
        74 => {
            run_toolbar_js("setDlPanel(true)");
            1
        }
        // Ctrl+F 页内查找（F-CEF-18）
        70 => {
            open_find_bar();
            1
        }
        // Ctrl++（不按 Shift 是 Ctrl+=；windows_key_code 两者同为 187，
        // 数字小键盘 + 是 107）放大
        187 | 107 => {
            apply_zoom(1);
            1
        }
        // Ctrl+- 缩小（数字小键盘 - 是 109）
        189 | 109 => {
            apply_zoom(-1);
            1
        }
        // Ctrl+0 复位 100%
        48 => {
            apply_zoom(0);
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

/// 判断 `Content-Disposition` 响应头是否要求下载。
///
/// 只认首段指令 `attachment`（大小写不敏感）；`inline`、空串、其它值都不算。
/// **单一来源**：`ShellResourceRequestHandler::on_resource_response` 只调这一份。
fn is_attachment_disposition(value: &str) -> bool {
    value
        .split(';')
        .next()
        .map(|v| v.trim().eq_ignore_ascii_case("attachment"))
        .unwrap_or(false)
}

// ---- 下载记账（F-CEF-15）----
// CEF 的 `DownloadItem` 没有 user data，`on_before_download` 与 `on_download_updated`
// 只能靠 `item.id()` 关联。运行期维护一张映射表；进程退出即丢，库里的记录才是持久事实。
// 所有写库/发事件都走异步任务，**不得在 CEF 回调线程 block_on**（会卡死 UI）。

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

struct DlTrack {
    db_id: Arc<Mutex<Option<i64>>>,
    last_progress: Arc<Mutex<Instant>>,
    finished: Arc<AtomicBool>,
}

fn dl_tracks() -> &'static Mutex<HashMap<u32, DlTrack>> {
    static TRACKS: OnceLock<Mutex<HashMap<u32, DlTrack>>> = OnceLock::new();
    TRACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 发 `downloads-updated`（主界面弹窗/状态栏角标据此刷新）。
fn emit_downloads_updated() {
    use tauri::Emitter;
    if let Some(app) = crate::cef_cmd::app_handle() {
        let _ = app.emit(crate::contracts::events::DOWNLOADS_UPDATED, ());
    }
}

/// 工具栏下载面板的数据（只给页面必要字段，路径不出 Rust 层）。
pub(crate) fn downloads_panel_json(list: &[crate::db::downloads::Download]) -> String {
    let rows: Vec<serde_json::Value> = list
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id,
                "filename": d.filename,
                "state": d.state,
                "total": d.total_bytes,
                "received": d.received_bytes,
                "createdAt": d.created_at,
            })
        })
        .collect();
    serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
}

/// 通知工具栏面板重拉列表：库查询在 tokio 线程，JS 执行切回主线程。
fn request_downloads_refresh() {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    tauri::async_runtime::spawn(async move {
        let Ok(list) = crate::db::downloads::list_downloads(&pool).await else { return };
        let json = downloads_panel_json(&list);
        let _ = app.run_on_main_thread(move || {
            run_toolbar_js(&format!(
                "if (window.__shiftDownloads) {{ window.__shiftDownloads({json}); }}"
            ));
        });
    });
}

/// 登记一次下载：先建运行期槽，再异步写库（路径已去过重）。
fn register_download(item_id: u32, filename: String, url: Option<String>, path: std::path::PathBuf) {
    let track = DlTrack {
        db_id: Arc::new(Mutex::new(None)),
        last_progress: Arc::new(Mutex::new(Instant::now())),
        finished: Arc::new(AtomicBool::new(false)),
    };
    let slot = track.db_id.clone();
    dl_tracks()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(item_id, track);

    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        log::warn!("[cef] 下载记账跳过：全局连接池未注册");
        return;
    };
    let path_str = path.to_string_lossy().to_string();
    tauri::async_runtime::spawn(async move {
        match crate::db::downloads::insert_download(
            &pool,
            &filename,
            url.as_deref(),
            Some(&path_str),
            0,
        )
        .await
        {
            Ok(id) => {
                *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
                log::info!("[cef] 下载记录已入库 id={id} file={filename}");
                emit_downloads_updated();
                request_downloads_refresh();
            }
            Err(e) => log::warn!("[cef] 下载记录入库失败: {e}"),
        }
    });
}

/// 等 db id 就绪（写入是异步的，进度/完成回调可能先到；最多等 5 秒）。
async fn wait_dl_id(slot: &Arc<Mutex<Option<i64>>>) -> Option<i64> {
    for _ in 0..100 {
        if let Some(id) = *slot.lock().unwrap_or_else(|e| e.into_inner()) {
            return Some(id);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

/// 下载进度/终态跟进：非终态 500ms 节流写字节数；终态写状态并摘表。
fn track_download(it: &mut DownloadItem) {
    let item_id = it.id();
    let (db_id, last_progress, finished) = {
        let tracks = dl_tracks().lock().unwrap_or_else(|e| e.into_inner());
        let Some(t) = tracks.get(&item_id) else {
            return;
        };
        (t.db_id.clone(), t.last_progress.clone(), t.finished.clone())
    };
    if finished.load(Ordering::Relaxed) {
        return;
    }

    let completed = it.is_complete() != 0;
    let canceled = it.is_canceled() != 0;
    let interrupted = it.is_interrupted() != 0;
    let is_final = completed || canceled || interrupted;
    let received = it.received_bytes();
    let total = it.total_bytes();

    if !is_final {
        let mut last = last_progress.lock().unwrap_or_else(|e| e.into_inner());
        if last.elapsed() < std::time::Duration::from_millis(500) {
            return;
        }
        *last = Instant::now();
    }

    let state = if completed {
        "completed"
    } else if canceled {
        "cancelled"
    } else if interrupted {
        "failed"
    } else {
        ""
    };

    if let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() {
        let state = state.to_string();
        tauri::async_runtime::spawn(async move {
            let Some(db_id) = wait_dl_id(&db_id).await else { return };
            if state.is_empty() {
                if let Err(e) = crate::db::downloads::update_download_bytes(
                    &pool,
                    db_id,
                    received,
                    Some(total),
                )
                .await
                {
                    log::warn!("[cef] 下载进度写库失败 id={db_id}: {e}");
                }
            } else if let Err(e) = crate::db::downloads::update_download_state(
                &pool, db_id, &state, Some(received),
            )
            .await
            {
                log::warn!("[cef] 下载状态写库失败 id={db_id}: {e}");
            }
            emit_downloads_updated();
            request_downloads_refresh();
        });
    }

    if is_final {
        finished.store(true, Ordering::Relaxed);
        dl_tracks()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&item_id);
        log::info!("[cef] 下载终态: item={item_id} state={state} bytes={received}/{total}");
    }
}

/// 打开下载文件所在目录（只允许下载目录内的路径，防任意路径打开）。
fn open_download_location(download_id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    tauri::async_runtime::spawn(async move {
        let path = sqlx::query_scalar::<_, Option<String>>(
            "SELECT path FROM downloads WHERE id = ?",
        )
        .bind(download_id)
        .fetch_one(&pool)
        .await
        .ok()
        .flatten();
        let Some(path) = path else {
            log::warn!("[cef] 打开下载目录失败：记录 {download_id} 无路径");
            return;
        };
        let file = std::path::PathBuf::from(&path);
        let dir = crate::engine::platform::resolve_download_dir();
        let dir_path = std::path::PathBuf::from(&dir);
        if !file.starts_with(&dir_path) {
            log::warn!("[cef] 拒绝打开下载目录外路径: {path}");
            return;
        }
        let target = file.parent().map(|p| p.to_path_buf()).unwrap_or(dir_path);
        let _ = app.run_on_main_thread(move || {
            if let Err(e) = std::process::Command::new("xdg-open").arg(&target).spawn() {
                log::warn!("[cef] 打开目录失败 {}: {e}", target.display());
            }
        });
    });
}

/// 重新下载：按记录里的 URL 再走一次 `start_download`。
fn retry_download(download_id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    let Some(app) = crate::cef_cmd::app_handle() else { return };
    tauri::async_runtime::spawn(async move {
        let url = sqlx::query_scalar::<_, Option<String>>("SELECT url FROM downloads WHERE id = ?")
            .bind(download_id)
            .fetch_one(&pool)
            .await
            .ok()
            .flatten();
        let Some(url) = url else {
            log::warn!("[cef] 重新下载失败：记录 {download_id} 无 URL");
            return;
        };
        let _ = app.run_on_main_thread(move || {
            BROWSERS.with(|m| {
                if let Some(b) = m.borrow().get(CONTENT_ID) {
                    if let Some(host) = b.host() {
                        host.start_download(Some(&CefString::from(url.as_str())));
                    }
                }
            });
        });
    });
}

/// 删除下载记录（不删磁盘文件，与主应用 `delete_download` 语义一致）。
fn delete_download_record(download_id: i64) {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else { return };
    tauri::async_runtime::spawn(async move {
        match sqlx::query("DELETE FROM downloads WHERE id = ?")
            .bind(download_id)
            .execute(&pool)
            .await
        {
            Ok(r) if r.rows_affected() > 0 => {
                log::info!("[cef] 下载记录已删除 id={download_id}");
                emit_downloads_updated();
                request_downloads_refresh();
            }
            Ok(_) => log::warn!("[cef] 下载记录不存在 id={download_id}"),
            Err(e) => log::warn!("[cef] 删除下载记录失败 id={download_id}: {e}"),
        }
    });
}

wrap_download_handler! {
    struct ShellDownloadHandler;

    impl DownloadHandler {
        /// 下载开始前决定落盘路径：直接存下载目录，不弹保存对话框
        /// （CEF 的 Views 模式没有系统保存对话框，不接管会静默失败）。
        ///
        /// 与原 WebKit 设计对齐（`tabs/backend/layout/gtk.rs`）：目录走用户设置、
        /// 同名文件加 (1)(2) 后缀、写 downloads 表并发 `downloads-updated`。
        fn on_before_download(
            &self,
            _browser: Option<&mut Browser>,
            download_item: Option<&mut DownloadItem>,
            suggested_name: Option<&CefString>,
            callback: Option<&mut BeforeDownloadCallback>,
        ) -> std::os::raw::c_int {
            let item_id = download_item.as_ref().map(|it| it.id()).unwrap_or(0);
            let url = download_item
                .as_ref()
                .map(|it| CefStringUtf16::from(&it.url()).to_string())
                .filter(|s| !s.is_empty());
            let name = suggested_name
                .map(|s| s.to_string())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    crate::tabs::util::download_filename_from_url(url.as_deref())
                });
            let safe = sanitize_filename(&name);
            let dir = crate::engine::platform::resolve_download_dir();
            let _ = std::fs::create_dir_all(&dir);
            let path = crate::tabs::backend::unique_download_path(&dir, &safe);
            log::info!("[cef] 下载开始: {name} -> {path}");
            register_download(item_id, name, url, std::path::PathBuf::from(&path));
            if let Some(cb) = callback {
                cb.cont(Some(&CefString::from(path.as_str())), 0);
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
                track_download(it);
            }
        }
    }
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

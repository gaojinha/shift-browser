//! CEF 应用级对象：进程初始化、上下文就绪通知、消息泵。
//!
//! 设计要点（P0 实测结论，别踩回头路）：
//!
//! 1. **GTK 必须先锁到 X11**。CEF 用 X11 子窗口嵌入（`set_as_child`），
//!    Wayland 后端拿不到 XID。调用方必须在 `initialize()` 之前：
//!    - `std::env::set_var("GDK_BACKEND", "x11")`
//!    - `gtk::init()`
//!
//!    晚一步 GTK 就被 CEF 带到 Wayland，扳不回来。
//!
//! 2. **消息泵走 `do_message_loop_work()`**。Tauri/tao 自己占着 GTK 主循环，
//!    不能再调 `cef::run_message_loop()`（会抢主循环）。用 glib 定时器泵即可。
//!
//! 3. **回调里绝不 panic**。CEF 回调是 `extern "C"`，panic 穿过 FFI 直接 abort 进程。

use cef::ImplCommandLine;
use cef::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// CEF 上下文是否已就绪（`on_context_initialized` 之后为 true）
static CONTEXT_READY: AtomicBool = AtomicBool::new(false);

/// 上下文就绪前登记的回调，就绪后按序执行
type ReadyCallbacks = OnceLock<Mutex<Vec<Box<dyn FnOnce() + Send>>>>;

static READY_CALLBACKS: ReadyCallbacks = OnceLock::new();

/// 进程级只初始化一次的标记
static INIT_ONCE: OnceLock<bool> = OnceLock::new();

/// 登记"CEF 上下文就绪"回调。
///
/// 若已就绪，**立即**在当前线程执行；否则排队，等 `on_context_initialized` 触发。
/// P2 标签体系接入前调用方暂未使用（当前窗口创建走 cef_cmd 直接排队），预留。
#[allow(dead_code)]
pub fn on_context_ready<F: FnOnce() + Send + 'static>(f: F) {
    if CONTEXT_READY.load(Ordering::SeqCst) {
        f();
        return;
    }
    READY_CALLBACKS
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Box::new(f));
}

/// 上下文是否已就绪
pub fn is_context_ready() -> bool {
    CONTEXT_READY.load(Ordering::SeqCst)
}

/// 初始化 CEF。同一进程重复调用只生效一次。
///
/// `resources_dir` / `locales_dir`：CEF 的 `*.pak`、`icudtl.dat`、`locales/` 所在目录
/// `cache_path`：用户数据目录（Cookie/缓存），建议放在应用数据目录下
pub fn initialize(resources_dir: &str, locales_dir: &str, cache_path: &str) -> Result<(), String> {
    if INIT_ONCE.get().is_some() {
        return if CONTEXT_READY.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err("CEF 初始化已在进行中".into())
        };
    }

    // 载入 CEF API 版本（必须在任何 CEF 调用之前）
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);

    let args = cef::args::Args::new();

    let mut app = ShiftCefApp::new();

    let settings = Settings {
        // 沙箱：当前用预编译 CEF，chrome-sandbox 未 setuid，先关掉。
        // 指纹与沙箱无关（TLS/JS 指纹在渲染进程，沙箱只是隔离），
        // 后续打包时把 chrome-sandbox 装上 setuid 再开。
        no_sandbox: 1,
        // 不做离屏渲染：我们用 windowed 模式（X11 子窗口）
        windowless_rendering_enabled: 0,
        // F-CEF-24d：CEF 自跑消息循环（独立 UI 线程）。
        // 旧的固定 50ms 泵（GTK 主循环驱动）会让 CEF 的 GLib X11 source 忙轮询，
        // 主进程 CPU 64–85%；自跑后实测降到个位数。线程安全前提见
        // F-CEF-24b（状态全局锁）+ 24c（句柄调用投递 CEF UI 线程）。
        multi_threaded_message_loop: 1,
        external_message_pump: 0,
        // 调试端口（仅本地排查用；不设环境变量时关闭）
        remote_debugging_port: std::env::var("SHIFT_CEF_DEBUG_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        resources_dir_path: CefString::from(resources_dir),
        locales_dir_path: CefString::from(locales_dir),
        root_cache_path: CefString::from(cache_path),
        ..Default::default()
    };

    let ok = cef::initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    );

    if ok != 1 {
        return Err(format!("cef::initialize 返回 {ok}"));
    }

    let _ = INIT_ONCE.set(true);
    Ok(())
}

/// 消息泵：由调用方在 GTK 主循环里定时调用（建议 10ms 一次）。
///
/// 必须从 **UI 线程** 调用。
///
/// F-CEF-24d 起 `multi_threaded_message_loop=1`（CEF 自跑消息循环），本函数**无调用方**，
/// 保留供回退用：回退时把 `multi_threaded_message_loop` 改回 0，并在 cef_cmd 的 init
/// 里重新挂 50ms 定时器调它。`dead_code` 豁免是有意的。
#[allow(dead_code)]
pub fn pump() {
    if !CONTEXT_READY.load(Ordering::SeqCst) {
        return;
    }
    // 单次最多处理一批，避免卡住 GTK 主循环
    cef::do_message_loop_work();
}

/// 关闭 CEF（进程退出前调用）。P2 接入应用退出流程前预留。
#[allow(dead_code)]
pub fn shutdown() {
    if CONTEXT_READY.load(Ordering::SeqCst) {
        cef::shutdown();
    }
}

// ============================================================
// App / BrowserProcessHandler 实现
// ============================================================

wrap_app! {
    pub struct ShiftCefApp;

    impl App {
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ShiftBrowserProcessHandler::new())
        }

        /// 强制 CEF 走 X11 后端（关键）。
        ///
        /// 实测（2026-10-20）：不指定时 CEF 会跟着桌面会话选 Wayland
        /// （日志里全是 `ui/ozone/platform/wayland/...`），此时
        /// `set_as_child()` 传的 X11 parent 被完全忽略，`window_handle()`
        /// 返回 1，既拿不到子窗口也改不了尺寸。
        ///
        /// 补上 `--disable-gpu` 是因为 X11 + GPU 组合下窗口创建会失败
        /// （实测视口 0×0）。软件渲染够用，等后续再评估。
        fn on_before_command_line_processing(
            &self,
            _process_type: Option<&CefString>,
            command_line: Option<&mut CommandLine>,
        ) {
            let Some(cmd) = command_line else {
                return;
            };
            cmd.append_switch_with_value(
                Some(&CefString::from("ozone-platform")),
                Some(&CefString::from("x11")),
            );
            // GPU：`--disable-gpu` 会让 WebGL 彻底不可用（2026-10-25 实测
            // canvas.getContext('webgl') 返回 null），而真 Chrome 有 WebGL，
            // 缺失既是可识别特征、也让地图/3D 站不可用。
            // 改走 Vulkan 后端 + SwiftShader 软件光栅：CEF 预编译包自带
            // libvk_swiftshader.so 与 libvulkan.so.1，不依赖系统 GL。
            // （试过 `--use-gl=egl`：窗口正常但 WebGL 仍 null，缺 ANGLE 的 GL 前端库；
            //   要拿到与真 Chrome 完全相同的 renderer 字符串，需补 libEGL.so/libGLESv2.so
            //   后改 `--use-angle=gl`，见指纹报告 §8。）
            // 2026-10-28 实测：Vulkan 后端在本机（Mesa/驱动组合）初始化失败——
            // stdio.log 里 `Internal Vulkan error (-3)` + `EGL_NOT_INITIALIZED` +
            // `GLDisplayEGL::Initialize failed`，随后导航到真实网站整窗卡死
            // （GNOME 报「无响应」，退出码 137 = 用户强制退出）。
            // 改用 ANGLE 自带的 SwiftShader 纯软件光栅：不依赖系统 GL/Vulkan，
            // 代价是 WebGL 走 CPU（速度慢，但可用且稳定）。
            cmd.append_switch_with_value(
                Some(&CefString::from("use-angle")),
                Some(&CefString::from("swiftshader")),
            );

            // 给 X11 窗口一个 WM_CLASS：Mutter 对没有 WM_CLASS 的窗口会当成
            // "待接管"状态（实测 WM_STATE=Iconic、永不映射）
            cmd.append_switch_with_value(
                Some(&CefString::from("class")),
                Some(&CefString::from("Shift")),
            );
            log::info!("[cef] 命令行已追加 --ozone-platform=x11 + ANGLE SwiftShader（软件光栅）");
        }
    }
}

wrap_browser_process_handler! {
    // 浏览器进程 handler：只关心"上下文就绪"这一个时机
    pub struct ShiftBrowserProcessHandler;

    impl BrowserProcessHandler {
        fn on_context_initialized(&self) {
            // 不能 panic：本函数处在 extern "C" 回调栈上
            CONTEXT_READY.store(true, Ordering::SeqCst);

            let callbacks = READY_CALLBACKS
                .get_or_init(|| Mutex::new(Vec::new()))
                .lock()
                .map(|mut list| std::mem::take(&mut *list))
                .unwrap_or_default();

            for cb in callbacks {
                // 单个回调出错不能拖垮整个初始化
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(cb)).is_err() {
                    log::error!("[cef] 上下文就绪回调 panic（已隔离）");
                }
            }
        }

    }
}

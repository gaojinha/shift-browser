//! CEF 标签页的 Tauri 侧接入（实验阶段，`--features cef-engine` 才编译）
//!
//! 线程模型：
//! - CEF 的 `Browser` 句柄**只能在 GTK 主线程**使用，全部存在 `thread_local` 里；
//! - Tauri 命令跑在异步线程，对 CEF 的操作一律通过 `app.run_on_main_thread()`
//!   投递，闭包里只捕获 `String`，绝不捕获 `Browser`；
//! - CEF 回调产生的事件走全局 `mpsc` 通道，`tab_id` 随事件一起带出。
//!
//! 当前阶段目标：在 Tauri 应用里开出一个由 CEF 渲染的窗口，跑通整条链路。
//! P2 再接入现有标签/分屏体系。

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Manager};

// gtk-rs 的窗口方法（add/show_all/window/allocation）都在 prelude trait 里
use gtk::prelude::*;
// CEF 的 Browser/Frame 方法在 Impl* trait 里
use cef::{ImplBrowser, ImplCommandLine, ImplFrame};

use crate::engine::cef::{
    app as cef_app, browser as cef_browser, shell as cef_shell, toolbar as cef_toolbar,
};
use cef_browser::CefEvent;

/// 全局事件发送端（CEF 回调线程 → Tauri 侧）
static EVENT_TX: OnceLock<Sender<(String, CefEvent)>> = OnceLock::new();
/// 全局事件接收端
static EVENT_RX: Mutex<Option<Receiver<(String, CefEvent)>>> = Mutex::new(None);

/// 主进程的 AppHandle（CEF 回调线程发 Tauri 事件用）。
///
/// CEF 回调拿不到 `AppHandle`（`extern "C"` 边界），但下载完成/进度要发
/// `DOWNLOADS_UPDATED` 让主界面弹窗与状态栏角标刷新，所以在 `init` 里存一份。
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// 取全局 AppHandle（未初始化时返回 None，调用方按"无人监听"降级，不 panic）。
pub(crate) fn app_handle() -> Option<&'static AppHandle> {
    APP_HANDLE.get()
}

/// **子进程入口**：CEF 用 `/proc/self/exe` 拉起渲染/GPU/zygote 子进程，
/// 子进程的命令行带 `--type=xxx`。命中时必须在这里跑完 CEF 子进程逻辑并**直接退出**，
/// 绝不能再往下走 Tauri 初始化（否则就是无限套娃 + 单实例误判）。
///
/// 返回 `true` 表示当前进程是 CEF 子进程且已处理完毕，调用方应立即 return。
pub fn try_run_subprocess() -> bool {
    use cef::{args::Args, execute_process, CefString};

    // 必须在任何 CEF 调用之前
    let _ = cef::api_hash(cef::sys::CEF_API_VERSION_LAST, 0);

    let args = Args::new();
    let Some(cmd_line) = args.as_cmd_line() else {
        return false;
    };

    let type_switch = CefString::from("type");
    if cmd_line.has_switch(Some(&type_switch)) != 1 {
        // 主进程
        return false;
    }

    let ret = execute_process(Some(args.as_main_args()), None, std::ptr::null_mut());
    // ret >= 0：子进程已跑完；< 0：类型未知，交给主流程
    ret >= 0
}

/// CEF profile / 缓存根目录（**单一来源**）。
///
/// 优先级：`SHIFT_CEF_CACHE_DIR`（非空）→ 应用缓存目录下的 `cef/`。
///
/// 为什么必须是单一来源：CEF 的 Cookies/LocalStorage 落在这个目录里；工具栏页
/// 与启动页也写在它的 `shell/` 子目录。2026-10-26 实测踩坑——`init` 读了 env、
/// `open_shell_on_main` 自己又算了一次 `app_cache_dir()`，结果 profile 用了临时目录、
/// 工具栏页仍读 `~/.cache/com.shiftbrowser.app/shell/toolbar.html`（旧页面/旧会话），
/// 真机验证直接不可信。两个调用点从此只许调本函数。
fn resolve_cef_cache_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = match std::env::var("SHIFT_CEF_CACHE_DIR") {
        Ok(d) if !d.trim().is_empty() => std::path::PathBuf::from(d),
        _ => app
            .path()
            .app_cache_dir()
            .map(|p| p.join("cef"))
            .map_err(|e| format!("拿不到缓存目录: {e}"))?,
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("建缓存目录失败: {e}"))?;
    Ok(dir)
}

/// 初始化 CEF（应用启动时调用一次）
pub fn init(app: &AppHandle) -> Result<(), String> {
    // GDK_BACKEND 已在 main() 最早期设置（必须早于任何 gtk_init）
    let _ = gtk::init();
    // CEF 回调线程发事件（下载进度/完成）要用的全局句柄
    let _ = APP_HANDLE.set(app.clone());

    // 2) 资源目录：生产期由打包流程注入 SHIFT_CEF_RESOURCES；
    //    开发期取**可执行文件所在目录**——cef crate 构建时把 libcef.so / *.pak /
    //    icudtl.dat / locales 全都放在 target/<profile>/ 下，与二进制同目录。
    //    （原先写死 /tmp/cef-rs/... 是本地 path 依赖时代的路径，换 crates.io 后已不存在，
    //      表现为启动 4 秒后 SIGTRAP：cef::initialize 拿不到资源目录。）
    let resources = std::env::var("SHIFT_CEF_RESOURCES").unwrap_or_else(|_| {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_string_lossy().into_owned()))
            .unwrap_or_else(|| ".".to_string())
    });

    // 缓存/profile 目录：默认用应用缓存目录，可用 SHIFT_CEF_CACHE_DIR 覆盖。
    // 单一来源见 `resolve_cef_cache_dir`——工具栏页/启动页也必须走同一份，
    // 否则会出现「profile 隔离了、工具栏页还读真实 ~/.cache」的半隔离（2026-10-26 实测）。
    let cache = resolve_cef_cache_dir(app)?;

    cef_app::initialize(
        &resources,
        &format!("{resources}/locales"),
        cache.to_string_lossy().as_ref(),
    )?;

    // 3) 事件通道
    let (tx, rx) = channel::<(String, CefEvent)>();
    let _ = EVENT_TX.set(tx);
    *EVENT_RX.lock().unwrap_or_else(|e| e.into_inner()) = Some(rx);

    // 4) 消息泵：固定 50ms（2026-10-27 实测 10ms 也 100% CPU，根因是 CEF 的 GLib
    // X11 source 忙轮询，单独立卡优化；这里先保证窗口/输入可用）。
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(50), || {
        cef_app::pump();
        gtk::glib::ControlFlow::Continue
    });

    log::info!("[cef] 初始化完成，资源目录 {resources}");

    // F-CEF-11：预计算页面注入脚本（on_load_start 是同步回调，不能 await）。
    // init 跑在 setup 的主线程而非 tokio worker，block_on 安全；改设置需重开窗口才生效。
    {
        use tauri::Manager;
        if let Some(db) = app.try_state::<crate::db::Db>() {
            tauri::async_runtime::block_on(async {
                // 站点 UA 覆盖表要先载入，否则 __SHIFT_SITE_OVERRIDES__ 为空
                crate::useragent::load(&db.pool).await;
                if let Err(e) = crate::extensions::cef_inject::build_and_cache(&db).await {
                    log::warn!("[cef-inject] 预计算失败，页面仅有基础标记: {e}");
                }
            });
        } else {
            log::warn!("[cef-inject] 拿不到数据库状态，跳过扩展注入");
        }
    }

    // 书签：从主应用数据库读真实书签，推给 CEF 工具栏页面。
    // 为什么异步：sqlx 只有 async API，而 init 在 CEF/GTK 主线程；
    // 查完用 glib idle 派回主线程再动 UI（execute_java_script 必须在主线程）。
    {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            use tauri::Manager;
            let Some(db) = handle.try_state::<crate::db::Db>() else {
                log::warn!("[cef] 拿不到数据库状态，书签栏留空");
                return;
            };
            match sqlx::query_as::<_, (String, String)>(
                "SELECT title, url FROM bookmarks ORDER BY sort_order, id LIMIT 200",
            )
            .fetch_all(&db.pool)
            .await
            {
                Ok(rows) => crate::engine::cef::shell::set_bookmarks(rows),
                Err(e) => log::warn!("[cef] 读书签失败: {e}"),
            }
        });
    }

    // ---- CEF 单栈窗口（唯一入口）----
    // 2026-10-26 高哥拍板：旧的 WebKit 主界面准备弃用，CEF 构建启动后只开这一个窗口。
    // SHIFT_CEF_SHELL_URL 指定初始页；没设就用内置启动页（newtab）。
    // 需要主界面做对比/回退时设 SHIFT_WEBKIT_UI=1（bootstrap 侧据此显示主窗口）。
    // 本文件只在 `--features cef-engine` 下编译，这里无需再判 feature。
    {
        let url = std::env::var("SHIFT_CEF_SHELL_URL").ok();
        log::info!("[cef] 单栈窗口准备启动，初始页={url:?}");
        let app = app.clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            if !cef_app::is_context_ready() {
                return gtk::glib::ControlFlow::Continue;
            }
            match open_shell_on_main(&app, url.as_deref()) {
                Ok(()) => log::info!("[cef] 单栈窗口已打开"),
                Err(e) => log::error!("[cef] 单栈窗口打开失败: {e}"),
            }
            gtk::glib::ControlFlow::Break
        });
    }

    // 实验用：设置 SHIFT_CEF_TEST_URL 后，启动 3 秒自动开一个 CEF 窗口做端到端验证
    if let Ok(url) = std::env::var("SHIFT_CEF_TEST_URL") {
        log::info!("[cef] 检测到测试 URL，3 秒后自动开窗: {url}");
        let app = app.clone();
        gtk::glib::timeout_add_seconds_local_once(3, move || match cef_open_window(app, url) {
            Ok(id) => log::info!("[cef] 测试窗口已创建: {id}"),
            Err(e) => log::error!("[cef] 测试窗口创建失败: {e}"),
        });
    }

    Ok(())
}

/// 打开一个由 CEF 渲染的独立窗口，返回 tab id。
///
/// 用**纯 GTK 窗口**而不是 Tauri 的 Window：CEF 需要拿到一个已 realize 的
/// GdkWindow 的 XID，Tauri 的窗口内部结构（tao 的 GtkBox）不可控，
/// `GtkFixed` 挂上去拿不到 window。embed 实验已证明纯 GTK 路径可行。
#[tauri::command]
pub fn cef_open_window(app: AppHandle, url: String) -> Result<String, String> {
    let tab_id = format!("cef-{}", uuid::Uuid::new_v4().simple());

    // GTK 只能在主线程操作
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let id = tab_id.clone();
    app.run_on_main_thread(move || {
        let r = create_gtk_cef_window(&id, &url);
        let _ = tx.send(r);
    })
    .map_err(|e| format!("投递主线程失败: {e}"))?;

    rx.recv().map_err(|e| format!("主线程无响应: {e}"))??;
    Ok(tab_id)
}

/// 创建纯 GTK 窗口并在其中嵌入 CEF（必须在主线程调用）
///
/// **必须用 `GtkDrawingArea` 做容器，不能用 `GtkFixed`。**
/// `GtkFixed` 是无窗口 widget，`window()` 返回的是顶层窗口的 GdkWindow，
/// CEF 拿它当 parent 时不会创建 X11 子窗口（`window_handle()` 返回 1，
/// 尺寸写死、无法 resize）。`GtkDrawingArea` 有独立的 GdkWindow，
/// CEF 才会在其下建立真正的 X11 子窗口——embed 实验用的就是这个。
fn create_gtk_cef_window(tab_id: &str, url: &str) -> Result<(), String> {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("Shift · CEF");
    window.set_default_size(1280, 860);

    // DrawingArea 直接铺满窗口（Window 是 Bin，只能有一个子 widget）
    let area = gtk::DrawingArea::new();
    area.set_size_request(1280, 860);
    window.add(&area);
    window.show_all();

    let gdk_window = area
        .window()
        .ok_or_else(|| "DrawingArea 没有 GdkWindow（未 realize）".to_string())?;

    let xid = {
        use gdkx11::X11Window;
        let x11 = gdk_window
            .clone()
            .downcast::<X11Window>()
            .map_err(|w| format!("不是 X11 窗口（实际 {:?}）", w.type_()))?;
        x11.xid()
    };

    let alloc = area.allocation();
    let (w, h) = (alloc.width(), alloc.height());

    log::info!("[cef] 窗口={tab_id} XID=0x{xid:x} 尺寸={w}x{h}");

    let tx = EVENT_TX
        .get()
        .ok_or_else(|| "CEF 未初始化（EVENT_TX 为空）".to_string())?
        .clone();

    // 异步创建：句柄稍后在 LifeSpanHandler::on_after_created 里登记进 BROWSERS
    if !cef_browser::create_browser(tab_id.to_string(), url, xid, (0, 0, w, h), tx) {
        return Err("browser_host_create_browser 返回失败".to_string());
    }

    // ---- 尺寸跟随：窗口 resize 时同步 DrawingArea 与 CEF 子窗口 ----
    let id_resize = tab_id.to_string();
    let area_for_resize = area.clone();
    window.connect_size_allocate(move |_, alloc| {
        let (w, h) = (alloc.width(), alloc.height());
        // 让 DrawingArea 跟着窗口走（否则 CEF 子窗口会被裁剪）
        area_for_resize.set_size_request(w, h);

        cef_browser::BROWSERS.with(|tabs| {
            if let Some(b) = tabs.borrow().get(&id_resize) {
                cef_browser::resize(b, w, h);
            }
        });
    });

    // ---- 测试用：SHIFT_CEF_TEST_RESIZE=900x560，8 秒后主动调 GTK resize ----
    if let Ok(spec) = std::env::var("SHIFT_CEF_TEST_RESIZE") {
        if let Some((ws, hs)) = spec.split_once('x') {
            if let (Ok(w), Ok(h)) = (ws.trim().parse::<i32>(), hs.trim().parse::<i32>()) {
                let id = tab_id.to_string();
                gtk::glib::timeout_add_seconds_local_once(8, move || {
                    log::info!("[cef] 测试：直接 resize CEF 到 {w}x{h}");
                    cef_browser::BROWSERS.with(|tabs| {
                        if let Some(b) = tabs.borrow().get(&id) {
                            cef_browser::resize(b, w, h);
                        } else {
                            log::warn!("[cef] 测试 resize：BROWSERS 里没有该标签");
                        }
                    });
                });
            }
        }
    }

    // ---- 关闭清理：窗口关掉时销毁 CEF 浏览器，避免句柄残留 ----
    let id_close = tab_id.to_string();
    window.connect_delete_event(move |_, _| {
        let browser = cef_browser::BROWSERS.with(|tabs| tabs.borrow_mut().remove(&id_close));
        if let Some(b) = browser {
            cef_browser::close(&b, true);
        }
        gtk::glib::Propagation::Proceed
    });

    log::info!("[cef] 浏览器已嵌入标签页 {tab_id}");
    Ok(())
}

/// 打开单栈窗口（窗口、工具栏、内容页全在 CEF 内）。
///
/// 必须在主线程调用；`app` 只用来取缓存目录。
/// 同步读书签（生成工具栏页面用）。
///
/// CEF 侧的异步推送实测不可靠（glib 定时器在回调链里不触发），
/// 改成生成页面时同步查一次——阻塞几十毫秒可接受，换来确定性。
fn cef_bookmarks_json(app: &AppHandle) -> String {
    use tauri::Manager;
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "{\"folders\":[],\"bookmarks\":[]}".to_string();
    };
    let pool = db.pool.clone();
    let data = tauri::async_runtime::block_on(async move {
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
    match data {
        Ok((bms, folders)) => {
            log::info!(
                "[cef] 书签同步读取 {} 条 + {} 个文件夹",
                bms.len(),
                folders.len()
            );
            let bm_json: Vec<serde_json::Value> = bms
                .iter()
                .map(|(id, t, u, f)| {
                    serde_json::json!({ "id": id, "title": t, "url": u, "folderId": f })
                })
                .collect();
            let f_json: Vec<serde_json::Value> = folders
                .iter()
                .map(|(id, n, p)| serde_json::json!({ "id": id, "name": n, "parentId": p }))
                .collect();
            serde_json::json!({ "folders": f_json, "bookmarks": bm_json }).to_string()
        }
        Err(e) => {
            log::warn!("[cef] 读书签失败: {e}");
            "{\"folders\":[],\"bookmarks\":[]}".to_string()
        }
    }
}

/// 读下载记录给工具栏下载面板（启动时嵌进页面；后续由 shell 侧推送刷新）。
///
/// 与书签同理：CEF 的推送时机不可靠，生成页面时同步查一次最稳。
/// 阻塞几十毫秒可接受（只在开窗时执行一次）。
fn cef_downloads_json(app: &AppHandle) -> String {
    use tauri::Manager;
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    let pool = db.pool.clone();
    match tauri::async_runtime::block_on(async move {
        crate::db::downloads::list_downloads(&pool).await
    }) {
        Ok(list) => crate::engine::cef::shell::downloads_panel_json(&list),
        Err(e) => {
            log::warn!("[cef] 读下载列表失败: {e}");
            "[]".to_string()
        }
    }
}

fn open_shell_on_main(app: &AppHandle, url: Option<&str>) -> Result<(), String> {
    // 必须与 `init` 共用同一份解析：工具栏页/启动页要和 profile 同住一个隔离目录。
    let cache = resolve_cef_cache_dir(app)?;
    let bookmarks_json = cef_bookmarks_json(app);
    crate::engine::cef::passwords_ui::init(app);
    crate::engine::cef::certificates_ui::init(app);
    crate::engine::cef::groups_ui::init(app);
    crate::engine::cef::downloads_ui::init(app);
    crate::engine::cef::userscripts_ui::init(app);
    let passwords_json = crate::engine::cef::passwords_ui::passwords_json(app);
    let downloads_json = cef_downloads_json(app);
    let userscripts_json = crate::engine::cef::userscripts_ui::userscripts_json(app);
    let toolbar_url = cef_toolbar::ensure_toolbar_html(
        &cache,
        &bookmarks_json,
        &passwords_json,
        &downloads_json,
        &userscripts_json,
    )?;
    // 初始页：显式 URL 优先；没给就用 CEF 内置启动页（本地文件，不依赖网络）。
    let content_url = match url {
        Some(u) if !u.trim().is_empty() => u.to_string(),
        _ => crate::engine::cef::newtab::ensure_newtab_html(&cache.join("shell"))?,
    };
    cef_shell::open(&content_url, &toolbar_url)?;

    // 窗口创建回调里调 show() 会卡死；出回调后立刻用 X11 直接映射
    cef_shell::force_show();

    // 书签/标签栏补推：glib 定时器在 force_show 里注册后实测不触发，
    // 改在这里用 GTK 主循环的 timeout 直接挂（同一线程、同一 main context）。
    for delay in [1500u64, 3500, 6000] {
        gtk::glib::timeout_add_local_once(std::time::Duration::from_millis(delay), || {
            cef_shell::refresh_bookmarks_ui();
            cef_shell::refresh_tabbar();
        });
    }
    Ok(())
}

/// Tauri 命令：打开 CEF 单栈窗口
#[tauri::command]
pub fn cef_open_shell(app: AppHandle, url: String) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let r = open_shell_on_main(&app2, Some(&url));
        let _ = tx.send(r);
    })
    .map_err(|e| format!("投递主线程失败: {e}"))?;
    rx.recv().map_err(|e| format!("主线程无响应: {e}"))?
}

/// 让指定 CEF 标签导航
#[tauri::command]
pub fn cef_navigate(app: AppHandle, tab_id: String, url: String) -> Result<(), String> {
    app.run_on_main_thread(move || {
        cef_browser::BROWSERS.with(|tabs| {
            if let Some(browser) = tabs.borrow().get(&tab_id) {
                if let Some(frame) = browser.main_frame() {
                    let u = cef::CefString::from(url.as_str());
                    frame.load_url(Some(&u));
                }
            }
        });
    })
    .map_err(|e| format!("投递主线程失败: {e}"))
}

/// 关闭指定 CEF 标签
#[tauri::command]
pub fn cef_close_window(app: AppHandle, tab_id: String) -> Result<(), String> {
    app.run_on_main_thread(move || {
        let browser = cef_browser::BROWSERS.with(|tabs| tabs.borrow_mut().remove(&tab_id));
        if let Some(b) = browser {
            cef_browser::close(&b, false);
        }
    })
    .map_err(|e| format!("投递主线程失败: {e}"))
}

/// 取走最近收到的 CEF 事件（调试用）
#[tauri::command]
pub fn cef_poll_events() -> Vec<String> {
    let mut out = Vec::new();
    if let Some(rx) = EVENT_RX.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        while let Ok((tab_id, ev)) = rx.try_recv() {
            out.push(format!("{tab_id}: {ev:?}"));
        }
    }
    out
}

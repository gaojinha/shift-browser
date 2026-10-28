// 应用启动装配（R2-1b 拆分：setup 主体从 lib.rs 抽出）
//
// 职责：Tauri `.setup()` 闭包的全部逻辑——敏感文件加固、内存压力治理、
// 数据库初始化、锁屏、TabManager/缓存/白名单载入、托盘、外部链接打开等。
// 失败路径（如数据库初始化失败）用本地 show_fatal_dialog 弹 GTK 原生窗后返回 Err。

use tauri::Manager;

// setup 的子步骤（按域拆出以控制本文件行数；调用顺序即原启动顺序，不可乱）
#[path = "bootstrap/caches.rs"]
mod caches;
#[path = "bootstrap/guards.rs"]
mod guards;
#[path = "bootstrap/tray_setup.rs"]
mod tray_setup;

/// 致命错误提示：setup 失败路径（如数据库初始化失败）下 Vue UI 尚不可用，
/// 只能走 GTK 原生层弹窗；与启动锁屏同模式——setup 在 GTK 主线程执行，
/// `run()` 模态阻塞，用户确认后调用方返回 Err 退出。
#[cfg(target_os = "linux")]
fn show_fatal_dialog(app: &tauri::AppHandle, msg: &str) {
    use gtk::prelude::*;
    let parent = app.get_window("main").and_then(|w| w.gtk_window().ok());
    let dialog = gtk::MessageDialog::new(
        parent.as_ref(),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Close,
        msg,
    );
    dialog.set_title("Shift Browser 启动失败");
    dialog.run();
    dialog.close();
}

/// 非 Linux 平台暂无 GUI，致命信息落 stderr
#[cfg(not(target_os = "linux"))]
fn show_fatal_dialog(_app: &tauri::AppHandle, msg: &str) {
    eprintln!("[ShiftBrowser] 致命错误: {msg}");
}

/// setup 闭包体（R2-1b 从 lib.rs 整体迁入）。
///
/// 返回 Err 会中止启动（数据库是全应用硬依赖，初始化失败必须立即终止）。
/// 创建主窗口（仅 WebKit 模式；CEF 模式彻底不创建，不是 hide）。
///
/// 2026-10-27 高哥拍板：旧的主界面项目要停掉，CEF 构建只留单栈窗口。
/// 之前用 `visible:false + hide()` 只是藏起来，X11 窗口仍 mapped（xwininfo 可见），
/// 所以改成从 `tauri.conf.json` 删窗口声明、这里按模式创建。
pub(crate) fn create_main_window(
    app: &mut tauri::App,
) -> Result<(), Box<dyn std::error::Error>> {
    let url = tauri::Url::parse("shift://main")?;
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::CustomProtocol(url))
        .title("Shift Browser")
        .inner_size(1280.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .resizable(true)
        .center()
        .decorations(false)
        .transparent(true)
        .build()?;
    Ok(())
}

pub(crate) fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // 安全（#1 · 2026-09-22）：敏感文件（cookie 库/WAL）启动即加固到 0600，
    // 并每 120s 重扫一次——WebKit 运行时会按 umask 重建这些文件。
    #[cfg(unix)]
    guards::spawn_sensitive_file_guard(app);
    // ===== 性能：WebKitGTK 内存压力治理 =====
    // 实测痛点：9 个标签时全部 WebKit 进程合计 3.75 GB（每标签一个渲染进程、
    // 单个 240-500MB，WebKitGTK 默认不主动回收）。
    // 方案：设置内存压力阈值，超过后 WebKit 分级回收（保守→严格→杀进程）。
    // 必须在使用任何 WebContext 之前设置（WebsiteDataManager 的静态全局设置）。
    #[cfg(target_os = "linux")]
    guards::setup_memory_pressure();

    // 锁屏是否启用过（解锁后通知前端重试初始化——锁屏期间 Db/TabManager 未就绪，
    // 主窗口 Vue 的 onMounted 命令调用全部失败，必须等 setup 完成后重试）
    // R2-1b：由 lock_dialog::show_startup_lock 返回（锁屏逻辑已迁至 ui/lock_dialog.rs）

    // 2026-09-19：GNOME 49 对无装饰窗口强制四角圆角，但 Tauri/wry 只把 WebView
    // 背景设为透明，GTK 窗口本身没设透明——圆角外露出黑色窗口背景（四个黑角）。
    // 手动设置 RGBA visual + app_paintable，让窗口真正透明，圆角外透出桌面。
    #[cfg(target_os = "linux")]
    guards::apply_window_transparency(app)?;

    // 初始化数据库（异步）
    let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

    log::info!("[ShiftBrowser] app_data_dir = {:?}", app_data_dir);
    log::info!("[ShiftBrowser] 目录存在: {}", app_data_dir.exists());

    // 注入 panic hook 的落盘目录（P0-4）：此后任何线程 panic 都会追加到 panic.log
    let _ = crate::PANIC_LOG_DIR.set(app_data_dir.clone());
    // 原生崩溃信号兜底（2026-10-19）：crash.log 与本目录同级，信号处理器直接写 fd
    crate::crash_signal::set_log_dir(&app_data_dir);
    // F-CEF-34：stdout/stderr 落盘。CEF（Chromium）的 CHECK 失败、渲染进程崩溃、
    // GPU 报错全走 stderr，GUI 启动时终端不可见——不重定向就一条都留不下。
    redirect_stdio_to_file(&app_data_dir);

    // 数据库是全应用硬依赖（TabManager/API/AI/设置全部 state::<Db>()），
    // 初始化失败必须**立即终止启动**——旧实现只记日志后继续，
    // 后续任意 state::<Db>() 访问都会无提示 panic（P0-3，2026-09-24）。
    let db = match tauri::async_runtime::block_on(crate::db::Db::init(app_data_dir.clone())) {
        Ok(db) => db,
        Err(e) => {
            log::error!("[ShiftBrowser] 数据库初始化失败，终止启动: {}", e);
            show_fatal_dialog(
                app.handle(),
                &format!(
                    "数据库初始化失败，Shift Browser 无法启动。\n\n错误信息：{e}\n数据目录：{}\n\n请检查磁盘空间与目录权限。若数据库文件损坏，可先备份目录后删除其中的 *.sqlite 文件再重启（本地未同步数据将丢失）。",
                    app_data_dir.display()
                ),
            );
            return Err(std::io::Error::other(format!("数据库初始化失败: {e}")).into());
        }
    };
    app.manage(db);
    log::info!("[ShiftBrowser] 数据库初始化成功");

    // 下载重启扫尾：上次进程退出时未完成的下载，其 WebKit 对象已消失，
    // 置为 interrupted，避免重启后永久“下载中”僵尸（前端给重新下载入口）
    {
        let db_state = app.state::<crate::db::Db>();
        match tauri::async_runtime::block_on(crate::db::downloads::sweep_interrupted_downloads(
            &db_state.pool,
        )) {
            Ok(0) => {}
            Ok(n) => log::info!("[ShiftBrowser] 下载扫尾：{} 条中断记录已标记", n),
            Err(e) => log::warn!("[ShiftBrowser] 下载扫尾失败（不阻断启动）: {}", e),
        }
    }

    // 密码库空闲自动锁定守护（R1）：默认 15 分钟无解锁活动即清除内存主密钥，
    // 设置 vault_idle_lock_minutes 可调，0 关闭
    crate::db::vault::spawn_idle_lock_watchdog(app.handle().clone());

    // 锁屏前预生成新标签页 + 后台刷新壁纸——锁屏 dialog 阻塞 setup 期间，
    // 这些异步任务照常跑（tokio 不依赖 GTK 主循环），解锁时 index.html 已就绪，
    // 前端 createTab 的 refresh_newtab 拿 GEN_LOCK 时防抖直接跳过，秒开启动页。
    // generate 关键路径已改为只读缓存不拉网络（newtab.rs），纯本地毫秒级。
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            let db_state = app_handle.state::<crate::db::Db>();
            if let Some(path) = crate::newtab::newtab_path(&app_handle) {
                match crate::newtab::generate(&db_state, &path).await {
                    Ok(p) => log::info!("[ShiftBrowser] 新标签页已预生成: {}", p),
                    Err(e) => log::error!("[ShiftBrowser] 新标签页预生成失败: {}", e),
                }
            }
            // 壁纸后台刷新（独立于 GEN_LOCK，拉取不阻塞任何 generate）
            crate::newtab::refresh_wallpapers_background(&app_handle).await;
        });
    }

    // 启动优化实施（锁屏让路）：以下初始化与锁屏 dialog 并发跑，不再排队等用户输密码。
    // dialog.run() 虽堵住 GTK 主线程，但 tokio 任务照跑——用户输完密码时后台多半已就绪。
    // 留守锁后的只有：TabManager（要 &mut app）、GTK 布局、定时器/接口/订阅/托盘
    // （spawn 本身不耗时，且锁屏期间不应对外服务/定时触发）、解锁通知、外部链接。
    // 注意：预热是后台跑的，极端情况（800ms 内解锁且首标签秒建）首个页面注入可能
    // 读到未预热的缓存——建标签的 re_inject 与延迟重试会补上，无感。
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            caches::warmup_caches(&app_handle).await;
        });
    }

    // P0-2 补完：注册 AI Provider 实现（纯内存注册，打破 engine→ai 反向依赖）
    crate::ai::register_provider_impls();

    // 引擎层：注册内容过滤器（网络层规则拦截）的缓存目录与应用句柄。
    // 必须在下面的注入流程之前注册：规则编译由注入流程触发（在主线程进行），
    // 编译完成后会自动重新注入，把过滤器挂到已打开标签上；
    // 加/删自定义关键词因此无需重启即可生效。
    {
        let cache_dir = app_data_dir.join("content-filters");
        crate::engine::content_filter::register_app(app.handle().clone(), cache_dir);
    }

    // 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
    // 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
    // 盖不住另一个独立的 GTK webview（标签页）。
    // R2-1b：实现已迁至 ui/lock_dialog.rs（含 LOCKED 状态与 GTK 对话框）
    let unlocked_after_lock =
        crate::lock_dialog::show_startup_lock(app, app.state::<crate::db::Db>().inner());

    // 初始化标签管理器
    app.manage(crate::tabs::TabManager::new());
    log::info!("[ShiftBrowser] 标签管理器初始化成功");

    // 定时任务调度器（阶段3 自动化：设置驱动的后台任务）
    crate::auto_tasks::spawn(app.handle().clone());

    // 本地开放 API（阶段4 开放生态：Unix 域套接字 + 行式 JSON）
    crate::api::spawn(app.handle().clone());

    // Linux：WebKit web extensions 目录 + 第三方 Cookie 策略
    // （P0-2 补完 · 2026-09-22：平台代码下沉到 engine::platform）
    #[cfg(target_os = "linux")]
    {
        let res_dir = app.path().resource_dir().ok();
        crate::engine::platform::setup_web_extensions_dir(res_dir);
        // 第三方 Cookie 阻断：仅"严格档"启用（绑定隐私防护档位，切换后需重启生效）
        let strict: bool = tauri::async_runtime::block_on(async {
            let db = app.state::<crate::db::Db>();
            let row: Option<(String,)> =
                sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield_level'")
                    .fetch_optional(&db.pool)
                    .await
                    .ok()
                    .flatten();
            row.map(|r| r.0 == "strict").unwrap_or(false)
        });
        crate::engine::platform::apply_cookie_policy(strict);
    }

    // Linux/GTK：初始化嵌入式 webview 的绝对定位布局（Fixed 容器 + 主 webview reparent）
    #[cfg(target_os = "linux")]
    {
        match crate::tabs::setup_gtk_layout(app) {
            Ok(()) => log::info!("[ShiftBrowser] GTK 布局初始化成功"),
            Err(e) => log::error!("[ShiftBrowser] GTK 布局初始化失败: {}", e),
        }
    }

    // 引擎层内容过滤器注册已在锁屏前完成（见上），此处不再重复——
    // register_app 重复调用会覆盖缓存目录与句柄，单次注册为准

    // 启动时注入所有启用扩展（UserContentManager 用户脚本/样式）
    // 启动优化实施：setup 时尚无标签页可注（首个标签由前端稍后创建，走建标签的
    // re_inject 覆盖），这里只后台预查一次（提前暴露坏扩展/坏规则日志），不堵主线程
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            let db_state = app_handle.state::<crate::db::Db>();
            match crate::db::extensions::list(&db_state).await {
                Ok(exts) => {
                    let enabled = exts.iter().filter(|e| e.enabled).count();
                    log::info!(
                        "[extensions] 启动预查：{} 个扩展（{} 个启用），首页面注入走建标签流程",
                        exts.len(),
                        enabled
                    );
                }
                Err(e) => log::warn!("[extensions] 启动预查失败（首页面注入不受影响）: {}", e),
            }
        });
    }

    // 启动时生成新标签页已在锁屏前 spawn 做过一次（见上），这里不再重复——
    // 两次全量 generate 只靠 1.5s 防抖去重，超时即双倍开销（启动优化实施）。
    // 前端 newtab_url 在文件不存在时会自行触发 generate（newtab.rs），有兜底

    // 防御：GNOME/Xwayland 偶发把新窗口以 Iconic（最小化）状态挂起，导致窗口不映射上屏。
    // 启动 1 秒后强制 unminimize + show + focus 一次
    {
        let app_win = app.handle().clone();
        // 窗口形态（2026-10-26 高哥拍板）：CEF 构建默认**只开 CEF 单栈窗口**，
        // 旧的 WebKit 主界面准备弃用，不再弹出来。需要主界面做对比/回退时设
        // SHIFT_WEBKIT_UI=1。主窗口在 tauri.conf.json 里默认 visible:false，
        // 非 CEF 构建在这里显示回来（延迟 1 秒是 XWayland 的既有兜底）。
        let webkit_ui = !cfg!(feature = "cef-engine") || std::env::var("SHIFT_WEBKIT_UI").is_ok();
        tauri::async_runtime::spawn(async move {
            // 用 tokio 异步 sleep，不占用阻塞线程（std::thread::sleep 会占一个 worker）
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            if let Some(w) = app_win.get_window("main") {
                if !webkit_ui {
                    crate::log_if_err!("window", w.hide());
                    // Tauri 的 hide() 在 GTK 下只改逻辑状态，X11 窗口仍 mapped
                    // （2026-10-27 实测：is_visible=false 但 xwininfo 仍 IsViewable）。
                    // 再走 GTK 层 hide 一次，确保真的不显示。
                    #[cfg(target_os = "linux")]
                    {
                        use gtk::prelude::*;
                        if let Ok(gtk_win) = w.gtk_window() {
                            gtk_win.hide();
                        }
                    }
                    log::info!(
                        "[ShiftBrowser] CEF 模式：主窗口 hide 后 visible={:?}",
                        w.is_visible()
                    );
                    let w2 = w.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                        log::info!(
                            "[ShiftBrowser] CEF 模式：800ms 后 visible={:?}",
                            w2.is_visible()
                        );
                        if w2.is_visible().unwrap_or(false) {
                            crate::log_if_err!("window", w2.hide());
                        }
                    });
                    log::info!("[ShiftBrowser] CEF 模式：主窗口不启动，只留 CEF 单栈窗口");
                    return;
                }
                crate::log_if_err!("window", w.unminimize());
                crate::log_if_err!("window", w.show());
                crate::log_if_err!("window", w.set_focus());
                log::info!("[ShiftBrowser] 主窗口已恢复/前置");
            }
        });
    }

    // 资讯订阅调度器（62 内置源，启动 6 秒后首轮，之后每 30 分钟）
    crate::feed::start_scheduler(app.handle());

    // 网络状态监测（状态栏：下行/上行/延迟，每 2 秒推送）
    crate::netmon::start_network_monitor(app.handle().clone());

    // 锁屏解锁过 → 通知前端重试初始化（锁屏期间 Db/TabManager 未就绪，
    // 主窗口 Vue 的 onMounted 初始化全部失败，解锁后必须重跑 loadTabs/createTab）
    if unlocked_after_lock {
        use tauri::Emitter;
        let _ = app.emit(crate::contracts::events::APP_UNLOCKED, ());
        log::info!("[ShiftBrowser] 已发出 app-unlocked，前端将重试初始化");
    }

    // ===== 系统托盘（关闭 = 缩托盘）=====
    // 见 tray.rs。ksni 菜单闭包跑在 D-Bus 线程，只往 mpsc 投动作，
    // 由 glib 定时器在 GTK 主线程排空执行（绝不跨线程碰 GTK）。
    // CEF 模式不建托盘：托盘菜单是旧主界面的入口，主窗口都停了，留着只会
    // 在系统托盘里挂一个点不开的图标（2026-10-27 高哥问「原来的图标还在」）。
    let cef_only = cfg!(feature = "cef-engine") && std::env::var("SHIFT_WEBKIT_UI").is_err();
    if !cef_only {
        tray_setup::setup_tray(app);
    }

    // 外部 URL 打开（默认浏览器入口）：命令行带 http/https 参数时（desktop 文件
    // Exec=shift-browser %U），等前端建好初始标签后再开新标签，避免与初始标签竞争。
    // 启动优化实施：旧逻辑死等 2000ms（慢机丢页、快机白等），现轮询 TabManager
    // 是否已有标签（200ms 一次），上限 EXTERNAL_URL_READY_MS，超时按旧逻辑直接开
    #[cfg(target_os = "linux")]
    {
        let urls: Vec<String> = std::env::args()
            .skip(1)
            .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
            .collect();
        if !urls.is_empty() {
            log::info!("[default-browser] 外部链接参数 {} 个", urls.len());
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let waited = {
                    let start = std::time::Instant::now();
                    let limit = std::time::Duration::from_millis(
                        crate::contracts::timeout::EXTERNAL_URL_READY_MS,
                    );
                    loop {
                        let n = app_handle
                            .state::<crate::tabs::TabManager>()
                            .all_tabs()
                            .len();
                        if n > 0 || start.elapsed() >= limit {
                            break start.elapsed();
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                    }
                };
                log::info!(
                    "[default-browser] 前端就绪等待 {}ms，开始打开外部链接",
                    waited.as_millis()
                );
                for url in urls {
                    let state = app_handle.state::<crate::tabs::TabManager>();
                    match crate::tabs::commands::create_tab(
                        app_handle.clone(),
                        state,
                        url.clone(),
                        None,
                        None,
                        None,
                        None,
                    )
                    .await
                    {
                        Ok(tab) => log::info!("[default-browser] 外部链接已打开: {}", tab.id),
                        Err(e) => log::warn!(
                            "[default-browser] 外部链接打开失败: {} ({})",
                            crate::redact_url(&url),
                            e
                        ),
                    }
                }
            });
        }
    }

    // 历史空标题回填（启动优化实施）：首绘后 20 秒空闲任务，每轮 500 条单事务，
    // 启动期不再跑，不跟首绘抢库
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            let db_state = app_handle.state::<crate::db::Db>();
            match crate::db::history::backfill_history_titles(db_state).await {
                Ok(0) => {}
                Ok(fixed) => log::info!("[ShiftBrowser] 历史空标题回填完成: {} 条", fixed),
                Err(e) => log::warn!("[ShiftBrowser] 历史空标题回填失败: {}", e),
            }
        });
    }

    Ok(())
}

/// 把 stdout/stderr 重定向到 `<app_data>/logs/stdio.log`（追加，超 5MB 轮转一份 .1）。
///
/// 必须在 CEF 初始化**之前**调用：CEF 子进程继承 stderr，Chromium 的崩溃线索
/// （CHECK failed / renderer crash / GPU fallback）都从这里出来。
fn redirect_stdio_to_file(app_data_dir: &std::path::Path) {
    use std::os::fd::IntoRawFd;

    let dir = app_data_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("stdio.log");
    // 简易轮转：只留一份上一版，避免无限增长
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 5 * 1024 * 1024 {
        let _ = std::fs::rename(&path, dir.join("stdio.log.1"));
    }
    let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let fd = f.into_raw_fd();
    // SAFETY: dup2 是异步信号安全的系统调用；成功后 fd 已复制，原 fd 可关。
    unsafe {
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
        libc::close(fd);
    }
    log::info!("[ShiftBrowser] stdout/stderr 已落盘: {path:?}");
}

// 应用启动装配（R2-1b 拆分：setup 主体从 lib.rs 抽出）
//
// 职责：Tauri `.setup()` 闭包的全部逻辑——敏感文件加固、内存压力治理、
// 数据库初始化、锁屏、TabManager/缓存/白名单载入、托盘、外部链接打开等。
// 失败路径（如数据库初始化失败）用本地 show_fatal_dialog 弹 GTK 原生窗后返回 Err。

use tauri::Manager;

// setup 的子步骤（按域拆出以控制本文件行数；调用顺序即原启动顺序，不可乱）
#[path = "bootstrap/guards.rs"]
mod guards;
#[path = "bootstrap/caches.rs"]
mod caches;
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
            // 历史空标题回填（2026-09-19）：页面加载时 document.title 未就绪导致
            // 历史里存了空标题，最近访问只剩网址——用 URL 域名兜底
            if let Ok(fixed) = crate::db::history::backfill_history_titles(db_state).await {
                if fixed > 0 {
                    log::info!("[ShiftBrowser] 历史空标题回填完成: {} 条", fixed);
                }
            }
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

    // 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
    // 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
    // 盖不住另一个独立的 GTK webview（标签页）。
    // R2-1b：实现已迁至 ui/lock_dialog.rs（含 LOCKED 状态与 GTK 对话框）
    let unlocked_after_lock = crate::lock_dialog::show_startup_lock(app, app.state::<crate::db::Db>().inner());

    // 初始化标签管理器
    app.manage(crate::tabs::TabManager::new());
    log::info!("[ShiftBrowser] 标签管理器初始化成功");

    // 用户脚本注入缓存 / settings 预热 / 权限白名单 / UA / devtools 开关载入
    caches::warmup_caches(app);

    // 定时任务调度器（阶段3 自动化：设置驱动的后台任务）
    crate::auto_tasks::spawn(app.handle().clone());

    // P0-2 补完：注册 AI Provider 实现（打破 engine→ai 反向依赖）
    crate::ai::register_provider_impls();

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
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT value FROM settings WHERE key = 'privacy_shield_level'",
            )
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

    // 引擎层：注册内容过滤器（网络层规则拦截）的缓存目录与应用句柄。
    // 必须在下面的注入流程之前注册：规则编译由注入流程触发（在主线程进行），
    // 编译完成后会自动重新注入，把过滤器挂到已打开标签上；
    // 加/删自定义关键词因此无需重启即可生效。
    {
        let cache_dir = app_data_dir.join("content-filters");
        crate::engine::content_filter::register_app(app.handle().clone(), cache_dir);
    }

    // 启动时注入所有启用扩展（UserContentManager 用户脚本/样式）
    {
        let app_handle = app.handle().clone();
        let db_state = app.state::<crate::db::Db>();
        crate::extensions::inject_on_startup(&app_handle, &db_state);
    }

    // 启动时生成新标签页（左侧栏面板生态）——异步生成，不阻塞主线程首绘
    // 前端 newtab_url 在文件不存在时会自行触发 generate（newtab.rs），有兜底
    {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            let db_state = app_handle.state::<crate::db::Db>();
            if let Some(path) = crate::newtab::newtab_path(&app_handle) {
                match crate::newtab::generate(&db_state, &path).await {
                    Ok(p) => log::info!("[ShiftBrowser] 新标签页已生成: {}", p),
                    Err(e) => log::error!("[ShiftBrowser] 新标签页生成失败: {}", e),
                }
            }
        });
    }

    // 防御：GNOME/Xwayland 偶发把新窗口以 Iconic（最小化）状态挂起，导致窗口不映射上屏。
    // 启动 1 秒后强制 unminimize + show + focus 一次
    {
        let app_win = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            // 用 tokio 异步 sleep，不占用阻塞线程（std::thread::sleep 会占一个 worker）
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            if let Some(w) = app_win.get_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
                log::info!("[ShiftBrowser] 窗口已强制恢复/前置");
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
    tray_setup::setup_tray(app);

    // 外部 URL 打开（默认浏览器入口）：命令行带 http/https 参数时（desktop 文件
    // Exec=shift-browser %U），启动后延迟打开新标签——等前端初始化完（onMounted
    // 会建初始标签），避免与初始标签竞争
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
                tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
                for url in urls {
                    let state = app_handle.state::<crate::tabs::TabManager>();
                    match crate::tabs::commands::create_tab(app_handle.clone(), state, url.clone(), None, None).await {
                        Ok(tab) => log::info!("[default-browser] 外部链接已打开: {}", tab.id),
                        Err(e) => log::warn!("[default-browser] 外部链接打开失败: {} ({})", crate::redact_url(&url), e),
                    }
                }
            });
        }
    }

    Ok(())
}
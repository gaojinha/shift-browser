// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

mod agent_cmd;
mod ai;
mod about;
mod audit;
mod capabilities;
mod constants;
mod content_cmd;
mod contracts;
mod db;
mod default_browser;
mod engine;
mod extensions;
mod feed;
mod gpc;
mod auto_tasks;
mod api;
// P2-5：仅 fuzz feature 下把手写解析器驱动入口暴露给 fuzz/ crate；生产构建不可见
#[cfg(feature = "fuzz")]
pub mod fuzz_bridge {
    pub use crate::api::fuzz_helpers;
}
mod mcp_cmd;
mod netmon;
mod newtab;
mod perms;
mod reader;
mod selfcheck;
mod tabs;
mod tls;
mod useragent;
mod translate;
mod tray;
// R2-1b 拆分（2026-09-24）：锁屏对话框独立成子模块
#[path = "ui/lock_dialog.rs"]
mod lock_dialog;
// R2-1b 拆分（2026-09-24）：shift:// 协议 + 翻译 IPC 独立成子模块
#[path = "proto/shift.rs"]
mod proto_shift;

use db::{
    bookmarks, downloads, history, passwords, sessions, settings, tab_groups, users, workspaces, Db,
};
use tabs::TabManager;
use tauri::Emitter;
use tauri::Manager;

/// 隐私脱敏（2026-09-22）：日志里的 URL 只保留 scheme://host，
/// 去掉路径/查询/片段——URL 路径常含搜索词、文档名、token 等敏感信息。
pub fn redact_url(u: &str) -> String {
    match url::Url::parse(u) {
        Ok(p) => match p.host_str() {
            Some(h) => format!("{}://{}", p.scheme(), h),
            None => format!("{}://[内部]", p.scheme()),
        },
        Err(_) => "<非URL>".to_string(),
    }
}

/// 敏感文件权限加固（安全 · 2026-09-22）：Cookie 库/侧车 WAL 按 umask 落盘时
/// 常见 664（组/其他用户可读），会话 Cookie 泄露风险。应用数据目录虽为 0700 兜底，
/// 但纵深防御要求文件自身也为 0600。运行时 WebKit 可能重建这些文件，故需周期重扫。
#[cfg(unix)]
pub fn harden_sensitive_files(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    const SENSITIVE: &[&str] = &[
        "cookies",
        "cookies.sqlite",
        "cookies.bak",
        "cookies-wal",
        "cookies-shm",
        "cookies.sqlite-wal",
        "cookies.sqlite-shm",
        "shift-browser.db-wal",
        "shift-browser.db-shm",
    ];
    for name in SENSITIVE {
        let p = dir.join(name);
        if p.exists() {
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
    // 日志目录同样收紧（隐私：日志虽已脱敏，仍不应组/其他可读）
    let logs = dir.join("logs");
    if logs.exists() {
        let _ = std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700));
        if let Ok(rd) = std::fs::read_dir(&logs) {
            for e in rd.flatten() {
                let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

/// 查询是否处于启动锁屏状态（前端初始化前调用，不依赖 Db）
#[tauri::command]
fn is_locked() -> bool {
    crate::lock_dialog::is_locked()
}

/// 前端日志桥：把前端关键节点的执行情况写进 Rust 日志，
/// 便于排查"前端白屏/事件收不到/初始化不执行"这类看不到 console 的问题。
#[tauri::command]
fn front_log(msg: String) {
    log::info!("[front] {}", msg);
}

/// 一键自检：汇总版本/存储/运行时/能力状态（只读，不修改任何状态）
#[tauri::command]
async fn selfcheck_cmd(app: tauri::AppHandle) -> Result<selfcheck::SelfCheckReport, String> {
    let report = selfcheck::run(&app).await;
    log::info!(
        "[selfcheck] 健康={} 版本项={} 存储项={} 运行时项={} 能力项={}",
        report.healthy,
        report.versions.len(),
        report.storage.len(),
        report.runtime.len(),
        report.capabilities.len()
    );
    // 关键环境事实写入日志（排查问题时第一手资料：版本/引擎/路径）
    let env_line: Vec<String> = report
        .versions
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 环境: {}", env_line.join(" | "));
    // 能力状态也记一行（过滤器是否就绪、拦截是否开启、隐私档位）
    let cap_line: Vec<String> = report
        .capabilities
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 能力: {}", cap_line.join(" | "));
    for item in report
        .versions
        .iter()
        .chain(report.storage.iter())
        .chain(report.runtime.iter())
        .chain(report.capabilities.iter())
    {
        if !item.ok {
            log::warn!("[selfcheck] 异常项: {} - {}", item.name, item.detail);
        }
    }
    Ok(report)
}

/// 「关于」页信息（只读，打开即显示，不依赖自检）
#[tauri::command]
fn app_about_cmd(app: tauri::AppHandle) -> about::AboutInfo {
    let info = about::collect(&app);
    log::info!(
        "[about] {} v{} · WebKitGTK {} · {} · {}",
        info.app_name,
        info.version,
        info.webkit_version,
        info.os_arch,
        info.profile
    );
    info
}

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

/// panic 日志目录（setup 中 app_data_dir 确定后注入）；未注入前 panic 至少落 stderr + 日志
static PANIC_LOG_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 安装全局 panic 钩子（P0-4，2026-09-24）。
///
/// 全应用无 `catch_unwind`：WebKit 网页进程崩溃有 UI 兜底，浏览器自身线程
/// panic 却只会让进程静默消失，用户无法提供任何证据。钩子统一记录
/// 线程/位置/载荷/backtrace 到 stderr、日志，并在数据目录追加 `panic.log`。
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread_name = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<非字符串 panic 载荷>".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[panic] unix_ts={ts} thread={thread_name} location={location}\n          payload: {payload}\n{backtrace}"
        );
        // 1. stderr：logger 尚未初始化的最早启动阶段也能保留
        eprintln!("{line}");
        // 2. tauri-plugin-log（初始化后进入正式日志文件）
        log::error!("{line}");
        // 3. 数据目录 panic.log 追加落盘（用户事后可提供）
        if let Some(dir) = PANIC_LOG_DIR.get() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                let _ = writeln!(f, "{line}\n");
            }
        }
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub fn run() {
    // 最早安装：任何后续初始化（含 GTK/WebKit）中 panic 都有证据
    install_panic_hook();

    // ===== WebKit DMA-BUF 线性格式兜底（P0-7，2026-09-24）=====
    // WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24:0 用于规避 WebKitGTK 2.42+
    // AcceleratedBackingStore 竞态空指针崩溃（见 HANDOFF-2026-09-23 全记录）。
    // 原先只写在 .desktop 模板的 Exec= env 前缀里——从终端/IDE/MCP 客户端等
    // 非 desktop 入口启动时不生效。前移到进程启动最早处（任何 GTK/WebKit
    // 初始化之前），覆盖全部启动入口；用户显式设置过同名变量则不覆盖。
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT").is_none() {
        std::env::set_var("WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT", "XR24:0");
        log::info!("[ShiftBrowser] 已注入 WEBKIT_DMABUF_RENDERER_BUFFER_FORMAT=XR24:0（启动入口兜底，可被环境变量覆盖）");
    }

    // ===== DMA-BUF 渲染器：不再默认禁用（2026-09-23 回退）=====
    // 【原方案】禁用它（WEBKIT_DISABLE_DMABUF_RENDERER=1）可修 Intel HD 630 上的
    // 网页/视频闪烁（WebKitGTK 2.42+ 默认走 DMA-BUF 路径，与 i915 配合不良）。
    //
    // 【为何回退】core dump 定位到崩溃在 WebKit 自身的
    //   AcceleratedBackingStore::update()（AcceleratedBackingStore.cpp:808）
    //   ← enterAcceleratedCompositingMode ← IPC EnterAcceleratedCompositingMode
    // 即「网页请求进入加速合成模式」时，m_legacyMainFrameProcess 已失效仍被解引用
    // （WebKit 生命周期竞态 bug，rdi=0）。而**禁用 DMA-BUF 会让加速合成走
    // SHM/EGLImage 兜底路径**，更容易踩中该竞态 → 网页删文件等操作触发 SIGSEGV。
    //
    // 结论：不禁用 = 走 WebKit 默认 DMA-BUF 路径，绕开该竞态。
    // 若视频闪烁复现，可显式设 `WEBKIT_DISABLE_DMABUF_RENDERER=1` 启动（二者取舍）。
    log::info!(
        "[ShiftBrowser] WEBKIT_DISABLE_DMABUF_RENDERER={:?}（未设置=走默认 DMA-BUF 渲染）",
        std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER")
    );

    // ===== 视频进程崩溃修复（2026-09-22）：降级硬件视频解码器 =====
    // 症状：播放某些 H.264 视频时 WebKitWebProcess 直接 abort：
    //   ** (WebKitWebProcess:PID): ERROR **: video meta uses 368x480 instead of 360x480
    // 成因：硬件解码器（V4L2 stateless / VA-API）输出帧宽按 16 字节对齐（360→368），
    // 而 WebKit 的 video meta 校验期望未对齐宽度，断言失败 → g_error() → 进程终止。
    // 规避：用 GStreamer 标准的 GST_PLUGIN_FEATURE_RANK 把硬件解码器 rank 压到 0，
    // 使其回退到软件解码（libav），彻底绕开对齐 padding 路径。
    // 用户可覆盖（机器上硬解正常、且想要 4K 硬解性能时，自行设置该变量）。
    if std::env::var_os("GST_PLUGIN_FEATURE_RANK").is_none() {
        std::env::set_var(
            "GST_PLUGIN_FEATURE_RANK",
            // rank 0 = 不参与自动选择；软件解码 avdec_* 保持默认 rank
            "v4l2slh264dec:0,v4l2slh265dec:0,v4l2slvp8dec:0,v4l2slvp9dec:0,v4l2slav1dec:0,\
             v4l2h264dec:0,v4l2h265dec:0,v4l2vp8dec:0,v4l2vp9dec:0,\
             vah264dec:0,vah265dec:0,vavp8dec:0,vavp9dec:0,vaav1dec:0,vajpegdec:0",
        );
    }
    log::info!(
        "[ShiftBrowser] GST_PLUGIN_FEATURE_RANK={:?}（视频进程崩溃修复）",
        std::env::var("GST_PLUGIN_FEATURE_RANK")
    );

    // ===== Cookie 持久化（修复"登录状态不保存"）=====
    // WebKitGTK 的 CookieManager 默认是内存模式，重启即丢——必须显式
    // set_persistent_storage 落盘。且该调用必须在任何 web process 创建之前
    // （tauri 的 setup 回调执行时主 webview 已创建，已太晚），故放在 run() 开头。
    // 存储路径与 app_data_dir 一致：$XDG_DATA_HOME/com.shiftbrowser.app/cookies.sqlite
    #[cfg(target_os = "linux")]
    {
        // P0-2：平台代码下沉到 engine::platform（GTK 初始化 + Cookie 持久化）
        let data_home = std::env::var("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::env::var("HOME")
                    .map(|h| std::path::PathBuf::from(h).join(".local/share"))
                    .unwrap_or_else(|_| std::path::PathBuf::from("."))
            });
        if let Some(cookie_path) =
            crate::engine::platform::init_cookie_persistence(&data_home)
        {
            // 安全：Cookie 库按 umask 落盘（常见 664），立即收紧到 600（另有周期重扫）
            if let Some(dir) = cookie_path.parent() {
                crate::harden_sensitive_files(dir);
            }
            // 此处 logger 插件尚未初始化，用 println! 保证启动日志可见
            println!(
                "[ShiftBrowser] Cookie 持久化已启用: {}",
                cookie_path.display()
            );
        }
    }

    // ===== 隐私防护 P3：TLS 指纹规范化（已回退，2026-09-18）=====
    // 回退原因：设置 G_TLS_GNUTLS_PRIORITY 后邮箱正文渲染为空 —— 该环境变量
    // 作用于进程内全部 TLS 连接，若站点部分资源依赖被裁掉的协议/套件组合，
    // 会表现为内容加载不全（空白）。网络底层改动风险远高于收益，故回退。
    // 保留结论：JA4 无法伪装成 Chrome（GnuTLS vs BoringSSL），
    // 与其在底层冒险，不如把预算放在页面内的加噪与归一化。
    // std::env::set_var(
    //     "G_TLS_GNUTLS_PRIORITY",
    //     "NORMAL:-VERS-ALL:+VERS-TLS1.3:+VERS-TLS1.2:-VERS-TLS1.0:-VERS-TLS1.1",
    // );
    tauri::Builder::default()
    // 单实例（阶段 4 D3）：已有实例运行时，新实例退出并把命令行参数转发给已有实例。
    // 必须注册为第一个插件（官方要求）。回调在已有实例进程里执行，
    // argv 是新实例的完整命令行参数——过滤 http/https 后让已有实例开新标签。
    .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
      let urls: Vec<String> = argv
        .iter()
        .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
        .cloned()
        .collect();
      log::info!("[single-instance] 已有实例收到新实例参数: {:?}", argv);
      if !urls.is_empty() {
        log::info!("[single-instance] 转发外部链接 {} 个", urls.len());
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
          // 稍等现有实例就绪（标签管理器已 manage）
          tokio::time::sleep(std::time::Duration::from_millis(300)).await;
          for url in urls {
            if let Some(state) = app_handle.try_state::<TabManager>() {
              match crate::tabs::commands::create_tab(app_handle.clone(), state, url.clone(), None, None).await {
                Ok(tab) => log::info!("[single-instance] 外部链接已打开: {}", tab.id),
                Err(e) => log::warn!("[single-instance] 外部链接打开失败: {} ({})", redact_url(&url), e),
              }
            }
          }
          // 窗口前置（可能最小化/缩托盘）
          if let Some(w) = app_handle.get_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
          }
        });
      }
    }))
    .plugin(tauri_plugin_opener::init())
    // 注意：不再注册 shell/dialog/fs 插件——前端只用到 opener，
    // 多余插件只增加攻击面与二进制体积（安全最小化原则）
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        // 并发写 SQLite 时 sqlx 慢语句 WARN 刷屏，降到 Error
        .level_for("sqlx::query", log::LevelFilter::Error)
        .build(),
    )
    .invoke_handler(tauri::generate_handler![
      // 启动锁屏状态（前端初始化前查询，锁屏期间静默等待解锁）
      is_locked,
      // 前端日志桥（排查前端执行链路）
      front_log,
      // 设置
      settings::get_setting,
      settings::get_all_settings,
      settings::set_setting,
      settings::delete_setting,
      settings::set_settings_bulk,
      // 网络层拦截规则落盘（阶段3.2：写 Web 进程扩展规则文件）
      settings::sync_ext_rules,
      // 默认浏览器（阶段 4：xdg-settings 集成）
      default_browser::default_browser_status,
      default_browser::set_default_browser,
      // 用户脚本（阶段3.2 平台化：Tampermonkey 风格）
      db::userscripts::list_userscripts,
      db::userscripts::refresh_userscripts_cache,
      db::userscripts::add_userscript,
      db::userscripts::update_userscript,
      db::userscripts::delete_userscript,
      // 证书管理 / 监测 / 验证（P1-P3 · 2026-09-22）
      tls::get_tls_info,
      tls::list_certificates,
      tls::get_certificate,
      tls::delete_certificate,
      tls::list_cert_exceptions,
      tls::delete_cert_exception,
      tls::list_cert_events,
      tls::clear_cert_events,
      tls::scan_cert_expiry,
      tls::accept_cert_exception,
      // User-Agent 管理（方案 B · 2026-09-22）
      useragent::get_user_agent_config,
      useragent::set_user_agent,
      useragent::set_user_agent_override,
      // 一键清除浏览数据（隐私 · 2026-09-22）
      tabs::commands::clear_browsing_data,
      // 权限请求（安全 #2 · 2026-09-22）
      perms::respond_permission,
      perms::list_permission_rules,
      perms::remove_permission_rule,
      // 书签
      bookmarks::get_bookmarks,
      bookmarks::get_bookmark_folders,
      bookmarks::create_bookmark,
      bookmarks::create_bookmark_folder,
      bookmarks::update_bookmark,
      bookmarks::delete_bookmark,
      bookmarks::delete_bookmark_folder,
      bookmarks::search_bookmarks,
      bookmarks::import_bookmarks_html,
      bookmarks::import_bookmarks_from_file,
      bookmarks::export_bookmarks_html,
      // 历史
      history::record_history,
      history::backfill_history_titles,
      history::get_recent_history,
      history::search_history,
      history::get_most_visited,
      history::clear_history,
      history::clear_history_before,
      history::delete_history_entry,
      // 会话
      sessions::save_session,
      sessions::load_last_session,
      sessions::list_sessions,
      sessions::delete_session,
      // 翻译
      translate::translate_text,
      // 下载记录
      downloads::create_download,
      downloads::update_download_progress,
      downloads::get_downloads,
      downloads::delete_download,
      // 标签分组
      tab_groups::create_tab_group,
      tab_groups::get_tab_groups,
      tab_groups::update_tab_group,
      tab_groups::delete_tab_group,
      // 工作区
      workspaces::create_workspace,
      workspaces::get_workspaces,
      workspaces::update_workspace,
      workspaces::delete_workspace,
      // 密码（AES-256-GCM 加密存储）
      passwords::save_password,
      passwords::get_password,
      passwords::get_password_by_id,
      passwords::get_passwords_for_domain,
      passwords::list_passwords,
      passwords::delete_password,
      // 密码库主密码（阶段 2：数据主权底线）
      db::vault::vault_status,
      db::vault::set_vault_password,
      db::vault::unlock_vault,
      db::vault::lock_vault,
      db::vault::remove_vault_password,
      db::vault::set_vault_idle_lock,
      // 本地开放 API：token 热轮换 / 状态面板一次性门票（HANDOFF P2-3）
      api::rotate_api_token,
      api::create_dashboard_ticket,
      // 上云 E2E 加密同步（第 45 章 · 服务器零知识）
      db::sync::sync_status,
      db::sync::sync_configure,
      db::sync::sync_push,
      db::sync::sync_pull,
      db::sync::sync_clear,
      // 标签页
      tabs::commands::create_tab,
      tabs::commands::close_tab,
      tabs::commands::close_all_tabs,
      tabs::commands::switch_tab,
      // 性能：空闲标签休眠（回收渲染进程内存）
      tabs::commands::hibernate_idle_tabs,
      // 翻译：对当前活动标签整页翻译（复用 whole_page_translate.js）
      tabs::commands::translate_current_page,
      tabs::commands::navigate_tab,
      tabs::commands::go_back,
      tabs::commands::go_forward,
      tabs::commands::get_tabs,
      tabs::commands::set_webview_visible,
      tabs::commands::update_tab,
      tabs::commands::reload_tab,
      tabs::commands::reorder_tabs,
      tabs::commands::toggle_pin,
      tabs::commands::toggle_mute,
      tabs::commands::toggle_maximize,
      tabs::commands::minimize_window,
      tabs::commands::close_window,
      tabs::commands::start_drag_window,
      tabs::commands::set_bookmarks_bar_visible,
      tabs::commands::set_vertical_tabs,
      tabs::commands::set_split_tab,
      tabs::commands::clear_split,
      tabs::commands::get_split_tab,
      tabs::commands::get_nav_state,
      tabs::commands::set_webview_ai_panel,
      tabs::commands::open_devtools,
      // 自检（可观测性）
      selfcheck_cmd,
      // 关于页信息（只读，打开即显示）
      app_about_cmd,
      // AI 侧边栏
      ai::ai_chat,
      ai::ai_chat_stream,
      ai::ai_cancel_stream,
      ai::ai_list_providers,
      // 页面理解层（阶段 1.1）
      content_cmd::content_snapshot,
      content_cmd::content_extract,
      content_cmd::ai_friendliness,
      // llms.txt 信号（阶段 1.5 标准跟踪 · Lighthouse Agentic 信号③/④落地；
      // 走 LlmsCache 会话缓存 + 每域限次 + 3s 超时 + 失败静默——Agent 导航用的"站点自述"）
      crate::engine::llmstxt::read_llmstxt,
      // MCP 工具层（阶段 1.3）
      mcp_cmd::mcp_list_servers,
      mcp_cmd::mcp_add_server,
      mcp_cmd::mcp_remove_server,
      mcp_cmd::mcp_set_server_enabled,
      mcp_cmd::mcp_list_tools,
      mcp_cmd::mcp_call_tool,
      mcp_cmd::mcp_get_audit_log,
      // Agent 执行层（阶段 1.4）
      agent_cmd::agent_confirm_action,
      agent_cmd::agent_confirm_multi_step,
      agent_cmd::agent_action_record,
      agent_cmd::agent_action_history,
      agent_cmd::agent_action_replay_or_retry,
      agent_cmd::agent_actions_batch_retry,
      agent_cmd::agent_debug_execute,
      // 扩展管理
      extensions::load_extension,
      extensions::list_extensions,
      extensions::set_extension_enabled,
      extensions::delete_extension,
      // 广告拦截
      extensions::privacy_cmd::set_adblock_enabled,
      extensions::privacy_cmd::get_adblock_enabled,
      // 隐私防护（GPC + 反指纹 + WebRTC 防泄露）
      extensions::privacy_cmd::set_privacy_shield_enabled,
      extensions::privacy_cmd::get_privacy_shield_enabled,
      extensions::privacy_cmd::set_privacy_shield_level,
      extensions::privacy_cmd::get_privacy_shield_level,
      extensions::privacy_cmd::set_privacy_whitelist,
      extensions::privacy_cmd::get_privacy_whitelist,
      // 反指纹 2.0 双模式（阶段 2：加噪 / 统一化）
      extensions::privacy_cmd::set_privacy_fp_mode,
      extensions::privacy_cmd::get_privacy_fp_mode,
      // WebMCP polyfill 试验（阶段 1.5）
      extensions::privacy_cmd::set_webmcp_polyfill,
      extensions::privacy_cmd::get_webmcp_polyfill,
      // 隐私防护统计（防护看得见）
      extensions::get_privacy_stats,
      extensions::reset_privacy_stats,
      extensions::privacy_selftest,
      // GPC 站点声明感知（阶段 2：GPC 完整化）
      gpc::gpc_site_check,
      // 零遥测审计（阶段 2：全量网络出口清单）
      audit::egress_audit,
      // 新标签页
      newtab::refresh_newtab,
      newtab::get_newtab_url,
      newtab::save_quick_sites,
      newtab::todo_list,
      newtab::todo_save,
      newtab::open_ai_from_newtab,
      newtab::open_settings_from_newtab,
      newtab::open_url_from_newtab,
      // 用户账号（个人中心）
      users::user_register,
      users::user_login,
      users::user_logout,
      users::user_get_current,
      users::user_update_profile,
      users::user_change_password,
      users::user_delete,
      users::user_update_prefs,
      users::user_export,
      users::user_import,
      // 阅读模式
      reader::enter_reader_mode,
      reader::fetch_article,
      extensions::privacy_cmd::get_adblock_keywords,
      extensions::privacy_cmd::set_adblock_keywords,
    ])
    .register_uri_scheme_protocol("shift", |ctx, request| {
      // R2-1b（2026-09-24）：shift:// 协议响应逻辑已抽到 proto/shift.rs
      crate::proto_shift::handle(&ctx, request)
    })
    .setup(|app| {
      // 安全（#1 · 2026-09-22）：敏感文件（cookie 库/WAL）启动即加固到 0600，
      // 并每 120s 重扫一次——WebKit 运行时会按 umask 重建这些文件。
      #[cfg(unix)]
      {
        use tauri::Manager;
        if let Ok(dir) = app.path().app_data_dir() {
          crate::harden_sensitive_files(&dir);
          let dir2 = dir.clone();
          tauri::async_runtime::spawn(async move {
            loop {
              tokio::time::sleep(std::time::Duration::from_secs(120)).await;
              crate::harden_sensitive_files(&dir2);
            }
          });
        }
      }
      // ===== 性能：WebKitGTK 内存压力治理 =====
      // 实测痛点：9 个标签时全部 WebKit 进程合计 3.75 GB（每标签一个渲染进程、
      // 单个 240-500MB，WebKitGTK 默认不主动回收）。
      // 方案：设置内存压力阈值，超过后 WebKit 分级回收（保守→严格→杀进程）。
      // 必须在使用任何 WebContext 之前设置（WebsiteDataManager 的静态全局设置）。
      #[cfg(target_os = "linux")]
      {
        // P0-2：平台代码下沉到 engine::platform::linux
        let total_mb: u32 = std::fs::read_to_string("/proc/meminfo")
          .ok()
          .and_then(|s| {
            s.lines()
              .find(|l| l.starts_with("MemTotal:"))
              .and_then(|l| l.split_whitespace().nth(1))
              .and_then(|v| v.parse::<u64>().ok())
          })
          .map(|kb| (kb / 1024) as u32)
          .unwrap_or(8192);
        crate::engine::platform::linux::setup_memory_pressure(total_mb);
      }

      // 锁屏是否启用过（解锁后通知前端重试初始化——锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 命令调用全部失败，必须等 setup 完成后重试）
      // R2-1b：由 lock_dialog::show_startup_lock 返回（锁屏逻辑已迁至 ui/lock_dialog.rs）

      // 2026-09-19：GNOME 49 对无装饰窗口强制四角圆角，但 Tauri/wry 只把 WebView
      // 背景设为透明，GTK 窗口本身没设透明——圆角外露出黑色窗口背景（四个黑角）。
      // 手动设置 RGBA visual + app_paintable，让窗口真正透明，圆角外透出桌面。
      #[cfg(target_os = "linux")]
      {
        use gtk::prelude::*;
        if let Some(win) = app.get_window("main") {
          if let Ok(gtk_win) = win.gtk_window() {
            if let Some(screen) = gtk::prelude::WidgetExt::screen(&gtk_win) {
              if let Some(visual) = screen.rgba_visual() {
                gtk_win.set_visual(Some(&visual));
              }
              // Wayland 下 set_visual 无效（X11 概念），GTK 默认还会画主题背景色——
              // 用 CSS 强制窗口背景透明，圆角外才能透出桌面
              let css = gtk::CssProvider::new();
              css.load_from_data(b"window { background-color: transparent; }")
                .map_err(|e| format!("CSS 加载失败: {}", e))?;
              gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
              );
              log::info!(
                "[ShiftBrowser] 窗口透明设置：rgba_visual={}",
                screen.rgba_visual().is_some()
              );
            }
            gtk_win.set_app_paintable(true);
            gtk_win.queue_draw();
            log::info!(
              "[ShiftBrowser] 窗口透明已启用（app_paintable={}）",
              gtk_win.is_app_paintable()
            );
          }
        }
      }

      // 初始化数据库（异步）
      let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

      log::info!("[ShiftBrowser] app_data_dir = {:?}", app_data_dir);
      log::info!("[ShiftBrowser] 目录存在: {}", app_data_dir.exists());

      // 注入 panic hook 的落盘目录（P0-4）：此后任何线程 panic 都会追加到 panic.log
      let _ = PANIC_LOG_DIR.set(app_data_dir.clone());

      // 数据库是全应用硬依赖（TabManager/API/AI/设置全部 state::<Db>()），
      // 初始化失败必须**立即终止启动**——旧实现只记日志后继续，
      // 后续任意 state::<Db>() 访问都会无提示 panic（P0-3，2026-09-24）。
      let db = match tauri::async_runtime::block_on(Db::init(app_data_dir.clone())) {
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
          let db_state = app_handle.state::<Db>();
          // 历史空标题回填（2026-09-19）：页面加载时 document.title 未就绪导致
          // 历史里存了空标题，最近访问只剩网址——用 URL 域名兜底
          if let Ok(fixed) = history::backfill_history_titles(db_state).await {
            if fixed > 0 {
              log::info!("[ShiftBrowser] 历史空标题回填完成: {} 条", fixed);
            }
          }
          let db_state = app_handle.state::<Db>();
          if let Some(path) = newtab::newtab_path(&app_handle) {
            match newtab::generate(&db_state, &path).await {
              Ok(p) => log::info!("[ShiftBrowser] 新标签页已预生成: {}", p),
              Err(e) => log::error!("[ShiftBrowser] 新标签页预生成失败: {}", e),
            }
          }
          // 壁纸后台刷新（独立于 GEN_LOCK，拉取不阻塞任何 generate）
          newtab::refresh_wallpapers_background(&app_handle).await;
        });
      }

      // 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
      // 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
      // 盖不住另一个独立的 GTK webview（标签页）。
      // R2-1b：实现已迁至 ui/lock_dialog.rs（含 LOCKED 状态与 GTK 对话框）
      let unlocked_after_lock =
        lock_dialog::show_startup_lock(app, app.state::<Db>().inner());

      // 初始化标签管理器
      app.manage(tabs::TabManager::new());
      log::info!("[ShiftBrowser] 标签管理器初始化成功");

      // 用户脚本注入缓存（启动时刷新，确保首个页面注入前就绪——inject 路径只读缓存，不能 await）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::db::userscripts::refresh_cache(&db.pool));
        log::info!("[ShiftBrowser] 用户脚本注入缓存已刷新");
      }

      // 性能 P0：settings 缓存预热（消除热路径 N+1 查库）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::db::settings_cache::warmup(&db.pool));
      }

      // 权限白名单载入内存（安全 #2 · 2026-09-22）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::perms::load_allowlist(&db.pool));
      }
      // User-Agent 配置载入（方案 B · 2026-09-22）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::useragent::load(&db.pool));
      }
      // 开发者工具开关载入（安全 #6 · 2026-09-22，默认关）
      {
        let db = app.state::<crate::db::Db>();
        if let Ok(Some((v,))) = tauri::async_runtime::block_on(async {
          sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'enable_devtools'")
            .fetch_optional(&db.pool)
            .await
        }) {
          crate::engine::webview::DEVTOOLS_ENABLED
            .store(v == "true", std::sync::atomic::Ordering::Relaxed);
        }
      }

      // 定时任务调度器（阶段3 自动化：设置驱动的后台任务）
      auto_tasks::spawn(app.handle().clone());

      // P0-2 补完：注册 AI Provider 实现（打破 engine→ai 反向依赖）
      ai::register_provider_impls();

      // 本地开放 API（阶段4 开放生态：Unix 域套接字 + 行式 JSON）
      api::spawn(app.handle().clone());

      // Linux：WebKit web extensions 目录 + 第三方 Cookie 策略
      // （P0-2 补完 · 2026-09-22：平台代码下沉到 engine::platform）
      #[cfg(target_os = "linux")]
      {
        let res_dir = app.path().resource_dir().ok();
        crate::engine::platform::setup_web_extensions_dir(res_dir);
        // 第三方 Cookie 阻断：仅"严格档"启用（绑定隐私防护档位，切换后需重启生效）
        let strict: bool = tauri::async_runtime::block_on(async {
          let db = app.state::<Db>();
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
        match tabs::setup_gtk_layout(app) {
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
        engine::content_filter::register_app(app.handle().clone(), cache_dir);
      }

      // 启动时注入所有启用扩展（UserContentManager 用户脚本/样式）
      {
        let app_handle = app.handle().clone();
        let db_state = app.state::<Db>();
        extensions::inject_on_startup(&app_handle, &db_state);
      }

      // 启动时生成新标签页（左侧栏面板生态）——异步生成，不阻塞主线程首绘
      // 前端 newtab_url 在文件不存在时会自行触发 generate（newtab.rs），有兜底
      {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
          let db_state = app_handle.state::<Db>();
          if let Some(path) = newtab::newtab_path(&app_handle) {
            match newtab::generate(&db_state, &path).await {
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
      feed::start_scheduler(app.handle());

      // 网络状态监测（状态栏：下行/上行/延迟，每 2 秒推送）
      netmon::start_network_monitor(app.handle().clone());

      // 锁屏解锁过 → 通知前端重试初始化（锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 初始化全部失败，解锁后必须重跑 loadTabs/createTab）
      if unlocked_after_lock {
        let _ = app.emit(crate::contracts::events::APP_UNLOCKED, ());
        log::info!("[ShiftBrowser] 已发出 app-unlocked，前端将重试初始化");
      }

      // ===== 系统托盘（关闭 = 缩托盘）=====
      // 见 tray.rs。ksni 菜单闭包跑在 D-Bus 线程，只往 mpsc 投动作，
      // 由下面的 glib 定时器在 GTK 主线程排空执行（绝不跨线程碰 GTK）。
      {
        let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayAction>();

        // 起托盘服务（ksni 的 spawn 是 async，交给 tauri runtime）
        tauri::async_runtime::spawn(async move {
          use ksni::TrayMethods;
          match tray::ShiftTray::new(tray_tx).spawn().await {
            Ok(handle) => {
              log::info!("[tray] 系统托盘已启动");
              // Handle 必须存活到进程结束，drop 掉托盘图标就没了
              std::mem::forget(handle);
            }
            Err(e) => log::warn!("[tray] 托盘启动失败（无宿主扩展？）: {}", e),
          }
        });

        // 关窗 = 缩托盘：拦截主窗口 delete-event
        if let Some(w) = app.get_window("main") {
          if let Ok(gtk_win) = w.gtk_window() {
            use gtk::prelude::*;
            let app_de = app.handle().clone();
            gtk_win.connect_delete_event(move |_, _| {
              use std::sync::atomic::Ordering;
              if tray::REAL_QUIT.load(Ordering::SeqCst) {
                // 真退出：放行，窗口关闭 → 应用退出
                gtk::glib::Propagation::Proceed
              } else {
                // 缩托盘：隐藏窗口，阻止关闭
                if let Some(win) = app_de.get_window("main") {
                  let _ = win.hide();
                }
                log::info!("[tray] 窗口已缩到托盘（未退出）");
                gtk::glib::Propagation::Stop
              }
            });
          }
        }

        // GTK 主线程排空托盘动作（120ms 一轮，开销可忽略）
        let app_tray = app.handle().clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
          use std::sync::atomic::Ordering;
          while let Ok(action) = tray_rx.try_recv() {
            match action {
              tray::TrayAction::ToggleWindow => {
                if let Some(w) = app_tray.get_window("main") {
                  if w.is_visible().unwrap_or(false) {
                    let _ = w.hide();
                  } else {
                    let _ = w.show();
                    let _ = w.set_focus();
                  }
                }
              }
              tray::TrayAction::OpenDownloads => {
                let _ = app_tray.emit(crate::contracts::events::TRAY_OPEN_DOWNLOADS, ());
              }
              tray::TrayAction::OpenPasswords => {
                let _ = app_tray.emit(crate::contracts::events::TRAY_OPEN_PASSWORDS, ());
              }
              tray::TrayAction::Quit => {
                tray::REAL_QUIT.store(true, Ordering::SeqCst);
                if let Some(w) = app_tray.get_window("main") {
                  let _ = w.close();
                }
              }
            }
          }
          gtk::glib::ControlFlow::Continue
        });
      }

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
              let state = app_handle.state::<TabManager>();
              match crate::tabs::commands::create_tab(app_handle.clone(), state, url.clone(), None, None).await {
                Ok(tab) => log::info!("[default-browser] 外部链接已打开: {}", tab.id),
                Err(e) => log::warn!("[default-browser] 外部链接打开失败: {} ({})", redact_url(&url), e),
              }
            }
          });
        }
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    /// P1-2 契约守卫（2026-09-22）：断言 generate_handler! 注册的每个命令
    /// 在源码中都有对应的 #[tauri::command] 定义（防"定义了命令但漏注册"）。
    #[test]
    fn all_registered_commands_are_defined() {
        // 本文件即 lib.rs，include_str! 取自源码
        let src = include_str!("lib.rs");
        // 提取 generate_handler! 块
        let start = src.find("generate_handler![").expect("未找到 generate_handler");
        let block_start = start + "generate_handler![".len();
        let rest = &src[block_start..];
        let end = rest.find("])").expect("generate_handler 未闭合");
        let block = &rest[..end];
        // 注册的命令名（形如 a::b,）
        let registered: Vec<String> = block
            .split(',')
            .filter_map(|seg| {
                let t = seg.trim();
                if t.is_empty() { return None; }
                // 取最后一段（模块::函数）
                let name = t.split("::").last()?.trim();
                if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.is_empty() {
                    Some(name.to_string())
                } else {
                    None
                }
            })
            .collect();
        assert!(registered.len() > 100, "注册命令数异常: {}", registered.len());

        // 收集全部源码里的 #[tauri::command] 定义（含子模块）
        let sources = [
            include_str!("lib.rs"),
            include_str!("tabs.rs"),
            include_str!("tabs/commands.rs"),
            include_str!("tls.rs"),
            include_str!("perms.rs"),
            include_str!("useragent.rs"),
            include_str!("extensions.rs"),
            include_str!("extensions/adblock.rs"),
            include_str!("extensions/privacy.rs"),
            include_str!("extensions/privacy_cmd.rs"),
            include_str!("extensions/manager.rs"),
            include_str!("newtab.rs"),
            include_str!("ai.rs"),
            include_str!("api.rs"),
            include_str!("reader.rs"),
            include_str!("translate.rs"),
            include_str!("gpc.rs"),
            include_str!("audit.rs"),
            include_str!("mcp_cmd.rs"),
            include_str!("agent_cmd.rs"),
            include_str!("content_cmd.rs"),
            include_str!("default_browser.rs"),
            include_str!("db/bookmarks.rs"),
            include_str!("db/history.rs"),
            include_str!("db/sessions.rs"),
            include_str!("db/settings.rs"),
            include_str!("db/passwords.rs"),
            include_str!("db/vault.rs"),
            include_str!("db/sync.rs"),
            include_str!("db/workspaces.rs"),
            include_str!("db/tab_groups.rs"),
            include_str!("db/downloads.rs"),
            include_str!("db/userscripts.rs"),
            include_str!("db/users.rs"),
            include_str!("engine/llmstxt.rs"),
        ];
        let mut defined: std::collections::HashSet<String> = std::collections::HashSet::new();
        for s in sources {
            let mut idx = 0;
            while let Some(pos) = s[idx..].find("#[tauri::command]") {
                let at = idx + pos + "#[tauri::command]".len();
                // 找后续的 fn 名
                if let Some(fpos) = s[at..].find("fn ") {
                    let after = &s[at + fpos + 3..];
                    let name: String = after
                        .chars()
                        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                        .collect();
                    if !name.is_empty() {
                        defined.insert(name);
                    }
                }
                idx = at;
            }
        }

        // 断言：注册的每个命令都有定义
        let mut missing: Vec<&String> = registered
            .iter()
            .filter(|c| !defined.contains(*c))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "以下命令已注册但未找到 #[tauri::command] 定义: {:?}",
            missing
        );
    }

    #[test]
    fn callback_whitelist_accepts_normal() {
        assert!(crate::proto_shift::valid_callback("cb_1"));
        assert!(crate::proto_shift::valid_callback("a"));
        assert!(crate::proto_shift::valid_callback("_"));
        assert!(crate::proto_shift::valid_callback("x".repeat(64).as_str()));
    }

    #[test]
    fn callback_whitelist_rejects_injection() {
        // 恶意 callback 注入 JS：拼进变量名 eval 到特权 webview
        assert!(!crate::proto_shift::valid_callback(""));
        assert!(!crate::proto_shift::valid_callback("x".repeat(65).as_str()));
        assert!(!crate::proto_shift::valid_callback("cb;alert(1)//"));
        assert!(!crate::proto_shift::valid_callback("cb'));alert(1);//"));
        assert!(!crate::proto_shift::valid_callback("cb\"));alert(1);//"));
        assert!(!crate::proto_shift::valid_callback("cb\nalert(1)"));
        assert!(!crate::proto_shift::valid_callback("cb-1"));
        assert!(!crate::proto_shift::valid_callback("cb.1"));
        assert!(!crate::proto_shift::valid_callback("中文"));
    }

    #[test]
    fn nonce_injected_only_into_plain_inline_script_tags() {
        // 精确匹配 <script>：内联脚本拿 nonce；外部 module 脚本由 'self' 授权不动；
        // 闭合标签不得被改
        let html = "<head><script>var a=1;</script>\
<script type=\"module\" crossorigin src=\"/assets/x.js\"></script></head>";
        let out = crate::proto_shift::inject_script_nonce(html, "abc123");
        assert!(out.contains("<script nonce=\"abc123\">var a=1;</script>"));
        assert!(out.contains(
            "<script type=\"module\" crossorigin src=\"/assets/x.js\"></script>"
        ));
        // 只注入一次（闭合标签未被误匹配）
        assert_eq!(out.matches("nonce=\"abc123\"").count(), 1);
    }

    #[test]
    fn newtab_style_two_inline_scripts_both_get_nonce() {
        // newtab 渲染后：IPC 桥 + 业务脚本两个无属性 <script>
        let html = "<script>(function(){})();</script><div>x</div><script>boot();</script>";
        let out = crate::proto_shift::inject_script_nonce(html, "n2");
        assert_eq!(out.matches("<script nonce=\"n2\">").count(), 2);
    }

    #[test]
    fn csp_nonce_replaces_only_script_src_unsafe_inline() {
        let csp = crate::proto_shift::csp_with_script_nonce("Zm9vYmFy");
        assert!(
            csp.contains("script-src 'self' 'nonce-Zm9vYmFy'"),
            "script-src 应带 nonce: {csp}"
        );
        assert!(
            !csp.contains("script-src 'self' 'unsafe-inline'"),
            "script-src 不得再留 unsafe-inline"
        );
        // style-src 维持内联（本次收面只针对脚本）
        assert!(csp.contains("style-src 'self' 'unsafe-inline'"));
        // 其余指令不丢
        for directive in [
            "object-src 'none'",
            "base-uri 'self'",
            "connect-src 'self'",
            "default-src 'self'",
        ] {
            assert!(csp.contains(directive), "CSP 丢指令 {directive}: {csp}");
        }
    }

    #[test]
    fn fresh_nonce_is_random_attribute_safe_base64() {
        let a = crate::proto_shift::fresh_script_nonce();
        let b = crate::proto_shift::fresh_script_nonce();
        // 16 字节 → 标准 base64（24 字符含填充），字符集不含引号/尖括号/空白，
        // 可安全放进 HTML 属性与 CSP 令牌
        assert_eq!(a.len(), 24);
        for s in [&a, &b] {
            assert!(s
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '='));
            assert!(!s.contains(['"', '\'', '<', '>', ' ']));
        }
        assert_ne!(a, b, "nonce 不可重复");
    }
}

/// TS 类型契约导出（P2）：把 Rust struct 导出为 TS interface，
/// 消除前端 `src/types/*.ts` 手写与本端 struct 的漂移。
/// 生成物落在 `src/types/generated/`（由 cargo test 触发）。
#[cfg(test)]
mod ts_export_tests {
    use ts_rs::TS;

    #[test]
    fn export_bindings() {
        crate::tabs::Tab::export().expect("导出 Tab 失败");
        crate::db::bookmarks::Bookmark::export().expect("导出 Bookmark 失败");
        crate::db::history::HistoryEntry::export().expect("导出 HistoryEntry 失败");
        crate::db::downloads::Download::export().expect("导出 Download 失败");
        crate::db::passwords::PasswordMeta::export().expect("导出 PasswordMeta 失败");
        crate::db::sessions::Session::export().expect("导出 Session 失败");
        crate::db::sessions::SessionTab::export().expect("导出 SessionTab 失败");
        crate::extensions::manager::ExtensionInfo::export().expect("导出 ExtensionInfo 失败");
        crate::engine::content::Snapshot::export().expect("导出 Snapshot 失败");
        crate::engine::content::AriaElement::export().expect("导出 AriaElement 失败");
        crate::engine::content::InteractiveStats::export().expect("导出 InteractiveStats 失败");
        crate::engine::content::FormField::export().expect("导出 FormField 失败");
        crate::engine::content::LinkInfo::export().expect("导出 LinkInfo 失败");
        crate::engine::webmcp::WebmcpToolDef::export().expect("导出 WebmcpToolDef 失败");
    }
}

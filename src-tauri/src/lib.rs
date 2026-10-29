// LINES-EXEMPT: crate 根（模块声明+全局 setup+命令注册），不宜拆（2026-10-14 红线卡标注）
// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

mod about;
mod agent_cmd;
mod ai;
mod audit;
mod capabilities;
// constants.rs/contracts.rs 同时被 build.rs include! 用于生成前端 constants.ts，
// 部分条目仅 codegen 使用，在本 crate 编译期看似无引用——模块级 allow(dead_code)
#[allow(dead_code)]
mod constants;
mod content_cmd;
#[allow(dead_code)]
mod content_prompts;
/// 同上：契约源与 build.rs 共享，部分常量/方法仅服务 TS 生成
#[allow(dead_code)]
mod contracts;
mod db;
#[allow(dead_code)]
mod system_prompts;
// R1 错误类型化：统一错误码（E_DB/E_PERM/E_TIMEOUT/E_AUTH/E_NET/E_VALIDATION/E_INTERNAL）
mod api;
mod auto_tasks;
mod default_browser;
mod engine;
// CEF 引擎接入（--features cef-engine 才编译）
#[cfg(feature = "cef-engine")]
mod cef_cmd;

/// CEF 子进程入口（供 main.rs 最早期调用）。
/// 返回 true 表示本进程是 CEF 子进程且已处理完，主流程应直接退出。
#[cfg(feature = "cef-engine")]
pub fn cef_try_run_subprocess() -> bool {
    cef_cmd::try_run_subprocess()
}
mod error;
mod extensions;
mod feed;
mod gpc;
mod import_browser;
// P2-5：仅 fuzz feature 下把手写解析器驱动入口暴露给 fuzz/ crate；生产构建不可见
#[cfg(feature = "fuzz")]
pub mod fuzz_bridge {
    pub use crate::api::fuzz_helpers;
}

// F-100（2026-10-19）：真机 live 验收（tests/import_browser_live.rs）的窄门面。
// 与 fuzz_bridge 同范式——只开测试需要的 `*_inner` 入口，不改变任何运行时行为；
// 集成测试是独立 crate，`#[cfg(test)]` 对它不可见，故此处不加 cfg。
pub mod import_live_bridge {
    pub use crate::db::Db;
    pub use crate::import_browser::bookmarks::{import_bookmarks_inner, ImportBookmarksResult};
    pub use crate::import_browser::passwords::{import_passwords_inner, ImportPasswordsResult};
    pub use crate::import_browser::{
        preview_browser_import_inner, scan_browser_import_inner, ImportPreview, SourceReport,
    };
}
mod mcp_cmd;
mod net_guard;
mod netmon;
mod newtab;
mod perms;
mod reader;
mod selfcheck;
mod tabs;
mod tls;
mod translate;
mod tray;
mod update_check;
mod useragent;
// P2（2026-09-26）：网页随手问 —— Rust 代理连本机 opencode，浏览器只做手/入口。
// pub：tests/webqa_live.rs 真机验收直接调 SseAccumulator/fold_event/build_prompt
// 验证真实 IO 路径，而不是用 Python 复刻逻辑（复刻验证不了 Rust 代码本身）。
pub mod webqa;
// 侧边栏聊天的本机 opencode 推理 Provider（provider id：opencode）
// pub：tests/opencode_chat_live.rs 真机验收直接跑 OpencodeProvider 全链路
pub mod opencode_chat;
pub mod webzh;
// R2-1b 拆分（2026-09-24）：锁屏对话框独立成子模块
#[path = "ui/lock_dialog.rs"]
mod lock_dialog;
// R2-1b 拆分（2026-09-24）：shift:// 协议 + 翻译 IPC 独立成子模块
#[path = "proto/shift.rs"]
mod proto_shift;
// R2-1b 拆分（2026-09-24）：setup 启动装配独立成子模块
#[path = "app/bootstrap.rs"]
mod bootstrap;
// R2-1c 拆分（2026-09-25）：panic 钩子 / 启动环境变量 / 敏感文件加固 / 杂项命令
#[path = "app/env_setup.rs"]
mod env_setup;
#[path = "app/harden.rs"]
mod harden;
#[path = "app/misc_cmds.rs"]
mod misc_cmds;
#[path = "app/panic_hook.rs"]
mod panic_hook;
// 原生崩溃信号兜底（2026-10-19）：panic hook 抓不到 GTK/WebKit 段错误
#[path = "app/crash_signal.rs"]
pub(crate) mod crash_signal;
/// 统一错误上报（F-ERR-01）：errors.log + 环形缓冲 + FFI 边界 panic 防护
#[path = "app/err_report.rs"]
pub(crate) mod err_report;
#[cfg(unix)]
pub use harden::harden_sensitive_files;
pub(crate) use panic_hook::PANIC_LOG_DIR;

/// `let _ = expr;` 的“失败留痕”版（F-79 D2）。
///
/// 用于 GTK/WebKit/文件系统等 best-effort 操作：失败不该中断主流程，但静默吞掉
/// 会掩盖 UI 状态不一致、下载进度卡死、权限收紧失败等问题——统一按 warn 留痕，
/// 调用点写作 `crate::log_if_err!("标签", expr);`。
#[macro_export]
macro_rules! log_if_err {
    ($label:literal, $e:expr $(,)?) => {
        if let Err(err) = $e {
            log::warn!("[{}] 操作失败: {}", $label, err);
        }
    };
}

use db::{
    bookmarks, downloads, history, passwords, sessions, settings, tab_groups, users, workspaces,
};
use tabs::TabManager;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub fn run() {
    // 最早安装：任何后续初始化（含 GTK/WebKit）中 panic 都有证据
    panic_hook::install();
    // 原生崩溃兜底：GTK/WebKit 段错误时 panic hook 轮不到，信号处理器留信号+调用栈
    crash_signal::install();

    // GTK 初始化兜底：某些启动路径（尤其强制 Wayland / 无 DISPLAY）下，
    // WebKitGTK 的 WebContext 可能在 setup 前就被触发；未 init 会直接 panic。
    #[cfg(target_os = "linux")]
    {
        match gtk::init() {
            Ok(()) => {
                // logger 此时尚未初始化，用 eprintln 保证可见
                eprintln!("[ShiftBrowser] gtk::init 已完成（run 开头）");
            }
            Err(e) => eprintln!("[ShiftBrowser] gtk::init 失败（run 开头）: {e}"),
        }
    }

    // 启动环境变量与平台初始化（DMA-BUF 兜底 / 视频解码降级 / Cookie 持久化），
    // 必须在任何 GTK/WebKit 初始化之前执行——实现见 app/env_setup.rs
    env_setup::apply();

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
            // argv 可能含带 token/query 的完整 URL：http/https 参数只记 scheme://host，
            // 非 URL 参数原样（redact_url 对非 URL 返回"<非URL>"会丢信息，故只脱敏 URL）
            let safe_argv: Vec<String> = argv
                .iter()
                .map(|a| {
                    if a.starts_with("http://") || a.starts_with("https://") {
                        crate::redact_url(a)
                    } else {
                        a.clone()
                    }
                })
                .collect();
            log::info!("[single-instance] 已有实例收到新实例参数: {:?}", safe_argv);
            if !urls.is_empty() {
                log::info!("[single-instance] 转发外部链接 {} 个", urls.len());
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    // 稍等现有实例就绪（标签管理器已 manage）
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    for url in urls {
                        if let Some(state) = app_handle.try_state::<TabManager>() {
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
                                Ok(tab) => {
                                    log::info!("[single-instance] 外部链接已打开: {}", tab.id)
                                }
                                Err(e) => log::warn!(
                                    "[single-instance] 外部链接打开失败: {} ({})",
                                    redact_url(&url),
                                    e
                                ),
                            }
                        }
                    }
                    // 窗口前置（可能最小化/缩托盘）
                    if let Some(w) = app_handle.get_window("main") {
                        crate::log_if_err!("window", w.show());
                        crate::log_if_err!("window", w.unminimize());
                        crate::log_if_err!("window", w.set_focus());
                    }
                });
            }
        }))
        .plugin(tauri_plugin_opener::init())
        // 注意：不再注册 shell/dialog/fs 插件——前端只用到 opener，
        // 多余插件只增加攻击面与二进制体积（安全最小化原则）
        .plugin(
            tauri_plugin_log::Builder::default()
                // 排查用：SHIFT_LOG_LEVEL=debug|trace 提级，默认 info
                .level(
                    std::env::var("SHIFT_LOG_LEVEL")
                        .ok()
                        .and_then(|v| match v.to_ascii_lowercase().as_str() {
                            "trace" => Some(log::LevelFilter::Trace),
                            "debug" => Some(log::LevelFilter::Debug),
                            "warn" => Some(log::LevelFilter::Warn),
                            "error" => Some(log::LevelFilter::Error),
                            "info" => Some(log::LevelFilter::Info),
                            _ => None,
                        })
                        .unwrap_or(log::LevelFilter::Info),
                )
                // 并发写 SQLite 时 sqlx 慢语句 WARN 刷屏，降到 Error
                .level_for("sqlx::query", log::LevelFilter::Error)
                // 单文件 10MB，留 3 份历史（原默认 KeepOne 只有一份，崩溃前后容易滚掉）
                .max_file_size(10 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            // CEF 引擎（换核实验，--features cef-engine）
            #[cfg(feature = "cef-engine")]
            cef_cmd::cef_open_window,
            #[cfg(feature = "cef-engine")]
            cef_cmd::cef_open_shell,
            #[cfg(feature = "cef-engine")]
            cef_cmd::cef_navigate,
            #[cfg(feature = "cef-engine")]
            cef_cmd::cef_close_window,
            #[cfg(feature = "cef-engine")]
            cef_cmd::cef_poll_events,
            // 启动锁屏状态（前端初始化前查询，锁屏期间静默等待解锁）
            misc_cmds::is_locked,
            // 前端日志桥（排查前端执行链路）
            misc_cmds::front_log,
            // 设置
            settings::get_setting,
            settings::get_all_settings,
            settings::set_setting,
            settings::delete_setting,
            settings::set_settings_bulk,
            settings::reset_settings,
            // 网络层拦截规则落盘（阶段3.2：写 Web 进程扩展规则文件）
            settings::sync_ext_rules,
            // 默认浏览器（阶段 4：xdg-settings 集成）
            default_browser::default_browser_status,
            default_browser::set_default_browser,
            // 用户脚本（阶段3.2 平台化：Tampermonkey 风格）
            db::userscripts::list_userscripts_cmd,
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
            // 断网四层诊断（网卡/网关/DNS/出网，只读）
            netmon::net_diagnose,
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
            bookmarks::bookmarks_html::import_bookmarks_html,
            bookmarks::bookmarks_html::import_bookmarks_from_file,
            bookmarks::bookmarks_html::export_bookmarks_html,
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
            api::dashboard::rotate_api_token,
            api::dashboard::create_dashboard_ticket,
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
            tabs::commands::get_tabs_version,
            tabs::commands::create_tabs_batch,
            tabs::commands::set_webview_visible,
            tabs::commands::page_backdrop_shot,
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
            misc_cmds::selfcheck_cmd,
            // 关于页信息（只读，打开即显示）
            misc_cmds::app_about_cmd,
            // 启动查版本提示（R4-4 先行版：只提示不安装，fail-silent）
            update_check::check_for_update,
            // AI 侧边栏
            ai::ai_chat,
            ai::ai_chat_stream,
            ai::ai_cancel_stream,
            ai::ai_list_providers,
            // P2/P3：网页随手问（本机 opencode 代理）
            webqa::read_page_selection,
            webqa::opencode_probe,
            webqa::opencode_ask,
            webqa::opencode_cancel,
            // 本机 opencode 作为侧边栏推理 Provider：设置页模型下拉
            opencode_chat::models::opencode_list_models,
            // 整页汉化（webzh）
            webzh::page_collect_translatable,
            webzh::page_apply_translation,
            webzh::opencode_translate_batch,
            webzh::opencode_translate_cleanup,
            webzh::plan_translate_batches,
            webzh::opencode_translate_model,
            webzh::opencode_translate_warmup,
            webzh::translate_memory_lookup,
            webzh::translate_memory_remember,
            // 页面理解层（阶段 1.1）
            content_cmd::content_snapshot,
            content_cmd::content_extract,
            content_cmd::content_summary_prompt,
            content_cmd::content_qa_prompt,
            content_cmd::feed_dedup_prompt,
            content_cmd::feed_recommend_prompt,
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
            extensions::privacy_cmd::get_content_filter_status,
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
            // Chrome 指纹补齐（2026-09-29 Turnstile 确诊缺口，默认开）
            extensions::privacy_cmd::set_compat_chrome_fp,
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
            // 启动页侧栏「导入」图标 → 主界面事件（F-93）
            newtab::open_import_from_newtab,
            // 从浏览器导入（F-89 起的模块链：扫描 → 解密 → 书签/密码落库）
            import_browser::scan_browser_import,
            // 导入前预览（F-99：站点/用户名 + 书签分组，无明文字段）
            import_browser::preview_browser_import,
            import_browser::bookmarks::import_bookmarks_from_browser,
            import_browser::passwords::import_passwords_from_browser,
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
            // 窗口形态：CEF 构建不创建主窗口（彻底脱钩）；WebKit 构建照旧。
            // SHIFT_WEBKIT_UI=1 可强制要回主界面（对比/回退）。
            let cef_only =
                cfg!(feature = "cef-engine") && std::env::var("SHIFT_WEBKIT_UI").is_err();
            if !cef_only {
                crate::bootstrap::create_main_window(app)?;
            }
            // R2-1b（2026-09-24）：setup 启动装配已抽到 app/bootstrap.rs
            crate::bootstrap::setup(app)?;
            // CEF 引擎（换核实验）：初始化失败不阻断主流程，只记录
            #[cfg(feature = "cef-engine")]
            {
                if let Err(e) = cef_cmd::init(app.handle()) {
                    log::error!("[cef] 初始化失败，回退 WebKitGTK: {e}");
                }
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// R2-1 拆分：契约守卫 + TS 类型导出测试移至 lib_tests.rs
#[cfg(test)]
#[path = "lib_tests.rs"]
mod lib_tests;

mod db;
mod tabs;
mod ai;
mod extensions;
mod newtab;
mod feed;
mod reader;

use db::{bookmarks, downloads, history, passwords, sessions, settings, tab_groups, workspaces, Db};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
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
      // 设置
      settings::get_setting,
      settings::get_all_settings,
      settings::set_setting,
      settings::delete_setting,
      settings::set_settings_bulk,
      // 书签
      bookmarks::get_bookmarks,
      bookmarks::get_bookmark_folders,
      bookmarks::create_bookmark,
      bookmarks::create_bookmark_folder,
      bookmarks::update_bookmark,
      bookmarks::delete_bookmark,
      bookmarks::delete_bookmark_folder,
      bookmarks::search_bookmarks,
      // 历史
      history::record_history,
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
      // 标签页
      tabs::create_tab,
      tabs::close_tab,
      tabs::close_all_tabs,
      tabs::switch_tab,
      tabs::navigate_tab,
      tabs::go_back,
      tabs::go_forward,
      tabs::get_tabs,
      tabs::set_webview_visible,
      tabs::update_tab,
      tabs::set_bookmarks_bar_visible,
      tabs::get_nav_state,
      // AI 侧边栏
      ai::ai_chat,
      // 扩展管理
      extensions::load_extension,
      extensions::list_extensions,
      extensions::set_extension_enabled,
      extensions::delete_extension,
      // 广告拦截
      extensions::set_adblock_enabled,
      extensions::get_adblock_enabled,
      // 新标签页
      newtab::refresh_newtab,
      newtab::get_newtab_url,
      // 阅读模式
      reader::enter_reader_mode,
      reader::fetch_article,
      extensions::get_adblock_keywords,
      extensions::set_adblock_keywords,
    ])
    .setup(|app| {
      // 初始化数据库（异步）
      let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

      log::info!("[ShiftBrowser] app_data_dir = {:?}", app_data_dir);
      log::info!("[ShiftBrowser] 目录存在: {}", app_data_dir.exists());

      let db_handle = tauri::async_runtime::block_on(Db::init(app_data_dir));
      match db_handle {
        Ok(db) => {
          app.manage(db);
          log::info!("[ShiftBrowser] 数据库初始化成功");
        }
        Err(e) => {
          log::error!("[ShiftBrowser] 数据库初始化失败: {}", e);
        }
      }

      // 初始化标签管理器
      app.manage(tabs::TabManager::new());
      log::info!("[ShiftBrowser] 标签管理器初始化成功");

      // Linux：手动设置 WebKit web extensions 目录（tauri://localhost 协议依赖 web extension）
      // Tauri 2 不会自动设置 extensions_path，必须手动指定，否则前端资源加载失败
      #[cfg(target_os = "linux")]
      {
        use webkit2gtk::WebContextExt;
        use webkit2gtk::WebContext;
        if let Some(context) = WebContext::default() {
          if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
              context.set_web_extensions_directory(&dir.display().to_string());
              log::info!("[ShiftBrowser] web extensions 目录: {}", dir.display());
            }
          }
          // 注意：不要尝试 set_process_model(SharedSecondaryProcess)——
          // 1) 主 webview 在 setup 前已创建，进程池已固化，设置被 g_return_if_fail 静默忽略；
          // 2) 实测强行切换会触发 WebKit bug（WebLoaderStrategy internallyFailedLoad），整个应用崩。
          // 多标签开销靠"切标签前 hide 旧 + 前端连点合并"缓解，进程模型保持默认。
        }
      }

      // Linux/GTK：初始化嵌入式 webview 的绝对定位布局（Fixed 容器 + 主 webview reparent）
      #[cfg(target_os = "linux")]
      {
        match tabs::setup_gtk_layout(app) {
          Ok(()) => log::info!("[ShiftBrowser] GTK 布局初始化成功"),
          Err(e) => log::error!("[ShiftBrowser] GTK 布局初始化失败: {}", e),
        }
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

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
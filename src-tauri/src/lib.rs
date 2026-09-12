mod db;
mod tabs;
mod ai;
mod extensions;

use db::{bookmarks, downloads, history, passwords, sessions, settings, tab_groups, workspaces, Db};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_fs::init())
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
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
      // AI 侧边栏
      ai::ai_chat,
      // 扩展管理
      extensions::load_extension,
      extensions::list_extensions,
      extensions::set_extension_enabled,
      extensions::delete_extension,
    ])
    .setup(|app| {
      // 初始化数据库（异步）
      let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

      println!("[ShiftBrowser] app_data_dir = {:?}", app_data_dir);
      println!("[ShiftBrowser] 目录存在: {}", app_data_dir.exists());

      let db_handle = tauri::async_runtime::block_on(Db::init(app_data_dir));
      match db_handle {
        Ok(db) => {
          app.manage(db);
          println!("[ShiftBrowser] 数据库初始化成功");
        }
        Err(e) => {
          eprintln!("[ShiftBrowser] 数据库初始化失败: {}", e);
        }
      }

      // 初始化标签管理器
      app.manage(tabs::TabManager::new());
      println!("[ShiftBrowser] 标签管理器初始化成功");

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
              println!("[ShiftBrowser] web extensions 目录: {}", dir.display());
            }
          }
        }
      }

      // Linux/GTK：初始化嵌入式 webview 的绝对定位布局（Fixed 容器 + 主 webview reparent）
      #[cfg(target_os = "linux")]
      {
        match tabs::setup_gtk_layout(app) {
          Ok(()) => println!("[ShiftBrowser] GTK 布局初始化成功"),
          Err(e) => eprintln!("[ShiftBrowser] GTK 布局初始化失败: {}", e),
        }
      }

      // 启动时注入所有启用扩展（UserContentManager 用户脚本/样式）
      {
        let app_handle = app.handle().clone();
        let db_state = app.state::<Db>();
        extensions::inject_on_startup(&app_handle, &db_state);
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
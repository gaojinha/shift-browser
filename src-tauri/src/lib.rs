mod db;
mod tabs;
mod ai;
mod extensions;
mod newtab;
mod feed;
mod reader;
mod translate;

use db::{bookmarks, downloads, history, passwords, sessions, settings, tab_groups, users, workspaces, Db};
use tauri::Manager;

/// 简易 percent-decode（处理 shift:// query 参数，避免引入 url crate）
/// 注意：只处理 %XX 和 + → 空格，满足 shift:// 内部用
fn percent_decode(s: &str) -> String {
  let bytes = s.as_bytes();
  let mut out = Vec::with_capacity(bytes.len());
  let mut i = 0;
  while i < bytes.len() {
    match bytes[i] {
      b'+' => { out.push(b' '); i += 1; }
      b'%' if i + 2 < bytes.len() => {
        let h1 = (bytes[i + 1] as char).to_digit(16);
        let h2 = (bytes[i + 2] as char).to_digit(16);
        if let (Some(a), Some(b)) = (h1, h2) {
          out.push((a * 16 + b) as u8);
          i += 3;
        } else {
          out.push(bytes[i]); i += 1;
        }
      }
      c => { out.push(c); i += 1; }
    }
  }
  String::from_utf8_lossy(&out).into_owned()
}

/// 把翻译结果写到对应 webview 的 window.__shift_translate_result_<callback>
/// 菜单 JS 轮询这个变量拿到结果（shift:// Image 请求不读响应体，必须走 window 变量）
fn write_translate_result(
  app: &tauri::AppHandle,
  webview_label: &str,
  callback: &str,
  payload: &serde_json::Value,
) {
  let Some(wv) = app.get_webview(webview_label) else {
    log::warn!("[translate] 找不到 webview: {}", webview_label);
    return;
  };
  // 序列化为 JS 表达式：window.__shift_translate_result_<cb> = <json>;
  let json_str = payload.to_string();
  let escaped_cbk = callback.replace('\'', "\\'");
  let js = format!(
    "window.__shift_translate_result_{} = {};",
    escaped_cbk, json_str
  );
  if let Err(e) = wv.eval(&js) {
    log::warn!("[translate] eval 结果失败: {}", e);
  } else {
    log::debug!("[translate] 已写入结果 callback={}", callback);
  }
}

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
      bookmarks::import_bookmarks_html,
      bookmarks::import_bookmarks_from_file,
      bookmarks::export_bookmarks_html,
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
      tabs::reload_tab,
      tabs::reorder_tabs,
      tabs::toggle_pin,
      tabs::toggle_mute,
      tabs::toggle_maximize,
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
      newtab::save_quick_sites,
      newtab::open_ai_from_newtab,
      newtab::open_settings_from_newtab,
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
      extensions::get_adblock_keywords,
      extensions::set_adblock_keywords,
    ])
    .register_uri_scheme_protocol("shift", |ctx, request| {
      // shift://newtab → newtab/index.html; shift://worldnav → world-nav.html
      // shift://main → dist 前端资源（主窗口，从嵌入资源读取并替换 invoke_key）
      // 注意：shift://newtab 的 host 是 newtab，path 是 /
      let host = request.uri().host().unwrap_or("").to_string();
      let path = request.uri().path().to_string();
      let app = ctx.app_handle();

      // 主窗口：shift://main → dist（asset_resolver 读嵌入资源）
      if host == "main" {
        let rel = if path == "/" || path == "/index.html" {
          "index.html".to_string()
        } else {
          path.trim_start_matches('/').to_string()
        };
        if let Some(asset) = app.asset_resolver().get(rel.clone()) {
          let bytes = asset.bytes().to_vec();
          let mime = asset.mime_type();
          if rel == "index.html" {
            // 替换 IPC 桥里的 invoke_key 占位符（Tauri 校验 invoke_key）
            let html = String::from_utf8_lossy(&bytes);
            let html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
            return tauri::http::Response::builder()
              .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
              .body(html.into_bytes())
              .unwrap();
          }
          return tauri::http::Response::builder()
            .header(tauri::http::header::CONTENT_TYPE, mime)
            .body(bytes)
            .unwrap();
        }
        log::warn!("[shift://] main 资源未找到: {}", rel);
        return tauri::http::Response::builder()
          .status(404)
          .header(tauri::http::header::CONTENT_TYPE, "text/plain")
          .body("Not Found".as_bytes().to_vec())
          .unwrap();
      }

      // 翻译 IPC：child webview 的右键菜单 JS 通过 shift://__translate__/* 调翻译
      // （shift handler 同步返回，翻译异步 spawn，结果通过 webview.eval 写回 window 全局变量，
      //  菜单 JS 轮询拿到结果）
      if host == "__translate__" {
        let webview_label = ctx.webview_label().to_string();
        let query = request.uri().query().unwrap_or("");
        let params: std::collections::HashMap<String, String> = query
          .split('&')
          .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?;
            let v = it.next().unwrap_or("");
            Some((percent_decode(k), percent_decode(v)))
          })
          .collect();
        let callback = params.get("callback").cloned().unwrap_or_default();
        let app_clone = app.clone();

        match path.as_str() {
          "/text" => {
            let text = params.get("text").cloned().unwrap_or_default();
            log::info!(
              "[translate] 选中文本翻译 request: label={} callback={} len={}",
              webview_label, callback, text.len()
            );
            tauri::async_runtime::spawn(async move {
              let payload = match translate::translate_google(&text, "zh-CN").await {
                Ok(t) => serde_json::json!({ "text": t }),
                Err(e) => serde_json::json!({ "error": e }),
              };
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
            });
          }
          "/whole-page" => {
            log::info!(
              "[translate] 整页翻译 request: label={} callback={}",
              webview_label, callback
            );
            let url = params.get("url").cloned().unwrap_or_default();
            tauri::async_runtime::spawn(async move {
              // 整页翻译简化版：先 eval 收集文本节点，再分块翻译，再 eval 回填
              // 这里先返回"开始翻译"状态——具体逻辑后续扩展
              let payload = serde_json::json!({
                "translated": 0u32,
                "total": 0u32,
                "note": "整页翻译链路已接通，待实现 DOM 收集/回填"
              });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
              let _ = url; // 暂未用
            });
          }
          "/remove-auto-site" => {
            log::info!("[translate] 取消本站自动汉化: {}", webview_label);
            // 暂简化：不实际清理列表
            tauri::async_runtime::spawn(async move {
              let payload = serde_json::json!({ "ok": true });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
            });
          }
          _ => {
            log::warn!("[translate] 未知子路径: {}", path);
          }
        }

        // shift handler 同步返回（菜单 JS 用 Image().src 触发，不读响应体）
        return tauri::http::Response::builder()
          .status(200)
          .header(tauri::http::header::CONTENT_TYPE, "image/gif")
          .body(Vec::<u8>::new())
          .unwrap();
      }

      let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      let newtab_dir = app_data.join("newtab");
      let file_path = match (host.as_str(), path.as_str()) {
        ("newtab", "/") | ("newtab", "/index.html") => newtab_dir.join("index.html"),
        // newtab 页内"世界导航"按钮用相对路径 world-nav.html 跳转，
        // 落到 shift://newtab/world-nav.html，必须匹配到才能加载世界导航页
        ("newtab", "/world-nav.html") => newtab_dir.join("world-nav.html"),
        ("worldnav", "/") | ("worldnav", "/index.html") => newtab_dir.join("world-nav.html"),
        ("worldnav", "/world-nav.html") => newtab_dir.join("world-nav.html"),
        _ => {
          log::warn!("[shift://] 未知路径: {} {}", host, path);
          return tauri::http::Response::builder()
            .status(404)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain")
            .body("Not Found".as_bytes().to_vec())
            .unwrap();
        }
      };
      log::info!("[shift://] 加载: {:?}", file_path);
      if let Ok(data) = std::fs::read(&file_path) {
        // 把 IPC 桥里的 invoke_key 占位符替换成运行时真实 key（Tauri 校验 invoke_key）
        let html = String::from_utf8_lossy(&data);
        let html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
        tauri::http::Response::builder()
          .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
          .body(html.into_bytes())
          .unwrap()
      } else {
        log::error!("[shift://] 文件读取失败: {:?}", file_path);
        tauri::http::Response::builder()
          .status(404)
          .header(tauri::http::header::CONTENT_TYPE, "text/plain")
          .body("File not found".as_bytes().to_vec())
          .unwrap()
      }
    })
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

      // 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
      // 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
      // 盖不住另一个独立的 GTK webview（标签页）。
      #[cfg(target_os = "linux")]
      {
        use gtk::prelude::*;
        let db_state = app.state::<Db>();
        let lock_enabled: bool = {
          let row = tauri::async_runtime::block_on(
            sqlx::query_as::<_, (String,)>(
              "SELECT value FROM settings WHERE key = 'lock_enabled'",
            )
            .fetch_optional(&db_state.pool),
          )
          .ok()
          .flatten();
          row.map(|r| r.0 == "true").unwrap_or(false)
        };

        if lock_enabled {
          log::info!("[ShiftBrowser] 启动锁屏已启用，弹出 GTK 密码对话框");
          let parent_win = app
            .get_window("main")
            .and_then(|w| w.gtk_window().ok());

          loop {
            let dialog = gtk::Dialog::builder()
              .title("Shift Browser 已锁定")
              .modal(true)
              .resizable(false)
              .default_width(340)
              .default_height(160)
              .build();

            if let Some(ref pw) = parent_win {
              dialog.set_transient_for(Some(pw));
            }

            let content = dialog.content_area();
            content.set_spacing(8);
            content.set_margin(12);

            let label = gtk::Label::new(Some("输入账号密码解锁浏览器"));
            content.add(&label);

            let user_entry = gtk::Entry::new();
            user_entry.set_placeholder_text(Some("用户名"));
            content.add(&user_entry);

            let pass_entry = gtk::Entry::new();
            pass_entry.set_visibility(false);
            pass_entry.set_placeholder_text(Some("密码"));
            pass_entry.set_activates_default(true);
            content.add(&pass_entry);

            dialog.add_button("退出", gtk::ResponseType::Cancel);
            dialog.add_button("解锁", gtk::ResponseType::Ok);

            dialog.show_all();
            pass_entry.grab_focus();

            let response = dialog.run();

            let username = user_entry.text().to_string();
            let password = pass_entry.text().to_string();

            dialog.close();

            if response != gtk::ResponseType::Ok {
              log::info!("[ShiftBrowser] 启动锁屏取消，退出应用");
              std::process::exit(0);
            }

            match tauri::async_runtime::block_on(users::verify_password(
              &db_state, &username, &password,
            )) {
              Ok(true) => {
                log::info!("[ShiftBrowser] 启动锁屏验证通过: {}", username);
                // 验证通过后清除会话：启动锁屏只验证身份，不自动登录个人中心
                tauri::async_runtime::block_on(users::clear_session(&db_state));
                // 显式 destroy 锁屏 dialog，避免 close 后残留；forget 阻止 drop 再次释放
                unsafe {
                  dialog.destroy();
                }
                std::mem::forget(dialog);
                break;
              }
              Ok(false) => {
                log::warn!("[ShiftBrowser] 启动锁屏密码错误，重新输入");
              }
              Err(e) => {
                log::error!("[ShiftBrowser] 启动锁屏验证出错: {}", e);
              }
            }
          }
        }
      }

      // 初始化标签管理器
      app.manage(tabs::TabManager::new());
      log::info!("[ShiftBrowser] 标签管理器初始化成功");

      // 预生成新标签页 HTML（确保 shift:// 加载时文件就绪，避免时序竞态）
      {
        let app_handle = app.handle();
        if let Some(path) = newtab::newtab_path(app_handle) {
          let db = app_handle.state::<Db>();
          match tauri::async_runtime::block_on(newtab::generate(&db, &path)) {
            Ok(_) => log::info!("[ShiftBrowser] 新标签页预生成: {}", path.display()),
            Err(e) => log::error!("[ShiftBrowser] 新标签页预生成失败: {}", e),
          }
        }
      }

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
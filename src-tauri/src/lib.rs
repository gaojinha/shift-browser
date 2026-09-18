mod ai;
mod db;
mod extensions;
mod feed;
mod netmon;
mod newtab;
mod reader;
mod tabs;
mod translate;

use db::{
    bookmarks, downloads, history, passwords, sessions, settings, tab_groups, users, workspaces, Db,
};
use tauri::Emitter;
use tauri::Manager;

/// shift:// 自定义协议页面的 CSP（主窗口/新标签页/世界导航页统一）
/// 注意：Tauri 的 security.csp 只注入 tauri:// 与 http://asset.localhost，
/// 自定义协议（shift://）不走注入，必须在这里手动加响应头。
/// script-src 允许 'unsafe-inline'：newtab/world-nav 是本地生成的可信内联脚本；
/// object-src 'none' + base-uri 'self' + connect-src 收紧 防注入面。
const SHIFT_CSP: &str = "default-src 'self' shift: data:; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https: http:; connect-src 'self' ipc: http://ipc.localhost https://wttr.in https://ipinfo.io; font-src 'self' data:; media-src 'self' https: http:; frame-src 'self' https: http:; object-src 'none'; base-uri 'self'";

/// 整页翻译注入脚本（编译期嵌入，运行时 eval 到 child webview）
const WHOLE_PAGE_TRANSLATE_JS: &str = include_str!("whole_page_translate.js");

/// 启动锁屏中标志：setup 弹出 GTK 密码对话框期间为 true。
/// 前端 onMounted 在锁屏期间调用 Db/TabManager 命令会全部失败并弹"创建/加载标签页失败"
/// toast（反复重试），必须能查询到锁屏状态并静默等待 app-unlocked。
/// 用原子变量而非 Db/State：锁屏期间 Db 尚未 manage，命令参数不能依赖 State。
static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 查询是否处于启动锁屏状态（前端初始化前调用，不依赖 Db）
#[tauri::command]
fn is_locked() -> bool {
    LOCKED.load(std::sync::atomic::Ordering::SeqCst)
}

/// 前端日志桥：把前端关键节点的执行情况写进 Rust 日志，
/// 便于排查"前端白屏/事件收不到/初始化不执行"这类看不到 console 的问题。
#[tauri::command]
fn front_log(msg: String) {
    log::info!("[front] {}", msg);
}

/// 简易 percent-decode（处理 shift:// query 参数，避免引入 url crate）
/// 注意：只处理 %XX 和 + → 空格，满足 shift:// 内部用
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h1 = (bytes[i + 1] as char).to_digit(16);
                let h2 = (bytes[i + 2] as char).to_digit(16);
                if let (Some(a), Some(b)) = (h1, h2) {
                    out.push((a * 16 + b) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
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
    // P2：callback 白名单双保险（入口已校验，这里再挡一道）——
    // callback 被拼进 JS 变量名 eval 到特权 webview，恶意 callback 可注入任意 JS
    if !valid_callback(callback) {
        log::warn!("[translate] 非法 callback，拒绝写入: {:?}", callback);
        return;
    }
    let Some(wv) = app.get_webview(webview_label) else {
        log::warn!("[translate] 找不到 webview: {}", webview_label);
        return;
    };
    // 序列化为 JS 表达式：window.__shift_translate_result_<cb> = <json>;
    let json_str = payload.to_string();
    let js = format!(
        "window.__shift_translate_result_{} = {};",
        callback, json_str
    );
    if let Err(e) = wv.eval(&js) {
        log::warn!("[translate] eval 结果失败: {}", e);
    } else {
        log::debug!("[translate] 已写入结果 callback={}", callback);
    }
}

/// P2：callback 参数白名单——只允许 [A-Za-z0-9_]，长度 1~64。
/// callback 会被拼进 JS 变量名并 eval 到 webview，必须严格白名单，
/// 否则恶意页面可通过 shift://__translate__ 端点注入任意 JS
fn valid_callback(cb: &str) -> bool {
    !cb.is_empty() && cb.len() <= 64 && cb.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
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
      tabs::minimize_window,
      tabs::close_window,
      tabs::start_drag_window,
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
      // 隐私防护（GPC + 反指纹 + WebRTC 防泄露）
      extensions::set_privacy_shield_enabled,
      extensions::get_privacy_shield_enabled,
      // 新标签页
      newtab::refresh_newtab,
      newtab::get_newtab_url,
      newtab::save_quick_sites,
      newtab::todo_list,
      newtab::todo_save,
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
              .header(tauri::http::header::CONTENT_SECURITY_POLICY, SHIFT_CSP)
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
        // P2：callback 白名单——不合法直接拒绝，不 eval（防 JS 注入）
        if !valid_callback(&callback) {
          log::warn!("[translate] 非法 callback 拒绝: {:?}", callback);
          return tauri::http::Response::builder()
            .status(400)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain")
            .body("Bad Request".as_bytes().to_vec())
            .unwrap();
        }
        let app_clone = app.clone();

        match path.as_str() {
          "/text" => {
            let text = params.get("text").cloned().unwrap_or_default();
            log::info!(
              "[translate] 选中文本翻译 request: label={} callback={} len={}",
              webview_label, callback, text.len()
            );
            // P2：单段文本限长——恶意页面可刷超大文本放大翻译请求（外部网络调用）
            if text.len() > 5000 {
              let payload = serde_json::json!({ "error": "待翻译文本过长（>5000 字符）" });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
              return tauri::http::Response::builder()
                .status(200)
                .header(tauri::http::header::CONTENT_TYPE, "image/gif")
                .body(Vec::<u8>::new())
                .unwrap();
            }
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
            // 注入整页翻译脚本：脚本在页面内收集文本节点 → 分批调 /batch → 回填 DOM
            // （eval 是单向的，收集结果通过 /batch 的 shift:// 请求与回调变量回传）
            let wv_label = webview_label.clone();
            tauri::async_runtime::spawn(async move {
              if let Some(wv) = app_clone.get_webview(&wv_label) {
                if let Err(e) = wv.eval(WHOLE_PAGE_TRANSLATE_JS) {
                  log::warn!("[translate] 整页翻译脚本注入失败: {}", e);
                  let payload = serde_json::json!({ "error": format!("脚本注入失败: {}", e) });
                  write_translate_result(&app_clone, &wv_label, &callback, &payload);
                } else {
                  log::info!("[translate] 整页翻译脚本已注入: {}", wv_label);
                }
              } else {
                log::warn!("[translate] 找不到 webview: {}", wv_label);
              }
            });
          }
          "/batch" => {
            // 批量翻译（整页翻译脚本调用）：texts 是 JSON 数组，逐段翻（并发 5）
            let texts_json = params.get("texts").cloned().unwrap_or_default();
            log::info!(
              "[translate] 批量翻译 request: label={} bytes={}",
              webview_label, texts_json.len()
            );
            tauri::async_runtime::spawn(async move {
              let texts: Vec<String> = match serde_json::from_str(&texts_json) {
                Ok(v) => v,
                Err(e) => {
                  let payload = serde_json::json!({ "error": format!("参数解析失败: {}", e) });
                  write_translate_result(&app_clone, &webview_label, &callback, &payload);
                  return;
                }
              };
              // P2：批量限长——恶意页面可刷超大数组/超长文本放大翻译请求
              // （每段翻一次外部网络调用，20 段×2000 字符已是整页翻译的合理上限）
              if texts.len() > 20 || texts.iter().any(|t| t.len() > 2000) {
                let payload =
                  serde_json::json!({ "error": "批量翻译参数超限（≤20 段、每段 ≤2000 字符）" });
                write_translate_result(&app_clone, &webview_label, &callback, &payload);
                return;
              }
              let mut out: Vec<String> = vec![String::new(); texts.len()];
              // 分批并发（每批 5 段），保持顺序
              for chunk_start in (0..texts.len()).step_by(5) {
                let chunk_end = (chunk_start + 5).min(texts.len());
                let mut handles = Vec::new();
                for (offset, t) in texts[chunk_start..chunk_end].iter().enumerate() {
                  let i = chunk_start + offset;
                  let t = t.clone();
                  handles.push((
                    i,
                    tauri::async_runtime::spawn(async move {
                      translate::translate_google(&t, "zh-CN").await
                    }),
                  ));
                }
                for (i, h) in handles {
                  if let Ok(Ok(t)) = h.await {
                    out[i] = t;
                  }
                }
              }
              let payload = serde_json::json!({ "texts": out });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
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
          .header(tauri::http::header::CONTENT_SECURITY_POLICY, SHIFT_CSP)
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
      // 锁屏是否启用过（解锁后通知前端重试初始化——锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 命令调用全部失败，必须等 setup 完成后重试）
      let mut unlocked_after_lock = false;

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

      // 锁屏前预生成新标签页 + 后台刷新壁纸——锁屏 dialog 阻塞 setup 期间，
      // 这些异步任务照常跑（tokio 不依赖 GTK 主循环），解锁时 index.html 已就绪，
      // 前端 createTab 的 refresh_newtab 拿 GEN_LOCK 时防抖直接跳过，秒开启动页。
      // generate 关键路径已改为只读缓存不拉网络（newtab.rs），纯本地毫秒级。
      {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
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
          unlocked_after_lock = true;
          // 标记锁屏中：前端 is_locked 查到 true 后静默等待 app-unlocked，
          // 不再反复重试建标签（否则弹"创建标签页失败"toast）
          LOCKED.store(true, std::sync::atomic::Ordering::SeqCst);
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
                if let Err(e) = tauri::async_runtime::block_on(users::clear_session(&db_state)) {
                  log::warn!("[ShiftBrowser] 启动锁屏验证通过但会话清除失败: {}", e);
                }
                // 显式 destroy 锁屏 dialog，避免 close 后残留；forget 阻止 drop 再次释放
                unsafe {
                  dialog.destroy();
                }
                std::mem::forget(dialog);
                // 立即恢复主窗口前台/焦点——否则用户必须点一下窗体才能交互
                // （modal dialog 关闭后焦点不自动回到父窗口）
                if let Some(ref pw) = parent_win {
                  pw.present();
                }
                // 解锁完成：清除锁屏标志（前端收到 app-unlocked 后开始初始化）
                LOCKED.store(false, std::sync::atomic::Ordering::SeqCst);
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

          // 隐私防护 P0：第三方 Cookie 默认阻断（只允许第一方 Cookie）。
          // 注意必须在创建 child webview 之前设置才生效。
          #[cfg(target_os = "linux")]
          {
            use webkit2gtk::CookieManagerExt;
            if let Some(cm) = context.cookie_manager() {
              cm.set_accept_policy(webkit2gtk::CookieAcceptPolicy::NoThirdParty);
              log::info!("[privacy] 第三方 Cookie 已阻断（CookieAcceptPolicy::NoThirdParty）");
            }
          }
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

      // 网络状态监测（状态栏：下行/上行/延迟，每 2 秒推送）
      netmon::start_network_monitor(app.handle().clone());

      // 锁屏解锁过 → 通知前端重试初始化（锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 初始化全部失败，解锁后必须重跑 loadTabs/createTab）
      if unlocked_after_lock {
        let _ = app.emit("app-unlocked", ());
        log::info!("[ShiftBrowser] 已发出 app-unlocked，前端将重试初始化");
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_whitelist_accepts_normal() {
        assert!(valid_callback("cb_1"));
        assert!(valid_callback("a"));
        assert!(valid_callback("_"));
        assert!(valid_callback("x".repeat(64).as_str()));
    }

    #[test]
    fn callback_whitelist_rejects_injection() {
        // 恶意 callback 注入 JS：拼进变量名 eval 到特权 webview
        assert!(!valid_callback(""));
        assert!(!valid_callback("x".repeat(65).as_str()));
        assert!(!valid_callback("cb;alert(1)//"));
        assert!(!valid_callback("cb'));alert(1);//"));
        assert!(!valid_callback("cb\"));alert(1);//"));
        assert!(!valid_callback("cb\nalert(1)"));
        assert!(!valid_callback("cb-1"));
        assert!(!valid_callback("cb.1"));
        assert!(!valid_callback("中文"));
    }
}

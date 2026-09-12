// 标签页管理器
// 管理标签元数据 + 嵌入式 webview 生命周期
// 与 Electron 版 electron/tabManager.ts 对应（v4.5.0）

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl};

use crate::db::{history, Db};

#[cfg(target_os = "linux")]
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use webkit2gtk::{DownloadExt, SettingsExt, URIRequestExt, URIResponseExt, WebContextExt};
use webkit2gtk::WebViewExt;

// UI 布局常量（与前端 CSS 保持一致）
pub const TAB_BAR_HEIGHT: f64 = 40.0;
pub const ADDRESS_BAR_HEIGHT: f64 = 42.0;
pub const BOOKMARKS_BAR_HEIGHT: f64 = 30.0;
pub const STATUS_BAR_HEIGHT: f64 = 22.0;

/// 内容区顶部偏移 = 标签栏 + 地址栏 + 书签栏
pub const CONTENT_TOP_OFFSET: f64 =
  TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT + BOOKMARKS_BAR_HEIGHT;

/// 标签页元数据（运行时状态，不持久化；会话保存走 sessions 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
  pub id: String,
  pub url: String,
  pub title: String,
  pub favicon: String,
  pub is_active: bool,
  pub is_pinned: bool,
}

/// 标签管理器（Tauri State）
pub struct TabManager {
  tabs: Mutex<HashMap<String, Tab>>,
  order: Mutex<Vec<String>>,
  active_id: Mutex<Option<String>>,
}

impl TabManager {
  pub fn new() -> Self {
    Self {
      tabs: Mutex::new(HashMap::new()),
      order: Mutex::new(Vec::new()),
      active_id: Mutex::new(None),
    }
  }

  /// 生成简单 UUID（避免额外依赖）
  /// 注意：{:08x} 强制补零，保证 s 恰好 32 字符，避免切片越界
  fn uuid() -> String {
    let s = format!(
      "{:08x}{:08x}{:08x}{:08x}",
      rand_u32(),
      rand_u32(),
      rand_u32(),
      rand_u32()
    );
    format!(
      "{}-{}-4{}-{}-{}",
      &s[0..8],
      &s[8..12],
      &s[12..15],
      &s[15..19],
      &s[19..31]
    )
  }

  fn all_tabs(&self) -> Vec<Tab> {
    let tabs = self.tabs.lock().unwrap();
    let order = self.order.lock().unwrap();
    order
      .iter()
      .filter_map(|id| tabs.get(id).cloned())
      .collect()
  }

  /// 根据窗口尺寸计算内容区域大小（逻辑像素）
  fn content_size(window: &tauri::Window) -> (f64, f64) {
    let inner = window.inner_size().unwrap_or(tauri::PhysicalSize::new(1280, 800));
    let scale = window.scale_factor().unwrap_or(1.0);
    let w = inner.width as f64 / scale;
    let h = inner.height as f64 / scale;
    (w, (h - CONTENT_TOP_OFFSET - STATUS_BAR_HEIGHT).max(100.0))
  }

  /// 创建嵌入式 webview
  fn create_webview(
    app: &AppHandle,
    id: &str,
    url: &str,
  ) -> Result<(), String> {
    let window = app
      .get_window("main")
      .ok_or_else(|| "主窗口不存在".to_string())?;

    let parsed_url = url
      .parse::<tauri::Url>()
      .map_err(|e| format!("URL 解析失败: {}", e))?;

    let (w, h) = Self::content_size(&window);
    println!("[tabs] create_webview: inner={:?} content=({:.0},{:.0})", window.inner_size(), w, h);

    // 页面加载完成时同步标题（用 host 作标题，MVP 阶段）
    let app_handle = app.clone();
    let app_handle2 = app_handle.clone();
    let tab_id = id.to_string();
    let label = format!("tab-{}", id);
    let child = window
      .add_child(
        WebviewBuilder::new(label.clone(), WebviewUrl::External(parsed_url))
          // target=_blank / window.open 新窗口请求：统一创建新标签页打开链接
          .on_new_window(move |url, _features| {
            println!("[tabs] on_new_window: url={}", url);
            let app = app_handle.clone();
            let url_str = url.to_string();
            tauri::async_runtime::spawn(async move {
              let state = app.state::<TabManager>();
              if let Ok(tab) = create_tab(app.clone(), state, url_str).await {
                println!("[tabs] on_new_window: 新标签创建成功 id={}", tab.id);
              }
            });
            tauri::webview::NewWindowResponse::Deny
          })
.on_page_load(move |_webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
              let url = payload.url();
              let host = url.host_str().unwrap_or("").to_string();
              let title = if host.is_empty() {
                url.to_string()
              } else {
                host
              };

              println!("[tabs] page_load_finished: tab={} title={}", tab_id, title);

              // 按设置记录浏览历史（record_history 开关，默认开）——先 clone，避免 title 被 move
              let app3 = app_handle2.clone();
              let url_str = url.to_string();
              let title_opt = Some(title.clone());

              if let Some(state) = app_handle2.try_state::<TabManager>() {
                let mut tabs = state.tabs.lock().unwrap();
                if let Some(tab) = tabs.get_mut(&tab_id) {
                  tab.url = url.to_string();
                  tab.title = title;
                }
              }

              tauri::async_runtime::spawn(async move {
                let db = app3.state::<Db>();
                let enabled: Option<(String,)> = sqlx::query_as(
                  "SELECT value FROM settings WHERE key = 'record_history'",
                )
                .fetch_optional(&db.pool)
                .await
                .ok()
                .flatten();
                let record = enabled.map(|v| v.0 != "false").unwrap_or(true);
                if record {
                  if let Err(e) = history::record_history(db, url_str, title_opt).await {
                    println!("[tabs] record_history 失败: {}", e);
                  }
                }
              });
            }
          }),
        LogicalPosition::new(0.0, CONTENT_TOP_OFFSET),
        LogicalSize::new(w, h),
      )
      .map_err(|e| format!("创建 webview 失败: {}", e))?;

    println!("[tabs] child_webview: pos={:?} size={:?}", child.position().unwrap(), child.size().unwrap());

    // GTK 的 vbox 会忽略 add_child 的位置参数并平分空间（webview 落到窗口下半部分）。
    // 解法：把主 webview 和所有 child webview reparent 到 GtkFixed 容器，实现绝对定位叠层。
    #[cfg(target_os = "linux")]
    {
      let db = app.state::<Db>();
      Self::fix_gtk_layout(&child, w, h, db.pool.clone(), app.clone())?;
    }
    Ok(())
  }

/// Linux/GTK 布局修复：把 child webview reparent 到 GtkFixed 容器，定位到内容区
  /// 结构：窗口 vbox → [GtkFixed] → 主 webview(0,0 全屏) + child webview(0,82 内容区)
  /// with_webview 闭包在主线程执行；gtk 对象在闭包内创建/查找，不跨线程捕获
  #[cfg(target_os = "linux")]
  fn fix_gtk_layout(
    child: &tauri::Webview,
    w: f64,
    h: f64,
    db_pool: sqlx::SqlitePool,
    app: AppHandle,
  ) -> Result<(), String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let child = child.clone();

    child
      .with_webview(move |pwv| {
        let child_gtk = pwv.inner();
        // 从 parent 拿 vbox（不调 window 方法，避免主线程死锁）
        if let Some(vbox) = child_gtk
          .parent()
          .and_then(|p| p.dynamic_cast::<gtk::Box>().ok())
        {
          let fixed = find_or_create_fixed(&vbox);
          // 标准 reparent（gtk_widget_reparent FFI）：正确处理 GDK 窗口/输入窗口跟随新 parent，
          // 避免 remove+put 导致 WebKitWebView 输入窗口不跟随（链接点击失效）
          unsafe {
            gtk::ffi::gtk_widget_reparent(
              child_gtk.upcast_ref::<gtk::Widget>().to_glib_none().0,
              fixed.upcast_ref::<gtk::Widget>().to_glib_none().0,
            );
          }
          child_gtk.set_size_request(w as i32, h as i32);
          // 允许 JS 自动打开窗口（target=_blank / window.open 触发 create 信号）
          // 注意：不能新建 Settings 替换（会丢失 Tauri 默认设置导致链接点击失效），
          // 只能修改现有 Settings 的属性
          if let Some(settings) = webkit2gtk::WebViewExt::settings(&child_gtk) {
            settings.set_javascript_can_open_windows_automatically(true);
          }
          // 监听 child webview 的鼠标点击事件
          child_gtk.connect_button_press_event(|wv, event| {
              let (x, y) = (event.position().0, event.position().1);
              println!(
                  "[tabs] child button_press: button={} pos=({},{})",
                  event.button(), x, y
              );
              // WebKit 需要焦点处理 DOM 点击事件；强制聚焦 child webview
              wv.grab_focus();
              // 链接点击修复：WebKit 输入窗口在 reparent 后不处理 DOM 点击（链接不导航），
              // 用 JS 检查点击位置的链接并 window.open 打开新标签页（触发 on_new_window 创建新 tab）
              let js = format!(
                  "(function(){{var el=document.elementFromPoint({},{});var a=el&&el.closest?el.closest('a[href]'):null;if(a&&a.href)window.open(a.href);}})()",
                  x, y
              );
              let _ = wv.run_javascript(&js, None::<&gtk::gio::Cancellable>, |_| {});
              false.into() // 不拦截，让 webview 处理
          });
          child_gtk.connect_button_release_event(|_wv, event| {
              println!(
                  "[tabs] child button_release: button={} pos=({},{})",
                  event.button(),
                  event.position().0,
                  event.position().1
              );
              false.into()
          });
          // 下载信号：网页触发下载（点击下载链接等）→ 记录到 downloads 表 + 进度/完成/失败更新
          // WebView 无 download 信号，用 WebContext 的 download_started（全局注册一次）
          // 下载目的地默认 ~/Downloads/<文件名>
          static DOWNLOAD_HOOK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
          if let Some(ctx) = child_gtk.context() {
            if !DOWNLOAD_HOOK.swap(true, std::sync::atomic::Ordering::SeqCst) {
              let dl_pool = db_pool.clone();
              let dl_app = app.clone();
              ctx.connect_download_started(move |_ctx, download| {
                println!("[downloads] 下载开始信号");
                // 文件名：从 URL 推断（URL 最后一段，去 query）
                let url = download
                  .request()
                  .and_then(|r| r.uri())
                  .map(|s| s.to_string());
                let filename = url
                  .as_deref()
                  .map(|u| {
                    let path = u.split('?').next().unwrap_or(u);
                    let name = path.rsplit('/').next().unwrap_or("download").to_string();
                    if name.is_empty() { "download".to_string() } else { name }
                  })
                  .unwrap_or_else(|| "download".to_string());
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                let dest_dir = format!("{}/Downloads", home);
                let _ = std::fs::create_dir_all(&dest_dir);
                let dest = format!("{}/{}", dest_dir, filename);
                download.set_destination(&format!("file://{}", dest));
                // 总字节：从 response 拿（可能未知为 0）
                let total = download
                  .response()
                  .map(|r| r.content_length() as i64)
                  .unwrap_or(0);

                // 共享下载记录 id（异步插入完成后设置；进度/完成/失败回调等 id 就绪）
                // 注意：绝不能在 GTK 主线程回调里 block_on（会死锁卡死 UI），全部走 spawn 异步
                let id_state: Arc<Mutex<Option<i64>>> = Arc::new(Mutex::new(None));

                // 异步插入记录
                let pool0 = dl_pool.clone();
                let app0 = dl_app.clone();
                let filename_db = filename.clone();
                let url_db = url.clone();
                let dest_db = dest.clone();
                let id_state0 = id_state.clone();
                tauri::async_runtime::spawn(async move {
                  let res = sqlx::query(
                    "INSERT INTO downloads (filename, url, path, total_bytes, received_bytes, state) VALUES (?, ?, ?, ?, 0, 'in_progress')",
                  )
                  .bind(&filename_db)
                  .bind(&url_db)
                  .bind(Some(&dest_db))
                  .bind(total)
                  .execute(&pool0)
                  .await;
                  if let Ok(r) = res {
                    let id = r.last_insert_rowid();
                    *id_state0.lock().unwrap() = Some(id);
                    println!("[downloads] 记录 id={} filename={} total={}", id, filename_db, total);
                    let _ = app0.emit("downloads-updated", ());
                  } else {
                    println!("[downloads] 记录插入失败");
                  }
                });

                // 等待下载记录 id 就绪（最多 5 秒）
                async fn wait_download_id(state: &Arc<Mutex<Option<i64>>>) -> Option<i64> {
                  for _ in 0..100 {
                    if let Some(id) = *state.lock().unwrap() {
                      return Some(id);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                  }
                  None
                }

                // 进度回调（每次收到数据，异步更新，不阻塞主线程）
                // 节流：500ms 内最多写一次数据库（下载快时 received_data 每秒多次，全写会锁竞争）
                let pool2 = dl_pool.clone();
                let app2 = dl_app.clone();
                let id_state2 = id_state.clone();
                let last_progress: Arc<Mutex<std::time::Instant>> =
                  Arc::new(Mutex::new(std::time::Instant::now()));
                download.connect_received_data(move |dl, _len| {
                  let received = dl.received_data_length() as i64;
                  // 总字节可能下载开始时未知（0），进度回调时再补一次
                  let total_now = dl.response().map(|r| r.content_length() as i64).unwrap_or(0);
                  let mut last = last_progress.lock().unwrap();
                  if last.elapsed() < std::time::Duration::from_millis(500) {
                    return;
                  }
                  *last = std::time::Instant::now();
                  drop(last);
                  let pool3 = pool2.clone();
                  let app3 = app2.clone();
                  let id_state3 = id_state2.clone();
                  tauri::async_runtime::spawn(async move {
                    let id_now = { *id_state3.lock().unwrap() };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state3).await,
                    };
                    if let Some(id) = id {
                      let _ = sqlx::query(
                        "UPDATE downloads SET received_bytes = ?, total_bytes = CASE WHEN total_bytes = 0 THEN ? ELSE total_bytes END WHERE id = ?",
                      )
                      .bind(received)
                      .bind(total_now)
                      .bind(id)
                      .execute(&pool3)
                      .await;
                      let _ = app3.emit("downloads-updated", ());
                    }
                  });
                });

                // 完成回调
                let pool4 = dl_pool.clone();
                let app4 = dl_app.clone();
                let id_state4 = id_state.clone();
                download.connect_finished(move |dl| {
                  let received = dl.received_data_length() as i64;
                  let pool5 = pool4.clone();
                  let app5 = app4.clone();
                  let id_state5 = id_state4.clone();
                  tauri::async_runtime::spawn(async move {
                    let id_now = { *id_state5.lock().unwrap() };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state5).await,
                    };
                    if let Some(id) = id {
                      let _ = sqlx::query(
                        "UPDATE downloads SET received_bytes = ?, state = 'completed' WHERE id = ?",
                      )
                      .bind(received)
                      .bind(id)
                      .execute(&pool5)
                      .await;
                      let _ = app5.emit("downloads-updated", ());
                    }
                  });
                });

                // 失败回调
                let pool6 = dl_pool.clone();
                let app6 = dl_app.clone();
                let id_state6 = id_state.clone();
                download.connect_failed(move |_dl, err| {
                  println!("[downloads] 下载失败: {}", err);
                  let pool7 = pool6.clone();
                  let app7 = app6.clone();
                  let id_state7 = id_state6.clone();
                  tauri::async_runtime::spawn(async move {
                    let id_now = { *id_state7.lock().unwrap() };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state7).await,
                    };
                    if let Some(id) = id {
                      let _ = sqlx::query("UPDATE downloads SET state = 'failed' WHERE id = ?")
                        .bind(id)
                        .execute(&pool7)
                        .await;
                      let _ = app7.emit("downloads-updated", ());
                    }
                  });
                });
              });
              println!("[downloads] download 信号已注册");
            }
          }
          child_gtk.show();
          println!(
            "[tabs] child_webview reparent 到 Fixed: pos=(0,{}) size=({:.0},{:.0})",
            CONTENT_TOP_OFFSET, w, h
          );
          // 布局完成后打印实际分配尺寸（idle 延迟，等 GTK 布局循环）
          let child_gtk2 = child_gtk.clone();
          gtk::glib::idle_add_local_once(move || {
            let alloc = child_gtk2.allocation();
            println!(
              "[tabs] child allocation: {}x{} at ({},{})",
              alloc.width(),
              alloc.height(),
              alloc.x(),
              alloc.y()
            );
          });
        } else {
          println!("[tabs] child_webview 的 parent 不是 GtkBox，跳过 reparent");
        }
        let _ = tx.send(());
      })
      .map_err(|e| format!("child webview 句柄获取失败: {}", e))?;

    rx.recv().map_err(|e| format!("主线程布局失败: {}", e))?;
    Ok(())
  }

  /// 切换 webview 显隐
  fn set_webview_visible(app: &AppHandle, id: &str, visible: bool) -> Result<(), String> {
    let window = app
      .get_window("main")
      .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    if let Some(webview) = window.get_webview(&label) {
      if visible {
        let _ = webview.show();
        let _ = webview.set_focus();
      } else {
        let _ = webview.hide();
      }
    }
    Ok(())
  }

  /// 关闭并销毁 webview
  fn destroy_webview(app: &AppHandle, id: &str) {
    let window = match app.get_window("main") {
      Some(w) => w,
      None => return,
    };
    let label = format!("tab-{}", id);
    if let Some(webview) = window.get_webview(&label) {
      let _ = webview.close();
    }
  }
}

/// 应用启动时初始化 GTK 布局（setup 阶段调用，主线程）
/// 布局结构：窗口 vbox → [GtkOverlay] → 主 webview（主层，自动填满）+ GtkFixed（上层，child webviews 绝对定位）
/// 主 webview 在 overlay 主层自动跟随窗口（不手动改 size_request，避免撑大窗口 natural size 导致无法缩小）
#[cfg(target_os = "linux")]
pub fn setup_gtk_layout(app: &tauri::App) -> Result<(), String> {
  if let Some(main_wv) = app.get_webview("main") {
    main_wv
      .with_webview(move |pwv| {
        let main_gtk = pwv.inner();
        // 从 parent 拿 vbox（不调 window 方法，避免主线程死锁）
        if let Some(vbox) = main_gtk
          .parent()
          .and_then(|p| p.dynamic_cast::<gtk::Box>().ok())
        {
          // 创建 GtkOverlay 挂到 vbox（填满窗口）
          let overlay = gtk::Overlay::new();
          vbox.remove(&main_gtk);
          vbox.pack_start(&overlay, true, true, 0);
          overlay.show();

          // 主 webview 放主层：自动填满 overlay，natural size 固定 1280x800
          overlay.add(&main_gtk);
          main_gtk.set_size_request(1280, 800);
          main_gtk.show();

          // GtkFixed 放上层：child webviews 绝对定位
          // 关键：Fixed 用 margin_top 定位到内容区（y=112 起），避免覆盖地址栏拦截点击事件
          let fixed = gtk::Fixed::new();
          fixed.set_valign(gtk::Align::Start);
          fixed.set_margin_top(CONTENT_TOP_OFFSET as i32);
          fixed.set_size_request(1280, 800 - CONTENT_TOP_OFFSET as i32 - STATUS_BAR_HEIGHT as i32);
          overlay.add_overlay(&fixed);
          fixed.show();

          println!("[tabs] setup: GtkOverlay 布局初始化完成");
        } else {
          println!("[tabs] setup: 主 webview 的 parent 不是 GtkBox，跳过 reparent");
        }
      })
      .map_err(|e| format!("主 webview 句柄获取失败: {}", e))?;
  }

  // 注册窗口 resize 监听：同步 child webviews 和 Fixed 容器（主 webview 在 overlay 主层自动跟随窗口）
  let app_handle = app.handle().clone();
  if let Some(window) = app.get_window("main") {
    window.on_window_event(move |event| {
      if let tauri::WindowEvent::Resized(size) = event {
        let w = size.width as i32;
        let h = size.height as i32;
        let content_h = (h as f64 - CONTENT_TOP_OFFSET - STATUS_BAR_HEIGHT).max(100.0) as i32;
        println!(
          "[tabs] window resized: {}x{} content_h={}",
          w, h, content_h
        );

        // child webviews：Fixed 内位置 (0,0)，尺寸 (w, content_h)；Fixed 本身在窗口 (0, CONTENT_TOP_OFFSET)
        if let Some(state) = app_handle.try_state::<TabManager>() {
          let ids: Vec<String> = state.tabs.lock().unwrap().keys().cloned().collect();
          for id in ids {
            let label = format!("tab-{}", id);
            if let Some(wv) = app_handle.get_webview(&label) {
              wv.with_webview(move |pwv| {
                let gtk_wv = pwv.inner();
                if let Some(fixed) = gtk_wv
                  .parent()
                  .and_then(|p| p.dynamic_cast::<gtk::Fixed>().ok())
                {
                  fixed.move_(&gtk_wv, 0, 0);
                  fixed.set_size_request(w, content_h);
                }
                gtk_wv.set_size_request(w, content_h);
              });
            }
          }
        }
      }
    });
  }

  Ok(())
}

/// 从 vbox 查找 GtkOverlay 里的 GtkFixed 容器，不存在则创建并挂到 vbox（兜底）
#[cfg(target_os = "linux")]
fn find_or_create_fixed(vbox: &gtk::Box) -> gtk::Fixed {
  // 优先：vbox → GtkOverlay → GtkFixed
  if let Some(overlay) = vbox
    .children()
    .iter()
    .find(|c| c.type_().name() == "GtkOverlay")
    .and_then(|c| c.dynamic_cast_ref::<gtk::Overlay>().map(|o| o.clone()))
  {
    if let Some(f) = overlay
      .children()
      .iter()
      .find(|c| c.type_().name() == "GtkFixed")
      .and_then(|c| c.dynamic_cast_ref::<gtk::Fixed>().map(|f| f.clone()))
    {
      return f;
    }
  }
  // 兜底：vbox 直接找 Fixed
  if let Some(f) = vbox
    .children()
    .iter()
    .find(|c| c.type_().name() == "GtkFixed")
    .and_then(|c| c.dynamic_cast_ref::<gtk::Fixed>().map(|f| f.clone()))
  {
    return f;
  }
  let f = gtk::Fixed::new();
  vbox.pack_start(&f, true, true, 0);
  f.show();
  f
}

/// 随机数（简单 xorshift，够用即可）
fn rand_u32() -> u32 {
  use std::time::{SystemTime, UNIX_EPOCH};
  let nanos = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|d| d.subsec_nanos())
    .unwrap_or(0);
  let mut x = nanos.wrapping_mul(0x9E3779B9).wrapping_add(0x85EBCA6B);
  x ^= x >> 13;
  x = x.wrapping_mul(0xC2B2AE35);
  x ^ (x >> 16)
}

/// 创建新标签页
#[tauri::command]
pub async fn create_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  url: String,
) -> Result<Tab, String> {
  println!("[tabs] create_tab: url={}", url);
  let id = TabManager::uuid();
  let title = url.clone();

  TabManager::create_webview(&app, &id, &url)?;
  println!("[tabs] create_tab: webview 创建成功 id={}", id);

  let tab = Tab {
    id: id.clone(),
    url,
    title,
    favicon: String::new(),
    is_active: true,
    is_pinned: false,
  };

  // 更新状态：新标签激活，旧标签取消激活
  {
    let mut tabs = state.tabs.lock().unwrap();
    let mut order = state.order.lock().unwrap();
    let mut active = state.active_id.lock().unwrap();

    for t in tabs.values_mut() {
      t.is_active = false;
    }
    tabs.insert(id.clone(), tab.clone());
    order.push(id.clone());
    *active = Some(id.clone());
  }

  Ok(tab)
}

/// 关闭标签页（切换到相邻标签）
#[tauri::command]
pub async fn close_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] close_tab: id={}", id);
  let mut next_id: Option<String> = None;

  {
    let mut tabs = state.tabs.lock().unwrap();
    let mut order = state.order.lock().unwrap();
    let mut active = state.active_id.lock().unwrap();

    let was_active = *active == Some(id.clone());
    let removed = tabs.remove(&id);
    if let Some(pos) = order.iter().position(|x| x == &id) {
      order.remove(pos);
    }

    if was_active {
      // 优先切到右边的标签，没有则切左边
      if let Some(pos) = order.iter().position(|x| x == &id) {
        let _ = pos;
      }
      next_id = order.last().cloned();
      if let Some(nid) = &next_id {
        if let Some(t) = tabs.get_mut(nid) {
          t.is_active = true;
        }
      }
      *active = next_id.clone();
    }

    if removed.is_none() {
      return Ok(None);
    }
  }

  TabManager::destroy_webview(&app, &id);

  // 激活下一个标签的 webview
  if let Some(nid) = &next_id {
    let _ = TabManager::set_webview_visible(&app, nid, true);
  }

  let tabs = state.tabs.lock().unwrap();
  Ok(next_id.and_then(|nid| tabs.get(&nid).cloned()))
}

/// 关闭全部标签（切换工作区时清空用）
#[tauri::command]
pub async fn close_all_tabs(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
  println!("[tabs] close_all_tabs");
  let ids: Vec<String> = {
    let mut tabs = state.tabs.lock().unwrap();
    let mut order = state.order.lock().unwrap();
    let mut active = state.active_id.lock().unwrap();
    let collected: Vec<String> = tabs.keys().cloned().collect();
    tabs.clear();
    order.clear();
    *active = None;
    collected
  };

  for id in ids {
    TabManager::destroy_webview(&app, &id);
  }
  Ok(())
}

/// 切换标签页
#[tauri::command]
pub async fn switch_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] switch_tab: id={}", id);
  // 先隐藏当前活动 webview
  let old_active = {
    let mut active = state.active_id.lock().unwrap();
    let old = active.clone();
    *active = Some(id.clone());
    old
  };
  if let Some(old) = &old_active {
    if old != &id {
      let _ = TabManager::set_webview_visible(&app, old, false);
    }
  }

  // 更新激活状态
  {
    let mut tabs = state.tabs.lock().unwrap();
    let exists = tabs.contains_key(&id);
    if !exists {
      return Ok(None);
    }
    for t in tabs.values_mut() {
      t.is_active = t.id == id;
    }
  }

  // 显示目标 webview
  let _ = TabManager::set_webview_visible(&app, &id, true);

  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 导航到新 URL
#[tauri::command]
pub async fn navigate_tab(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  id: String,
  url: String,
) -> Result<Option<Tab>, String> {
  println!("[tabs] navigate_tab: id={} url={}", id, url);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;

  let parsed_url = url
    .parse::<tauri::Url>()
    .map_err(|e| format!("URL 解析失败: {}", e))?;
  webview
    .navigate(parsed_url)
    .map_err(|e| format!("导航失败: {}", e))?;

  let mut tabs = state.tabs.lock().unwrap();
  let mut tab = tabs.get_mut(&id);
  if let Some(ref mut t) = tab {
    t.url = url.clone();
    t.title = url;
  }
  Ok(tab.cloned())
}

/// 获取所有标签页（按顺序）
#[tauri::command]
pub async fn get_tabs(state: tauri::State<'_, TabManager>) -> Result<Vec<Tab>, String> {
  Ok(state.all_tabs())
}

/// 隐藏/显示所有标签页 webview（弹窗/菜单等 Vue UI 覆盖层需要）
#[tauri::command]
pub async fn set_webview_visible(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
  visible: bool,
) -> Result<(), String> {
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  // 注意：child webview 不在 window.webviews() 列表里（只有 main），
  // 必须从 TabManager 拿已知标签逐个 get_webview
  let ids: Vec<String> = state.tabs.lock().unwrap().keys().cloned().collect();
  let mut count = 0usize;
  for id in ids {
    let label = format!("tab-{}", id);
    if let Some(wv) = window.get_webview(&label) {
      let wv2 = wv.clone();
      wv.with_webview(move |pwv| {
        let gtk_wv = pwv.inner();
        // 同时隐藏/显示 Fixed 容器：Fixed 覆盖内容区会拦截弹窗点击，必须一起隐藏
        if let Some(fixed) = gtk_wv
          .parent()
          .and_then(|p| p.dynamic_cast::<gtk::Fixed>().ok())
        {
          if visible {
            fixed.show();
          } else {
            fixed.hide();
          }
        }
        if visible {
          wv2.show();
        } else {
          wv2.hide();
        }
      });
      count += 1;
    }
  }
  println!(
    "[tabs] set_webview_visible: visible={} count={}",
    visible, count
  );
  Ok(())
}

/// 后退（对当前活动标签执行 history.back()）
#[tauri::command]
pub async fn go_back(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
  let id = state.active_id.lock().unwrap().clone();
  let Some(id) = id else {
    return Ok(None);
  };
  println!("[tabs] go_back: tab={}", id);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
  webview
    .eval("history.back()")
    .map_err(|e| format!("后退失败: {}", e))?;
  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 前进（对当前活动标签执行 history.forward()）
#[tauri::command]
pub async fn go_forward(
  app: AppHandle,
  state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
  let id = state.active_id.lock().unwrap().clone();
  let Some(id) = id else {
    return Ok(None);
  };
  println!("[tabs] go_forward: tab={}", id);
  let window = app
    .get_window("main")
    .ok_or_else(|| "主窗口不存在".to_string())?;
  let label = format!("tab-{}", id);
  let webview = window
    .get_webview(&label)
    .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
  webview
    .eval("history.forward()")
    .map_err(|e| format!("前进失败: {}", e))?;
  let tabs = state.tabs.lock().unwrap();
  Ok(tabs.get(&id).cloned())
}

/// 更新标签页元数据（标题/图标等，供前端同步）
#[tauri::command]
pub async fn update_tab(
  state: tauri::State<'_, TabManager>,
  id: String,
  title: Option<String>,
  favicon: Option<String>,
) -> Result<Option<Tab>, String> {
  let mut tabs = state.tabs.lock().unwrap();
  let mut tab = tabs.get_mut(&id);
  if let Some(ref mut t) = tab {
    if let Some(new_title) = title {
      t.title = new_title;
    }
    if let Some(new_favicon) = favicon {
      t.favicon = new_favicon;
    }
  }
  Ok(tab.cloned())
}
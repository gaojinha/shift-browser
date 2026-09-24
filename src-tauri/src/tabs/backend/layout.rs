//! GTK 布局、分屏几何与开发者工具（R2-1 拆分）

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager};

use crate::db::downloads;

use super::super::model::TabManager;
use super::super::util::sanitize_filename;
use crate::engine::layout::CONTENT_TOP_OFFSET;

use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use webkit2gtk::{DownloadExt, URIRequestExt, URIResponseExt, WebContextExt, WebViewExt};
use crate::engine::webview::WebViewBackend;

use super::{UI_FOCUSED, UI_OVERLAY_ACTIVE};
use super::unique_download_path;

impl TabManager {
    /// Linux/GTK 布局修复：把 child webview reparent 到 GtkFixed 容器，定位到内容区
    /// 结构：窗口 vbox → [GtkFixed] → 主 webview(0,0 全屏) + child webview(0,82 内容区)
    /// with_webview 闭包在主线程执行；gtk 对象在闭包内创建/查找，不跨线程捕获
    #[cfg(target_os = "linux")]
    pub(crate) fn fix_gtk_layout(
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
        if let Some(vbox) = crate::engine::layout::parent_vbox(&child_gtk)
        {
          let fixed = crate::engine::layout::find_or_create_fixed(&vbox);
          // 标准 reparent（gtk_widget_reparent FFI）：正确处理 GDK 窗口/输入窗口跟随新 parent，
          // 避免 remove+put 导致 WebKitWebView 输入窗口不跟随（链接点击失效）
          unsafe {
            gtk::ffi::gtk_widget_reparent(
              child_gtk.upcast_ref::<gtk::Widget>().to_glib_none().0,
              fixed.upcast_ref::<gtk::Widget>().to_glib_none().0,
            );
          }
          child_gtk.set_size_request(w as i32, h as i32);
          // 焦点跟踪：页面（child webview）获得焦点 —— 此时前端收不到按键，GTK 层兜底转发
          child_gtk.connect_focus_in_event(|_, _| {
            UI_FOCUSED.store(false, std::sync::atomic::Ordering::Relaxed);
            gtk::glib::Propagation::Proceed
          });
          // 允许 JS 自动打开窗口（target=_blank / window.open 触发 create 信号）
          // 注意：不能新建 Settings 替换（会丢失 Tauri 默认设置导致链接点击失效），
          // 只能修改现有 Settings 的属性
          // 页面默认设置（UA / 开发者工具 / JS 弹窗）统一由引擎层施加
          // host 用于站点级 UA 覆盖（方案 B · 2026-09-22）
          let ua_host = child_gtk.host();
          child_gtk.apply_default_settings(&ua_host);
          // 下载策略（关键 · 2026-09-21）：主框架导航到"不可显示的 MIME"（apk/zip/exe/…）时，
          // WebKit 默认不当下载 → 表现为"帧加载已中断"（用户报"下载不能开始"）。
          // 这里接 decide-policy：对不支持的 MIME 调 decision.download()，
          // 触发 WebContext 的 download-started（已注册，落 ~/Downloads + 记录 downloads 表）。
          child_gtk.attach_download_policy();
          // 监听 child webview 的鼠标点击事件
          child_gtk.connect_button_press_event(|wv, event| {
              let (x, y) = (event.position().0, event.position().1);
              // 高频事件（每次点击/滚动都触发）→ debug 级，避免刷屏（RUST_LOG=debug 可开）
              log::debug!(
                  "[tabs] child button_press: button={} pos=({},{})",
                  event.button(), x, y
              );
              // WebKit 需要焦点处理 DOM 点击事件；强制聚焦 child webview
              wv.grab_focus();
              // 链接点击修复：WebKit 输入窗口在 reparent 后不处理 DOM 点击（链接不导航），
              // 用 JS 检查点击位置的链接；仅 target=_blank 用 window.open 开新标签（触发 on_new_window），
              // 普通链接不拦截，交给 WebKit 原生导航（当前标签），避免所有链接都开新标签
              let js = format!(
                  "(function(){{var el=document.elementFromPoint({},{});var a=el&&el.closest?el.closest('a[href]'):null;if(a&&a.target==='_blank'&&a.href)window.open(a.href);}})()",
                  x, y
              );
              // 引擎层统一封装（WebKit 2.40+ 弃用 run_javascript，改用 evaluate_javascript）
              crate::engine::webview::eval_js(wv, &js);
              false.into() // 不拦截，让 webview 处理
          });
          child_gtk.connect_button_release_event(|_wv, event| {
              log::debug!(
                  "[tabs] child button_release: button={} pos=({},{})",
                  event.button(),
                  event.position().0,
                  event.position().1
              );
              false.into()
          });
          // 滚轮事件：GTK 滚轮事件发给鼠标下的 widget，但 reparent 后若 webview 未聚焦，
          // DOM 滚动可能不生效（与链接点击同源的输入窗口问题）。滚动时强制聚焦 + 记录日志诊断。
          child_gtk.connect_scroll_event(|wv, event| {
              let (dx, dy) = event.delta();
              // 滚轮事件最密集 → debug 级（每次滚动都 info 会刷爆日志）
              log::debug!(
                  "[tabs] child scroll: delta=({:.2},{:.2}) dir={:?}",
                  dx, dy, event.direction()
              );
              wv.grab_focus();
              // 诊断：打印视口高度 vs 内容高度（判断 WebKit 是否错误识别页面高度）
              static SCROLL_DIAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
              if !SCROLL_DIAG.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let js = "(function(){return 'url='+location.href+' ua='+navigator.userAgent.substring(0,80)+' vh='+window.innerHeight+' sh='+document.documentElement.scrollHeight+' bh='+(document.body?document.body.scrollHeight:0)+' dpr='+window.devicePixelRatio;})()";
                crate::engine::webview::eval_js_with(wv, js, move |res| {
                    if let Ok(info) = res {
                        log::debug!("[tabs] 滚动诊断: {}", info);
                    }
                });
              }
              false.into() // 不拦截，交给 webview 处理 DOM 滚动
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
                log::info!("[downloads] 下载开始信号");
                // 文件名：从 URL 推断（URL 最后一段，去 query）
                // 安全：用 url crate 解析取解码后的 path（%2e%2e/%2f 会被解码成 ../），
                // 再经 sanitize_filename 剥离路径分隔符/点段/控制字符，防路径穿越
                let url = download
                  .request()
                  .and_then(|r| r.uri())
                  .map(|s| s.to_string());
                let filename = url
                  .as_deref()
                  .map(|u| {
                    let path = url::Url::parse(u)
                      .map(|p| p.path().to_string())
                      .unwrap_or_else(|_| u.split('?').next().unwrap_or(u).to_string());
                    let name = path.rsplit('/').next().unwrap_or("download").to_string();
                    sanitize_filename(&name)
                  })
                  .unwrap_or_else(|| "download".to_string());
                // 用系统 XDG 下载目录（~/.config/user-dirs.dirs；中文系统常是 ~/下载），
                // 避免硬编码 ~/Downloads 与实际落盘位置不一致（用户"找不到下载的文件"）。
                // P0-2：平台目录解析下沉到 engine::platform
                let dest_dir = crate::engine::platform::download_dir()
                  .map(|p| p.to_string_lossy().to_string())
                  .unwrap_or_else(|| {
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                    format!("{}/Downloads", home)
                  });
                let _ = std::fs::create_dir_all(&dest_dir);
                // 同名文件已存在时加 (1)(2) 后缀，避免覆盖
                let dest = unique_download_path(&dest_dir, &filename);
                // 目的地必须是合法 file URI——路径含中文/空格时手拼 "file://…" 非法，
                // WebKit 会取消下载（表现为"下载不能开始"）。
                let dest_uri = url::Url::from_file_path(&dest)
                  .map(|u| u.to_string())
                  .unwrap_or_else(|_| format!("file://{}", dest));
                download.set_destination(&dest_uri);
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
                  let res = downloads::insert_download(
                    &pool0,
                    &filename_db,
                    url_db.as_deref(),
                    Some(&dest_db),
                    total,
                  )
                  .await;
                  if let Ok(id) = res {
                    *id_state0.lock().unwrap_or_else(|e| e.into_inner()) = Some(id);
                    log::info!("[downloads] 记录 id={} filename={} total={}", id, filename_db, total);
                    let _ = app0.emit(crate::contracts::events::DOWNLOADS_UPDATED, ());
                  } else {
                    log::error!("[downloads] 记录插入失败");
                  }
                });

                // 等待下载记录 id 就绪（最多 5 秒）
                async fn wait_download_id(state: &Arc<Mutex<Option<i64>>>) -> Option<i64> {
                  for _ in 0..100 {
                    if let Some(id) = *state.lock().unwrap_or_else(|e| e.into_inner()) {
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
                  let mut last = last_progress.lock().unwrap_or_else(|e| e.into_inner());
                  if last.elapsed() < std::time::Duration::from_millis(500) {
                    return;
                  }
                  *last = std::time::Instant::now();
                  drop(last);
                  let pool3 = pool2.clone();
                  let app3 = app2.clone();
                  let id_state3 = id_state2.clone();
                  tauri::async_runtime::spawn(async move {
                    let id_now = { *id_state3.lock().unwrap_or_else(|e| e.into_inner()) };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state3).await,
                    };
                    if let Some(id) = id {
                      let _ = downloads::update_download_bytes(&pool3, id, received, Some(total_now)).await;
                      let _ = app3.emit(crate::contracts::events::DOWNLOADS_UPDATED, ());
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
                    let id_now = { *id_state5.lock().unwrap_or_else(|e| e.into_inner()) };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state5).await,
                    };
                    if let Some(id) = id {
                      let _ = downloads::update_download_state(&pool5, id, "completed", Some(received)).await;
                      let _ = app5.emit(crate::contracts::events::DOWNLOADS_UPDATED, ());
                    }
                  });
                });

                // 失败回调
                let pool6 = dl_pool.clone();
                let app6 = dl_app.clone();
                let id_state6 = id_state.clone();
                download.connect_failed(move |_dl, err| {
                  log::error!("[downloads] 下载失败: {}", err);
                  let pool7 = pool6.clone();
                  let app7 = app6.clone();
                  let id_state7 = id_state6.clone();
                  tauri::async_runtime::spawn(async move {
                    let id_now = { *id_state7.lock().unwrap_or_else(|e| e.into_inner()) };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state7).await,
                    };
                    if let Some(id) = id {
                      let _ = downloads::update_download_state(&pool7, id, "failed", None).await;
                      let _ = app7.emit(crate::contracts::events::DOWNLOADS_UPDATED, ());
                    }
                  });
                });
              });
              log::info!("[downloads] download 信号已注册");
            }
          }
          child_gtk.show();
          log::info!(
            "[tabs] child_webview reparent 到 Fixed: pos=(0,{}) size=({:.0},{:.0})",
            CONTENT_TOP_OFFSET, w, h
          );
          // 布局完成后打印实际分配尺寸（idle 延迟，等 GTK 布局循环）
          let child_gtk2 = child_gtk.clone();
          gtk::glib::idle_add_local_once(move || {
            let alloc = child_gtk2.allocation();
            log::info!(
              "[tabs] child allocation: {}x{} at ({},{})",
              alloc.width(),
              alloc.height(),
              alloc.x(),
              alloc.y()
            );
          });
        } else {
          log::info!("[tabs] child_webview 的 parent 不是 GtkBox，跳过 reparent");
        }
        let _ = tx.send(());
      })
      .map_err(|e| format!("child webview 句柄获取失败: {}", e))?;

        rx.recv().map_err(|e| format!("主线程布局失败: {}", e))?;
        Ok(())
    }

}


/// 应用启动时初始化 GTK 布局（setup 阶段调用，主线程）
/// 布局结构：窗口 vbox → [GtkOverlay] → 主 webview（主层，自动填满）+ GtkFixed（上层，child webviews 绝对定位）
/// 主 webview 在 overlay 主层自动跟随窗口（不手动改 size_request，避免撑大窗口 natural size 导致无法缩小）
#[cfg(target_os = "linux")]
pub(crate) fn setup_gtk_layout(app: &tauri::App) -> Result<(), String> {
    // 读取书签栏显隐，决定初始 top_offset
    let top = if let Some(state) = app.try_state::<TabManager>() {
        state.top_offset()
    } else {
        CONTENT_TOP_OFFSET
    };

    if let Some(main_wv) = app.get_webview("main") {
        main_wv
            .with_webview(move |pwv| {
                let main_gtk = pwv.inner();
                // 从 parent 拿 vbox（不调 window 方法，避免主线程死锁）
                if let Some(vbox) = crate::engine::layout::parent_vbox(&main_gtk) {
                    // 创建 GtkOverlay 挂到 vbox（填满窗口）
                    let overlay = gtk::Overlay::new();
                    vbox.remove(&main_gtk);
                    vbox.pack_start(&overlay, true, true, 0);
                    overlay.show();

                    // 主 webview 放主层：自动填满 overlay。size_request 只设最小尺寸（跟窗口 minWidth/minHeight 一致），
                    // 不能设 1280x800——那会撑大窗口 natural size，导致窗口无法缩小
                    overlay.add(&main_gtk);
                    main_gtk.set_size_request(800, 600);
                    main_gtk.show();

                    // 焦点跟踪：主 webview（UI 层）获得焦点
                    main_gtk.connect_focus_in_event(|_, _| {
                        UI_FOCUSED.store(true, std::sync::atomic::Ordering::Relaxed);
                        gtk::glib::Propagation::Proceed
                    });

                    // GtkFixed 放上层：child webviews 绝对定位
                    // 关键：Fixed 用 margin_top 定位到内容区（y=top 起），避免覆盖地址栏拦截点击事件
                    // 同样只设最小尺寸，避免撑大窗口；resize 时 on_window_event 会更新为实际尺寸
                    // halign=Start：Fixed 只占自身请求宽度（默认 fill 会横跨全宽，
                    // 在 AI 分屏等"内容区变窄"场景下会拦截右侧 HTML 层（AI 面板）的点击）
                    let fixed = gtk::Fixed::new();
                    fixed.set_halign(gtk::Align::Start);
                    fixed.set_valign(gtk::Align::Start);
                    fixed.set_margin_top(top as i32);
                    fixed.set_size_request(800, 600);
                    overlay.add_overlay(&fixed);
                    fixed.show();

                    log::info!("[tabs] setup: GtkOverlay 布局初始化完成 top={}", top);

                    // 无边框窗口边缘拉伸：decorations=false 时 GTK/GNOME 不提供边缘 resize
                    // 热区，这里在窗口最上层铺一圈边缘原生部件补上（见 engine/window_resize.rs）
                    if let Some(tl) = main_gtk.toplevel() {
                        if let Ok(gtk_win) = tl.downcast::<gtk::Window>() {
                            crate::engine::window_resize::install(&overlay, &gtk_win);
                        } else {
                            log::warn!("[window-resize] toplevel 不是 GtkWindow，跳过边缘拉伸");
                        }
                    }
                } else {
                    log::info!("[tabs] setup: 主 webview 的 parent 不是 GtkBox，跳过 reparent");
                }
            })
            .map_err(|e| format!("主 webview 句柄获取失败: {}", e))?;
    }

    // 注册窗口 resize 监听：同步 child webviews 和 Fixed 容器（主 webview 在 overlay 主层自动跟随窗口）
    let app_handle = app.handle().clone();
    if let Some(window) = app.get_window("main") {
        // 闭包需按当前窗口尺寸重算几何，这里克隆一份句柄（Window 是引用计数句柄）
        let win_for_geo = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Resized(size) = event {
                let w = size.width as i32;
                let h = size.height as i32;
                let top = if let Some(state) = app_handle.try_state::<TabManager>() {
                    state.top_offset()
                } else {
                    CONTENT_TOP_OFFSET
                };
                let ai_active = if let Some(state) = app_handle.try_state::<TabManager>() {
                    *state
                        .ai_panel_active
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                } else {
                    false
                };
                // 几何计算统一走引擎层（避免公式散落多处漂移）
                let vertical = app_handle
                    .try_state::<TabManager>()
                    .map(|s| s.vertical_tabs())
                    .unwrap_or(false);
                let geo = crate::engine::layout::ContentGeometry::compute(
                    &win_for_geo,
                    top,
                    ai_active,
                    vertical,
                );
                log::info!(
                    "[tabs] window resized: {}x{} content={}x{} top={} ai={}",
                    w,
                    h,
                    geo.width,
                    geo.height,
                    geo.top,
                    ai_active
                );
                if let Some(state) = app_handle.try_state::<TabManager>() {
                    if state.split_tab().is_some() {
                        // 分屏中：两个窗格各自几何，统一走 apply_split_layout
                        let _ = apply_split_layout(&app_handle, &state);
                    } else {
                        let ids: Vec<String> = state
                            .tabs
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .keys()
                            .cloned()
                            .collect();
                        for id in ids {
                            crate::engine::layout::apply_to_webview(&win_for_geo, &id, geo);
                        }
                    }
                }
            }
        });
    }

    // ===== 全局快捷键（GTK 顶层窗口拦截）=====
    // 背景：HTML 层（主 webview）的 JS 监听器只在主 webview 持有键盘焦点时触发；
    // 用户一旦点击网页，焦点落到 child webview，所有前端快捷键都会失效（时灵时不灵）。
    // 子控件未消费的按键会沿焦点链冒泡到顶层窗口，因此在 GTK 窗口层兜底最可靠。
    // 本批先兜底「开发者工具（F12 / Ctrl+Shift+I）」；其余浏览器级快捷键的路由见战略文档批次 0.3。
    if let Some(window) = app.get_window("main") {
        if let Ok(gtk_win) = window.gtk_window() {
            let app_handle = app.handle().clone();
            gtk_win.connect_key_press_event(move |_, event| {
                // 快捷键表来自 contracts.rs（单一来源）：keyval / 修饰键 / 兜底标记
                // 都在那里定义，避免此前裸 keycode（0xFFC9/0x69/0x74…）散落各处
                use crate::contracts::shortcuts::SHORTCUTS;
                let key: u32 = *event.keyval();
                let state = event.state();
                let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);

                let Some(sc) = SHORTCUTS.iter().find(|s| {
                    (key == s.gdk_keyval || key == s.gdk_keyval_shift)
                        && ctrl == s.ctrl
                        && shift == s.shift
                }) else {
                    return gtk::glib::Propagation::Proceed;
                };

                // ① 无条件响应（F12 / Ctrl+Shift+I：不论焦点在哪都开开发者工具）
                if sc.gtk_always {
                    open_devtools_for_active(&app_handle);
                    return gtk::glib::Propagation::Stop;
                }

                // ② 其余浏览器级快捷键：仅在「页面持焦」时兜底。
                // UI 层持焦时前端 keydown 已处理，这里必须放行，否则会重复触发（如建两个标签）。
                if !UI_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
                    log::debug!("[tabs] GTK 层转发快捷键: {}", sc.id);
                    let _ = app_handle.emit(crate::contracts::events::GLOBAL_SHORTCUT, sc.id);
                    return gtk::glib::Propagation::Stop;
                }
                gtk::glib::Propagation::Proceed
            });
            log::info!("[tabs] 全局快捷键已挂载：F12 / Ctrl+Shift+I 打开开发者工具");
        }
    }

    Ok(())
}

/// 应用某个标签 webview 的内容区几何（按当前窗口尺寸重算）。
/// 用于"显示之后"重新分配：Fixed/webview 隐藏期间尺寸变更不会触发 GTK 重新分配，
/// 且 WebKit 内部输入窗口不跟随尺寸变化，表现为"页面能看到但点不动"。
/// 实现委托引擎层（engine/layout.rs）。
#[cfg(target_os = "linux")]
pub(crate) fn apply_tab_geometry(window: &tauri::Window, id: &str, top: f64, ai_active: bool) {
    let vertical = window
        .app_handle()
        .try_state::<TabManager>()
        .map(|s| s.vertical_tabs())
        .unwrap_or(false);
    let geo = crate::engine::layout::ContentGeometry::compute(window, top, ai_active, vertical);
    crate::engine::layout::apply_to_webview(window, id, geo);
}

/// 应用分屏布局：左窗格=活动标签，右窗格=分屏标签（如有）；隐藏其余标签。
/// 未分屏时等价于"只显示活动标签"（但用统一的隐藏-再显示路径，避免 z-order 残留）。
pub(crate) fn apply_split_layout(app: &AppHandle, state: &TabManager) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let top = state.top_offset();
    let vertical = state.vertical_tabs();
    let ai_active = *state
        .ai_panel_active
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let active = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let split = state.split_tab();

    let full = crate::engine::layout::ContentGeometry::compute(&window, top, ai_active, vertical);
    let ids: Vec<String> = state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();

    // 先全部隐藏（避免 z-order/尺寸错位），再按窗格定位并显示
    for id in &ids {
        let _ = TabManager::set_webview_visible(app, id, false);
    }
    // 覆盖层（证书警告等）激活时：只隐藏、不显示，避免盖住覆盖层
    if UI_OVERLAY_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    match (active, split) {
        (Some(a), Some(s)) if s != a => {
            let (lg, rg) = full.split();
            crate::engine::layout::apply_to_webview(&window, &a, lg);
            crate::engine::layout::apply_to_webview(&window, &s, rg);
            // 先显示右窗格，再显示左窗格（活动标签获得焦点、置于上层）
            let _ = TabManager::set_webview_visible(app, &s, true);
            let _ = TabManager::set_webview_visible(app, &a, true);
        }
        (Some(a), _) => {
            crate::engine::layout::apply_to_webview(&window, &a, full);
            let _ = TabManager::set_webview_visible(app, &a, true);
        }
        _ => {}
    }
    Ok(())
}

// ---- 以下为命令区摘出的辅助项（P1-1）----
/// 打开活动标签的开发者工具（命令与 GTK 全局快捷键共用）。
/// 走异步任务派发：从 GTK 信号处理器里直接调 `with_webview` 有主线程重入风险，
/// 异步派发由 Tauri 正确调度到主线程执行。
pub(crate) fn open_devtools_for_active(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<TabManager>() else {
            return;
        };
        let id = state
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(id) = id else {
            return;
        };
        let Some(window) = app.get_window("main") else {
            return;
        };
        let label = format!("tab-{}", id);
        if let Some(wv) = window.get_webview(&label) {
            let _ = wv.with_webview(move |pwv| {
                crate::engine::webview::open_inspector(&pwv.inner());
            });
            log::info!("[tabs] 已打开开发者工具: tab={}", id);
        }
    });
}

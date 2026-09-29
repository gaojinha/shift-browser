//! child webview 的 Linux/GTK reparent、事件挂接与下载信号（R1 拆分 · 2026-09-25）
//!
//! 从 tabs/backend/layout.rs 原样外移；impl 块分散在子模块不影响方法归属。

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter};

use crate::db::downloads;
use crate::engine::layout::CONTENT_TOP_OFFSET;
use crate::tabs::model::TabManager;
use crate::tabs::util::sanitize_filename;

use crate::engine::webview::WebViewBackend;
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use webkit2gtk::{DownloadExt, URIRequestExt, URIResponseExt, WebContextExt, WebViewExt};

use super::super::{unique_download_path, UI_FOCUSED};

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
        // O1-b：逐跳 SSRF 守卫需按标签来源（origin=ai）判定，先取本 webview 的标签 id
        let guard_tab_id = child.label().trim_start_matches("tab-").to_string();

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
          // 滚轮诊断开关（2026-09-29）：SHIFT_SCROLL_DEBUG=1 时，Fixed 容器与 child
          // webview 的滚轮事件都打 info 日志。对比两者定位事件卡在哪层：
          // - fixed 有、child 无 = 事件被中间 widget 吃掉（覆盖层/透明层盖住）
          // - 两者都有但页面不滚 = WebKit/DOM 层问题（页面劫持 wheel/内容不可滚）
          let scroll_dbg = std::env::var("SHIFT_SCROLL_DEBUG").is_ok();
          if scroll_dbg {
            fixed.connect_scroll_event(|_, event| {
              let (dx, dy) = event.delta();
              log::info!(
                "[tabs] fixed scroll(top): delta=({:.2},{:.2}) dir={:?}",
                dx, dy, event.direction()
              );
              false.into() // 不拦截，只观测
            });
          }
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
          // O1-b：AI 标签（origin=ai）逐跳导航 SSRF 守卫（内网地址未进白名单则拒绝）
          child_gtk.attach_navigation_guard(app.clone(), guard_tab_id.clone());
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
// 兜底补滚节流器（2026-09-29）：B站等站点 JS 吞 wheel 事件（preventDefault），
// 事件到 webview 但页面不滚。此处 100ms 检查一次：位置未变则 scrollBy 补滚。
          // 正常页面 WebKit 100ms 内必滚动，不误伤。
          let scroll_check = std::rc::Rc::new(std::cell::Cell::new(
            std::time::Instant::now() - std::time::Duration::from_secs(1),
          ));
          child_gtk.connect_scroll_event(move |wv, event| {
              let (dx, dy) = event.delta();
              // 滚轮事件最密集 → 平时 debug 级（每次滚动都 info 会刷爆日志）；
              // 诊断开关开时打 info，直接进应用日志文件
              if scroll_dbg {
                log::info!(
                  "[tabs] child scroll: delta=({:.2},{:.2}) dir={:?}",
                  dx, dy, event.direction()
                );
                // DOM 滚动状态诊断：内容高度/overflow/滚动元素/当前滚动位置，
                // 判断页面不可滚还是事件被 JS 吞掉
                let js = "(function(){var de=document.documentElement;var b=document.body;var se=document.scrollingElement;return 'y='+Math.round(window.scrollY)+' ih='+window.innerHeight+' de_sh='+(de?de.scrollHeight:0)+' b_sh='+(b?b.scrollHeight:0)+' de_ov='+(de?getComputedStyle(de).overflowY:'?')+' b_ov='+(b?getComputedStyle(b).overflowY:'?')+' se='+(se?se.tagName:'?');})()";
                crate::engine::webview::eval_js_with(wv, js, move |res| {
                  if let Ok(info) = res {
                    log::info!("[tabs] 滚动DOM: {}", info);
                  }
                });
              } else {
                log::debug!(
                  "[tabs] child scroll: delta=({:.2},{:.2}) dir={:?}",
                  dx, dy, event.direction()
                );
              }
              wv.grab_focus();
              // 兜底补滚：事件到 webview 但 100ms 后滚动位置未变（JS 吞事件），
              // 主动 scrollBy 补滚。GTK smooth delta 单位约行，1 行按 100px 换算。
              // 横纵一起对比（scrollY*1000+scrollX 合成）。100ms 节流，性能可接受。
              let now = std::time::Instant::now();
              if now.duration_since(scroll_check.get()) >= std::time::Duration::from_millis(100)
                && (dy != 0.0 || dx != 0.0)
              {
                scroll_check.set(now);
                let wv2 = wv.clone();
                let dx_px = (dx * 100.0) as i32;
                let dy_px = (dy * 100.0) as i32;
                crate::engine::webview::eval_js_with(
                  wv,
                  "(function(){return Math.round(window.scrollY*1000+window.scrollX);})()",
                  move |r0| {
                    let Ok(y0) = r0 else { return };
                    let wv3 = wv2.clone();
                    gtk::glib::timeout_add_local_once(
                      std::time::Duration::from_millis(100),
                      move || {
                        let wv4 = wv3.clone();
                        crate::engine::webview::eval_js_with(
                          &wv3,
                          "(function(){return Math.round(window.scrollY*1000+window.scrollX);})()",
                          move |r1| {
                            let Ok(y1) = r1 else { return };
                            if y0 == y1 {
                              let js = format!("window.scrollBy({dx_px},{dy_px})");
                              crate::engine::webview::eval_js(&wv4, &js);
                              log::debug!("[tabs] 兜底补滚: dx={dx_px} dy={dy_px}");
                            }
                          },
                        );
                      },
                    );
                  },
                );
              }
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

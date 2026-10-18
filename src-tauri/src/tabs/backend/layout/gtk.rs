// LINES-EXEMPT: R2 渲染层去风险目标（2026-10-15 高哥拍板前不动），豁免至 R2 spike 落地
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

// ---- 下载/滚动判定纯函数（便于单测；F-36 从 fix_gtk_layout 闭包抽出，行为等价）----

/// 从下载 URL 推导下载文件名：
/// 用 url 解析取解码后的 path（%2e%2e/%2f 点段按 URL 规范被规范化掉），
/// 取最后一段，再经 sanitize_filename 剥路径分隔符/点段/控制字符，防路径穿越；
/// 解析不了的 URL 退回按 '?' 取首段；空段兜底 "download"。
fn download_filename_from_url(url: Option<&str>) -> String {
    url.map(|u| {
        let path = url::Url::parse(u)
            .map(|p| p.path().to_string())
            .unwrap_or_else(|_| u.split('?').next().unwrap_or(u).to_string());
        let name = path.rsplit('/').next().unwrap_or("download").to_string();
        sanitize_filename(&name)
    })
    .unwrap_or_else(|| "download".to_string())
}

/// 构造下载目的地的 file URI：路径含中文/空格时手拼 "file://…" 是非法 URI，
/// WebKit 会取消下载（表现为"下载不能开始"），必须走 from_file_path（percent-编码）；
/// 失败（相对路径/非 UTF-8）退回手拼，保证链路不断。
fn file_uri_for_path(dest: &str) -> String {
    url::Url::from_file_path(dest)
        .map(|u| u.to_string())
        .unwrap_or_else(|_| format!("file://{}", dest))
}

/// 滚轮诊断开关：精确值 "1" 才开（此前 is_ok() 导致空值也开，
/// 诊断 info 刷屏压垮主线程致 ANR，copilot 会诊确认 2026-09-29 修复）
fn scroll_debug_enabled(raw: Option<&str>) -> bool {
    raw == Some("1")
}

/// 兜底补滚检查闸门：在途检查未完成时跳过（防 eval 积压）、
/// 100ms 节流（正常页面 WebKit 100ms 内必滚动，不误伤）、零位移跳过
fn scroll_check_due(in_flight: bool, since_last: std::time::Duration, dx: f64, dy: f64) -> bool {
    !in_flight
        && since_last >= std::time::Duration::from_millis(100)
        && (dy != 0.0 || dx != 0.0)
}

/// GTK smooth delta（单位约行）→ 像素：1 行按 100px 换算
fn delta_to_px(delta: f64) -> i32 {
    (delta * 100.0) as i32
}

/// 等待下载记录 id 就绪（最多 100×50ms=5s）
async fn wait_download_id(state: &Arc<Mutex<Option<i64>>>) -> Option<i64> {
    for _ in 0..100 {
        if let Some(id) = *state.lock().unwrap_or_else(|e| e.into_inner()) {
            return Some(id);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    None
}

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
          // 滚轮诊断开关（2026-09-29）：SHIFT_SCROLL_DEBUG=1 精确值才开
          // （此前 is_ok() 导致空值也开；诊断 info 刷屏压垮主线程致 ANR，
          // copilot 会诊确认，2026-09-29 修复：精确值+限频+在途限制）
          let scroll_dbg = scroll_debug_enabled(std::env::var("SHIFT_SCROLL_DEBUG").ok().as_deref());
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
          // 在途限制（2026-09-29 ANR 修复）：同时只允许一轮检查在途，防止 eval 积压
          // 压垮主线程；诊断 DOM 查询同样受此限制，不逐事件刷屏。
          let scroll_check = std::rc::Rc::new(std::cell::Cell::new(
            std::time::Instant::now() - std::time::Duration::from_secs(1),
          ));
          let scroll_in_flight = std::rc::Rc::new(std::cell::Cell::new(false));
          child_gtk.connect_scroll_event(move |wv, event| {
              let (dx, dy) = event.delta();
              // 滚轮事件最密集 → 平时 debug 级（每次滚动都 info 会刷爆日志）；
              // 诊断开关开时打 info，直接进应用日志文件
              if scroll_dbg {
                log::info!(
                  "[tabs] child scroll: delta=({:.2},{:.2}) dir={:?}",
                  dx, dy, event.direction()
                );
                // 注：DOM 结构诊断（scrollY/overflow/scrollingElement）已完成使命
                // （确诊 B站 JS 吞事件），为防逐事件 eval 压垮主线程已删除；
                // 需再诊断时用补滚链的 scrollY 对比日志即可。
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
              // 在途检查未完成时跳过（防 eval 积压）；100ms 节流；零位移跳过
              if scroll_check_due(
                scroll_in_flight.get(),
                now.duration_since(scroll_check.get()),
                dx,
                dy,
              ) {
                scroll_check.set(now);
                scroll_in_flight.set(true);
                let flight = scroll_in_flight.clone();
                let wv2 = wv.clone();
                let dx_px = delta_to_px(dx);
                let dy_px = delta_to_px(dy);
                crate::engine::webview::eval_js_with(
                  wv,
                  "(function(){return Math.round(window.scrollY*1000+window.scrollX);})()",
                  move |r0| {
                    let Ok(y0) = r0 else {
                      flight.set(false);
                      return;
                    };
                    let wv3 = wv2.clone();
                    let flight2 = flight.clone();
                    gtk::glib::timeout_add_local_once(
                      std::time::Duration::from_millis(100),
                      move || {
                        let wv4 = wv3.clone();
                        let flight3 = flight2.clone();
                        crate::engine::webview::eval_js_with(
                          &wv3,
                          "(function(){return Math.round(window.scrollY*1000+window.scrollX);})()",
                          move |r1| {
                            // 所有路径清除在途标记（成功/失败/补滚），防堆积
                            flight3.set(false);
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
                let filename = download_filename_from_url(url.as_deref());
                // 下载目录优先级：用户设置覆盖 > 系统 XDG 目录 > ~/Downloads 兜底。
                // P0-2：平台目录解析下沉到 engine::platform（统一 resolve_download_dir，行为保持、可单测）
                let dest_dir = crate::engine::platform::resolve_download_dir();
                let _ = std::fs::create_dir_all(&dest_dir);
                // 同名文件已存在时加 (1)(2) 后缀，避免覆盖
                let dest = unique_download_path(&dest_dir, &filename);
                // 目的地必须是合法 file URI——路径含中文/空格时手拼 "file://…" 非法，
                // WebKit 会取消下载（表现为"下载不能开始"）。
                let dest_uri = file_uri_for_path(&dest);
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

                // 等待下载记录 id 就绪（最多 5s）：wait_download_id 已提模块级（F-36）

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
                      crate::log_if_err!(
                        "downloads",
                        downloads::update_download_bytes(&pool3, id, received, Some(total_now)).await
                      );
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
                      crate::log_if_err!(
                        "downloads",
                        downloads::update_download_state(&pool5, id, "completed", Some(received)).await
                      );
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
                      crate::log_if_err!(
                        "downloads",
                        downloads::update_download_state(&pool7, id, "failed", None).await
                      );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn download_filename_from_url_normal_and_query() {
        // 取解码后 path 的最后一段；query 不参与文件名推导（不能靠 query 注入路径）
        assert_eq!(
            download_filename_from_url(Some("https://example.com/files/report.pdf?v=1&x=../evil")),
            "report.pdf"
        );
        assert_eq!(
            download_filename_from_url(Some("https://example.com/a.zip?file=../../etc/shadow")),
            "a.zip"
        );
        // %2e%2e 点段按 URL 规范被规范化，末段仍经 sanitize 复剥
        assert_eq!(
            download_filename_from_url(Some("https://example.com/%2e%2e/%2e%2e/etc/passwd")),
            "passwd"
        );
    }

    #[test]
    fn download_filename_from_url_fallbacks() {
        // 域名根/尾斜杠 → 空段 → sanitize 兜底 download
        assert_eq!(download_filename_from_url(Some("https://example.com")), "download");
        assert_eq!(download_filename_from_url(Some("https://example.com/")), "download");
        // URL 缺失 → download
        assert_eq!(download_filename_from_url(None), "download");
        // 解析不了 → 按 '?' 取首段
        assert_eq!(download_filename_from_url(Some("not a url?x=1")), "not a url");
    }

    #[test]
    fn file_uri_for_path_encodes_and_falls_back() {
        // 中文/空格 → percent-编码的合法 file URI（无裸空格/非 ASCII）
        let uri = file_uri_for_path("/home/gao/Downloads/报表 v1.xlsx");
        assert!(uri.starts_with("file:///home/gao/Downloads/"));
        assert!(uri.ends_with("v1.xlsx"));
        assert!(!uri.contains(' '));
        assert!(!uri.contains('报'));
        // from_file_path 对相对路径失败 → 手拼兜底（保证链路不断）
        assert_eq!(file_uri_for_path("relative/x.zip"), "file://relative/x.zip");
    }

    #[test]
    fn scroll_debug_enabled_exact_one_only() {
        // 精确值 "1" 才开（此前 is_ok() 空值也开，诊断刷屏致 ANR）
        assert!(scroll_debug_enabled(Some("1")));
        assert!(!scroll_debug_enabled(Some("")));
        assert!(!scroll_debug_enabled(Some("0")));
        assert!(!scroll_debug_enabled(Some("2")));
        assert!(!scroll_debug_enabled(None));
    }

    #[test]
    fn scroll_check_gate() {
        // 在途 → 跳过
        assert!(!scroll_check_due(true, Duration::from_millis(999), 0.0, -1.0));
        // 未到 100ms 节流窗 → 跳过
        assert!(!scroll_check_due(false, Duration::from_millis(99), 0.0, -1.0));
        // 恰好 100ms + 有位移 → 触发
        assert!(scroll_check_due(false, Duration::from_millis(100), 0.0, -1.0));
        // 零位移 → 跳过
        assert!(!scroll_check_due(false, Duration::from_millis(999), 0.0, 0.0));
        // 横向位移同样算
        assert!(scroll_check_due(false, Duration::from_millis(100), 2.0, 0.0));
    }

    #[test]
    fn delta_to_px_converts_rows_to_pixels() {
        // 1 行 ≈ 100px（GTK smooth delta 单位约行）
        assert_eq!(delta_to_px(-1.0), -100);
        assert_eq!(delta_to_px(0.0), 0);
        assert_eq!(delta_to_px(0.5), 50);
        assert_eq!(delta_to_px(1.25), 125);
    }

    #[tokio::test]
    async fn wait_download_id_returns_when_ready() {
        // 模拟异步插入 60ms 完成 → 50ms 轮询取到
        let state = Arc::new(Mutex::new(None));
        let s2 = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(60)).await;
            *s2.lock().unwrap() = Some(42);
        });
        assert_eq!(wait_download_id(&state).await, Some(42));
    }

    #[tokio::test]
    async fn wait_download_id_returns_immediately_when_presumed_set() {
        // id 已就绪 → 首轮即返，不空等
        let state = Arc::new(Mutex::new(Some(7)));
        assert_eq!(wait_download_id(&state).await, Some(7));
    }
    // 超时路径（100×50ms=5s 返 None）不专测：真实时钟 5s/次拖慢全量关口，
    // 100/50 两数已随函数单一实现，漂移走代码评审
}

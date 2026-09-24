//! TabManager::create_webview——webview 创建与信号接线（R2-1 拆分）

use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
};

use crate::db::{history, Db};

use super::super::model::{now_ts, TabManager};
use crate::engine::layout::{CONTENT_TOP_OFFSET, VERTICAL_TAB_BAR_WIDTH};

#[cfg(target_os = "linux")]
use webkit2gtk::WebViewExt;
use crate::engine::webview::WebViewBackend;

use super::WEB_CRASH_COOLDOWN;
use super::is_internal_newtab_url;

impl TabManager {
    /// 创建嵌入式 webview
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    pub(crate) fn create_webview(app: &AppHandle, id: &str, url: &str, private: bool) -> Result<(), String> {
        let window = app
            .get_window("main")
            .ok_or_else(|| "主窗口不存在".to_string())?;

        let parsed_url = url
            .parse::<tauri::Url>()
            .map_err(|e| format!("URL 解析失败: {}", e))?;

        // file:// URL 用 shift:// 自定义协议替代，让 WebKitGTK 的 UserScript（IPC桥）能注入
        let webview_url = if parsed_url.scheme() == "file" {
            let path = parsed_url.path();
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法获取 app_data_dir: {}", e))?;
            let newtab_dir = app_data.join("newtab");
            let newtab_index = newtab_dir.join("index.html");
            let worldnav = newtab_dir.join("world-nav.html");
            if path == newtab_index.to_string_lossy() || path.ends_with("newtab/index.html") {
                log::info!("[tabs] file URL → shift://newtab");
                tauri::WebviewUrl::CustomProtocol(
                    "shift://newtab"
                        .parse()
                        .map_err(|e| format!("URL 解析失败: {}", e))?,
                )
            } else if path == worldnav.to_string_lossy() || path.ends_with("newtab/world-nav.html")
            {
                log::info!("[tabs] file URL → shift://worldnav");
                tauri::WebviewUrl::CustomProtocol(
                    "shift://worldnav"
                        .parse()
                        .map_err(|e| format!("URL 解析失败: {}", e))?,
                )
            } else {
                log::info!("[tabs] file URL 保持 External: {}", path);
                WebviewUrl::External(parsed_url)
            }
        } else {
            WebviewUrl::External(parsed_url)
        };

        let top = {
            if let Some(state) = app.try_state::<TabManager>() {
                state.top_offset()
            } else {
                CONTENT_TOP_OFFSET
            }
        };
        let ai_active = if let Some(state) = app.try_state::<TabManager>() {
            *state
                .ai_panel_active
                .lock()
                .unwrap_or_else(|e| e.into_inner())
        } else {
            false
        };
        let vertical = app
            .try_state::<TabManager>()
            .map(|s| s.vertical_tabs())
            .unwrap_or(false);
        let (w, h) = Self::content_size(&window, top, ai_active, vertical);
        log::info!(
            "[tabs] create_webview: inner={:?} content=({:.0},{:.0}) top={}",
            window.inner_size(),
            w,
            h,
            top
        );

        // 页面加载完成时同步标题（用 host 作标题，MVP 阶段）
        let app_handle = app.clone();
        let app_handle2 = app_handle.clone();
        let tab_id = id.to_string();
        let label = format!("tab-{}", id);
        // 新窗口去重：同一 URL 500ms 内重复触发（原生 create 信号 + JS window.open 双通道）只建一个标签
        let last_new_window: std::sync::Arc<
            std::sync::Mutex<Option<(String, std::time::Instant)>>,
        > = std::sync::Arc::new(std::sync::Mutex::new(None));
        let child = window
      .add_child(
        WebviewBuilder::new(label.clone(), webview_url)
          // 无痕标签：走 ephemeral WebContext（Cookie/缓存/存储不落盘、与常规标签隔离）
          .incognito(private)
          // Linux：显式指定 web extensions 目录（exe 同目录），否则 Tauri 的 IPC 桥
          // 依赖的 WebKit web extension 不会加载，__TAURI_INTERNALS__ 注入失败
          .extensions_path(
            std::env::current_exe()
              .ok()
              .and_then(|e| e.parent().map(|p| p.to_path_buf()))
              .unwrap_or_default(),
          )
          // target=_blank / window.open 新窗口请求：统一创建新标签页打开链接
          .on_new_window(move |url, _features| {
            log::info!("[tabs] on_new_window: url={}", crate::redact_url(url.as_str()));
            // P2：远程页面可控入口——只放行 http/https。
            // 拒绝 file:/javascript:/data: 等：恶意页面可 window.open('file:///etc/passwd')
            // 强制打开本地文件标签（create_webview 对非 newtab 的 file:// 保持 External 渲染）。
            // 内部页面（shift://newtab 等）由可信前端 create_tab 命令打开，不走此入口。
            if url.scheme() != "http" && url.scheme() != "https" {
              log::warn!("[tabs] on_new_window: 拒绝非 http/https scheme: {}", url);
              return tauri::webview::NewWindowResponse::Deny;
            }
            let url_str = url.to_string();
            // 去重：500ms 内同 URL 已处理过则忽略，防止原生+JS 双通道重复建标签
            {
              let mut last = last_new_window.lock().unwrap_or_else(|e| e.into_inner());
              let now = std::time::Instant::now();
              if let Some((prev_url, prev_at)) = last.as_ref() {
                if *prev_url == url_str && now.duration_since(*prev_at).as_millis() < 500 {
                  log::info!("[tabs] on_new_window: 重复请求已忽略 url={}", crate::redact_url(&url_str));
                  return tauri::webview::NewWindowResponse::Deny;
                }
              }
              *last = Some((url_str.clone(), now));
            }
            let app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
              let state = app.state::<TabManager>();
              if let Ok(tab) = crate::tabs::commands::create_tab(app.clone(), state, url_str, None, None).await {
                log::info!("[tabs] on_new_window: 新标签创建成功 id={}", tab.id);
                // 页面内 window.open 建的新标签，前端不知情 → 通知同步
                let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
              }
            });
            tauri::webview::NewWindowResponse::Deny
          })
.on_page_load(move |webview, payload| {
            // 开启页面 console 输出（诊断 JS 错误用）——任何加载事件都设置，
            // 不依赖 Finished（部分页面 Finished 不触发）
            let _ = webview.with_webview(move |pwv| {
                crate::engine::webview::enable_console_output(&pwv.inner());
            });
            // 任何加载事件（Started/Finished，含后退/前进缓存恢复）都安排延迟注入：
            // - UserScript 只对"之后导航"生效，已加载页面必须 evaluate 补注入；
            // - 后退/前进时 WebKit 从 back-forward 缓存恢复页面，UserScript 不会重新执行，
            //   返回的页面会丢脚本 → 必须重新 evaluate（800ms/3s/8s 重试，幂等无害）。
            let app_ext = app_handle2.clone();
            let label_ext = webview.label().to_string();
            tauri::async_runtime::spawn(async move {
                for delay in [800u64, 3000, 8000] {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    crate::extensions::inject_to_webview(&app_ext, &label_ext).await;
                }
            });
            if payload.event() == tauri::webview::PageLoadEvent::Finished {
              let url = payload.url();
              let host = url.host_str().unwrap_or("").to_string();
              // 内部页面（shift://newtab / shift://worldnav）标题固定"启动页"，
              // 不显示 host（否则标签页显示 "newtab"）
              let title = if is_internal_newtab_url(url.as_str()) {
                "启动页".to_string()
              } else if host.is_empty() {
                url.to_string()
              } else {
                host
              };

              log::info!("[tabs] page_load_finished: tab={} host={}", tab_id, crate::redact_url(&url.to_string()));

              // 证书监测（P3 · 2026-09-22）：HTTPS 页面加载完成后采集并记录证书快照，
              // 指纹变更/校验失败自动落事件（延迟 700ms 等 TLS 信息就绪）
              if url.scheme() == "https" {
                let app_cert = app_handle2.clone();
                let tab_id_cert = tab_id.clone();
                tauri::async_runtime::spawn(async move {
                  tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                  crate::tls::monitor(&app_cert, &tab_id_cert).await;
                });
              }

              // 提取 favicon 并更新 tab（更新后 emit 通知前端同步）
              let app_fav = app_handle2.clone();
              let tab_id_fav = tab_id.clone();
              let _ = webview.with_webview(move |pwv| {
                let gtk_wv = pwv.inner();
                crate::engine::webview::eval_js_with(
                  &gtk_wv,
                  "(function(){var l=document.querySelector('link[rel=\"icon\"],link[rel=\"shortcut icon\"]');var u=l?l.href:'';if(!u&&location.protocol!=='file:')u=location.origin+'/favicon.ico';return u;})()",
                  move |res| {
                    if let Ok(fav) = res {
                      if !fav.is_empty() && !fav.starts_with("about:") {
                        log::debug!("[tabs] favicon: tab={} {}", tab_id_fav, fav);
                        let updated = {
                          let state = app_fav.state::<TabManager>();
                          let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
                          match tabs.get_mut(&tab_id_fav) {
                            Some(t) if t.favicon != fav => {
                              t.favicon = fav;
                              true
                            }
                            _ => false,
                          }
                        };
                        // favicon 更新 → 通知前端同步
                        if updated {
                          let _ = app_fav.emit(crate::contracts::events::TABS_UPDATED, ());
                        }
                      }
                    }
                  },
                );
              });

              // 按设置记录浏览历史（record_history 开关，默认关 · 隐私优先）——先 clone，避免 title 被 move
              let app3 = app_handle2.clone();
              let url_str = url.to_string();
              let title_opt = Some(title.clone());

              if let Some(state) = app_handle2.try_state::<TabManager>() {
                let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(tab) = tabs.get_mut(&tab_id) {
                  tab.url = url.to_string();
                  // 仅当还没有真标题（title-notify 未触发过）时用 host 占位，避免覆盖 document.title
                  if tab.title.is_empty() || tab.title == tab.url {
                    tab.title = title;
                  }
                }
                // 无痕标签一律不记历史
                if tabs.get(&tab_id).map(|t| t.private).unwrap_or(false) {
                  return;
                }
              }
              // 页面加载完成（URL/标题变化）→ 通知前端同步（替代 2s 轮询）
              let _ = app_handle2.emit(crate::contracts::events::TABS_UPDATED, ());

              tauri::async_runtime::spawn(async move {
                let db = app3.state::<Db>();
                // 性能 P0：走 settings 缓存（原每次页面加载都查库）
                let enabled = crate::db::settings_cache::get(&db.pool, "record_history").await;
                // 隐私优先（2026-09-22）：未显式开启则不记录历史
                let record = enabled.map(|v| v == "true").unwrap_or(false);
                if record {
                  if let Err(e) = history::record_history(db, url_str, title_opt).await {
                    log::warn!("[tabs] record_history 失败: {}", e);
                  }
                }
              });
            }
          }),
        LogicalPosition::new(
          if vertical { VERTICAL_TAB_BAR_WIDTH } else { 0.0 },
          top,
        ),
        LogicalSize::new(w, h),
      )
      .map_err(|e| format!("创建 webview 失败: {}", e))?;

        log::info!(
            "[tabs] child_webview: pos={:?} size={:?}",
            child.position().unwrap(),
            child.size().unwrap()
        );

        // GTK 的 vbox 会忽略 add_child 的位置参数并平分空间（webview 落到窗口下半部分）。
        // 解法：把主 webview 和所有 child webview reparent 到 GtkFixed 容器，实现绝对定位叠层。
        #[cfg(target_os = "linux")]
        {
            let db = app.state::<Db>();
            Self::fix_gtk_layout(&child, w, h, db.pool.clone(), app.clone())?;
            // 新建标签后立即应用内容区几何（Fixed 的 margin 定位）：
            // 否则新标签会停在 setup 时的初始 margin —— 垂直标签下会盖住左侧标签栏。
            if let Some(win) = app.get_window("main") {
                let geo = crate::engine::layout::ContentGeometry::compute(
                    &win, top, ai_active, vertical,
                );
                crate::engine::layout::apply_to_webview(&win, id, geo);
            }
        }

        // 监听 WebKit title 变化 → 实时同步标签标题（on_page_load 的 host 只是初始占位）
        #[cfg(target_os = "linux")]
        {
            let app_for_title = app.clone();
            let id_for_title = id.to_string();
            let _ = child.with_webview(move |pwv| {
                let wv = pwv.inner().clone();
                let app2 = app_for_title;
                let tab_id = id_for_title;
                // 先注册加载失败监听（clone 后再 move 原值给 title_notify）
                let app_fail = app2.clone();
                let tab_id_fail = tab_id.clone();
                wv.connect_load_failed(move |_wv, _load_event, failing_uri, err| {
                    log::warn!(
                        "[tabs] load_failed: tab={} uri={} err={}",
                        tab_id_fail,
                        failing_uri,
                        err.message()
                    );
                    // 阻止 WebKit 默认错误页显示（返回 true = 阻止默认行为）
                    let _ = app_fail.emit(
                        crate::contracts::events::PAGE_LOAD_FAILED,
                        (tab_id_fail.clone(), failing_uri.to_string()),
                    );
                    true
                });
                // TLS 证书错误拦截（P2 · 2026-09-22）：缓存证书 + 通知前端弹"危险站点"页；
                // 返回 true 阻止 WebKit 默认错误页，是否放行交给前端"继续前往"决定。
                let app_tls = app2.clone();
                let tab_id_tls = tab_id.clone();
                wv.connect_load_failed_with_tls_errors(
                    move |_wv, failing_uri, certificate, flags| {
                        crate::tls::on_tls_error(
                            &app_tls,
                            &tab_id_tls,
                            failing_uri,
                            certificate,
                            flags,
                        );
                        true
                    },
                );
                // 权限请求处理（安全 #2 · 2026-09-22）：默认挂起 + 白名单放行 + 通知前端
                crate::perms::attach(app2.clone(), tab_id.clone(), &wv);
                // Web 进程终止恢复（视频崩溃修复 · 2026-09-22）：崩溃/内存超限时
                // 自动重载一次（带 30s 冷却），并通知前端提示，避免标签直接变白。
                let app_crash = app2.clone();
                let tab_id_crash = tab_id.clone();
                wv.connect_web_process_terminated(move |webview, reason| {
                    log::error!(
                        "[tabs] web 进程终止: tab={} reason={:?}",
                        tab_id_crash,
                        reason
                    );
                    let now = now_ts();
                    let map = WEB_CRASH_COOLDOWN.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
                    let allow_reload = {
                        let mut m = map.lock().unwrap_or_else(|e| e.into_inner());
                        let last = m.get(&tab_id_crash).copied().unwrap_or(0);
                        if now - last >= 30 {
                            m.insert(tab_id_crash.clone(), now);
                            true
                        } else {
                            false
                        }
                    };
                    use tauri::Emitter;
                    let _ = app_crash.emit(
                        crate::contracts::events::WEB_PROCESS_CRASHED,
                        serde_json::json!({
                            "tabId": tab_id_crash,
                            "reason": format!("{:?}", reason),
                            "autoReload": allow_reload,
                        }),
                    );
                    if allow_reload {
                        log::info!("[tabs] 自动重载崩溃标签: {}", tab_id_crash);
                        crate::engine::webview::WebViewBackend::reload(webview);
                    }
                });
                // 监听 WebKit title 变化 → 实时同步标签标题
                wv.connect_title_notify(move |webview| {
                    if let Some(t) = webview.title() {
                        let title = t.trim().to_string();
                        if title.is_empty() {
                            return;
                        }
                        // 内部启动页（shift://newtab/worldnav）标题固定"启动页"：
                        // 页面自身的 <title>（"新标签页"）不覆盖，否则启动页标签
                        // 刚出来一下又变成"新标签页"
                        if let Some(uri) = WebViewBackend::uri(webview) {
                            if is_internal_newtab_url(&uri) {
                                return;
                            }
                        }
                        // 标题变化 → 更新内存态；仅在真的变化时通知前端（防高频重复 emit）
                        let changed = {
                            if let Some(state) = app2.try_state::<TabManager>() {
                                let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
                                match tabs.get_mut(&tab_id) {
                                    Some(tab) if tab.title != title => {
                                        tab.title = title;
                                        true
                                    }
                                    _ => false,
                                }
                            } else {
                                false
                            }
                        };
                        if changed {
                            let _ = app2.emit(crate::contracts::events::TABS_UPDATED, ());
                        }
                    }
                });
            });
        }

        // 新标签创建后重新注入扩展 + 广告拦截（新 webview 需要注入）
        let app_for_inject = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::extensions::re_inject(&app_for_inject).await;
        });

        Ok(())
    }

}

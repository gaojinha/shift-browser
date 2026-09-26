//! TabManager::create_webview——webview 创建与信号接线（R2-1 拆分）

use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder};

use crate::db::Db;

use super::super::model::TabManager;
use crate::engine::layout::{CONTENT_TOP_OFFSET, VERTICAL_TAB_BAR_WIDTH};

// create_webview 的子步骤（按职责拆出以控制本文件行数）
#[path = "create/url.rs"]
mod url;
#[path = "create/page_load.rs"]
mod page_load;
#[cfg(target_os = "linux")]
#[path = "create/signals.rs"]
mod signals;

impl TabManager {
    /// 创建嵌入式 webview
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    pub(crate) fn create_webview(app: &AppHandle, id: &str, url: &str, private: bool) -> Result<(), String> {
        let window = app
            .get_window("main")
            .ok_or_else(|| "主窗口不存在".to_string())?;

        let webview_url = url::resolve_webview_url(app, url, private)?;

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
          // 页面加载前注入选区缓存监听：用户"选中→点侧栏"时选区会被浏览器清空，
          // 靠它在选中那一刻把 Range 存下来（详见 page_selwatch.js）。
          // 只挂一个 selectionchange 监听，不改页面、不读内容。
          .initialization_script(include_str!("../../page_selwatch.js"))
          // Linux：显式指定 web extensions 目录，否则 Tauri 的 IPC 桥依赖的 WebKit
          // web extension 不会加载，__TAURI_INTERNALS__ 注入失败。
          // 修复（2026-09-25）：Tauri 在 Linux 用 Cargo 包名拼资源路径，与 deb 的
          // productName 目录不一致 → 多候选探测含 .so 的目录（详见 platform::linux）。
          .extensions_path(
            crate::engine::platform::web_extensions_dir(app.path().resource_dir().ok())
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
              if let Ok(tab) = crate::tabs::commands::create_tab(app.clone(), state, url_str, None, None, None).await {
                log::info!("[tabs] on_new_window: 新标签创建成功 id={}", tab.id);
                // 页面内 window.open 建的新标签，前端不知情 → 通知同步
                let _ = app.emit(crate::contracts::events::TABS_UPDATED, ());
              }
            });
            tauri::webview::NewWindowResponse::Deny
          })
.on_page_load(move |webview, payload| {
            page_load::handle_page_load(&webview, &payload, &app_handle2, &tab_id);
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
        signals::wire_linux_signals(&child, app, id);

        // 新标签创建后重新注入扩展 + 广告拦截（新 webview 需要注入）
        let app_for_inject = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::extensions::re_inject(&app_for_inject).await;
        });

        Ok(())
    }

}

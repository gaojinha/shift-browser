// 标签页管理器
// 管理标签元数据 + 嵌入式 webview 生命周期
// 与 Electron 版 electron/tabManager.ts 对应（v4.5.0）

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewBuilder, WebviewUrl,
};

use crate::db::{downloads, history, Db};

#[cfg(target_os = "linux")]
use gtk::glib::translate::ToGlibPtr;
use gtk::prelude::*;
#[cfg(target_os = "linux")]
use webkit2gtk::{DownloadExt, URIRequestExt, URIResponseExt, WebContextExt, WebViewExt};

// UI 布局常量：统一由引擎层提供（唯一事实来源，见 engine/layout.rs）
pub use crate::engine::layout::CONTENT_TOP_OFFSET;
use crate::engine::layout::{
    BOOKMARKS_BAR_HEIGHT, CONTENT_TOP_OFFSET_NO_BOOKMARKS, TAB_BAR_HEIGHT, VERTICAL_TAB_BAR_WIDTH,
};

/// 键盘焦点是否在 UI 层（主 webview，跑 Vue 界面）。
/// 前端快捷键监听的是 HTML 层的 keydown，只在主 webview 持焦时触发；
/// 用户点击网页后焦点落到 child webview，按键到不了 HTML 层——此时由 GTK 顶层窗口
/// 兜底拦截并转发（emit `global-shortcut`），保证 Ctrl+T/W/R/L/H 等始终可用。
static UI_FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 清洗 URL 中的跨站追踪参数（隐私防护 P0）
/// 覆盖 utm_* 系列 + 主流平台点击 ID（fbclid/gclid/msclkid/spm 等）
fn clean_tracking_params(url: &str) -> String {
    const TRACKING_KEYS: &[&str] = &[
        "fbclid",
        "gclid",
        "dclid",
        "msclkid",
        "yclid",
        "twclid",
        "igshid",
        "ttclid",
        "mc_eid",
        "mc_cid",
        "mkt_tok",
        "_hsenc",
        "_hsmi",
        "vero_id",
        "oly_enc_id",
        "ref_src",
        "ref_url",
        "spm",
        "scm",
        "share_source",
        "share_medium",
        "from_source",
        "wd_uid",
        "s_kwcid",
        "elqTrackId",
        "trk",
    ];
    const TRACKING_PREFIXES: &[&str] = &["utm_", "pk_", "hsa_", "at_", "ito_"];
    // 只处理 http/https/query 存在的 URL
    if !url.contains('?') || !(url.starts_with("http://") || url.starts_with("https://")) {
        return url.to_string();
    }
    let Some((base, rest)) = url.split_once('?') else {
        return url.to_string();
    };
    let (query, fragment) = match rest.split_once('#') {
        Some((q, f)) => (q, Some(f)),
        None => (rest, None),
    };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|kv| {
            let key = kv.split('=').next().unwrap_or("").to_ascii_lowercase();
            if key.is_empty() {
                return false;
            }
            if TRACKING_KEYS.iter().any(|k| k.eq_ignore_ascii_case(&key)) {
                return false;
            }
            !TRACKING_PREFIXES.iter().any(|p| key.starts_with(p))
        })
        .collect();
    let cleaned = match (kept.is_empty(), fragment) {
        (true, Some(f)) => format!("{}#{}", base, f),
        (true, None) => base.to_string(),
        (false, Some(f)) => format!("{}?{}#{}", base, kept.join("&"), f),
        (false, None) => format!("{}?{}", base, kept.join("&")),
    };
    if cleaned != url {
        crate::extensions::CLEANED_PARAMS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        log::info!("[privacy] URL 追踪参数已清洗: {} -> {}", url, cleaned);
    }
    cleaned
}

/// 净化下载文件名（防路径穿越）：
/// 剥掉 / \ 路径分隔符、空字节/控制字符、首尾点段（防 ../ 与隐藏文件），限长 200 字符
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0' | '\n' | '\r' | '\t'))
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_string();
    if cleaned.is_empty() || cleaned == "." || cleaned == ".." {
        return "download".to_string();
    }
    let mut s: String = cleaned.chars().take(200).collect();
    if s.is_empty() {
        s = "download".to_string();
    }
    s
}

/// 标签页元数据（运行时状态，不持久化；会话保存走 sessions 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tab {
    pub id: String,
    pub url: String,
    pub title: String,
    pub favicon: String,
    pub is_active: bool,
    pub is_pinned: bool,
    pub is_muted: bool,
    /// 是否已休眠（webview 已销毁以释放内存；点击标签时按 url 重建）
    #[serde(default)]
    pub hibernated: bool,
}

/// 标签管理器（Tauri State）
pub struct TabManager {
    tabs: Mutex<HashMap<String, Tab>>,
    order: Mutex<Vec<String>>,
    active_id: Mutex<Option<String>>,
    /// 书签栏是否显示（影响 child webview 的 top 偏移：112 vs 82）
    show_bookmarks_bar: Mutex<bool>,
    /// set_webview_visible 已应用的最近 seq（P1-9：丢弃并发乱序的过期指令）
    webview_visible_seq: Mutex<u64>,
    /// AI 面板是否占用右侧（child webview 宽度让出 AI_PANEL_WIDTH，方便边看页面边对话）
    ai_panel_active: Mutex<bool>,
    /// 垂直标签模式（true 时左侧让出 VERTICAL_TAB_BAR_WIDTH，顶部不再含标签栏）
    vertical_tabs: Mutex<bool>,
    /// 分屏：右侧窗格承载的标签 id（None = 未分屏；左侧窗格始终是活动标签）
    split_tab: Mutex<Option<String>>,
    /// 每个标签最后一次激活时间（Unix 秒），用于"空闲休眠"判定
    last_active: Mutex<HashMap<String, i64>>,
}

/// 当前 Unix 时间戳（秒）
fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl TabManager {
    pub fn new() -> Self {
        Self {
            tabs: Mutex::new(HashMap::new()),
            order: Mutex::new(Vec::new()),
            active_id: Mutex::new(None),
            show_bookmarks_bar: Mutex::new(true),
            webview_visible_seq: Mutex::new(0),
            ai_panel_active: Mutex::new(false),
            vertical_tabs: Mutex::new(false),
            split_tab: Mutex::new(None),
            last_active: Mutex::new(HashMap::new()),
        }
    }

    /// 记录标签激活时间（休眠判定的输入）
    pub fn touch_active(&self, id: &str) {
        self.last_active
            .lock()
            .unwrap()
            .insert(id.to_string(), now_ts());
    }

    pub fn set_show_bookmarks_bar(&self, visible: bool) -> f64 {
        *self
            .show_bookmarks_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = visible;
        self.top_offset()
    }

    /// 当前内容区顶部偏移（随书签栏显隐变化）
    /// 运行时快照（自检/可观测性用）：标签数、活动标签、AI 分屏状态
    pub fn runtime_snapshot(&self) -> (usize, Option<String>, bool) {
        let count = self.tabs.lock().unwrap_or_else(|e| e.into_inner()).len();
        let active = self
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let ai = *self
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        (count, active, ai)
    }

    /// 活动标签的 URL（GPC 站点声明检测等用）
    pub fn active_url(&self) -> Option<String> {
        let id = self
            .active_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()?;
        let tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.get(&id).map(|t| t.url.clone())
    }

    /// 是否垂直标签模式（阶段 3.1）
    pub fn vertical_tabs(&self) -> bool {
        *self
            .vertical_tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// 设置垂直标签模式
    pub fn set_vertical_tabs(&self, on: bool) {
        *self
            .vertical_tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = on;
    }

    /// 分屏：右侧窗格承载的标签 id（None = 未分屏）
    pub fn split_tab(&self) -> Option<String> {
        self.split_tab
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// 设置/清除分屏标签
    pub fn set_split_tab(&self, id: Option<String>) {
        *self
            .split_tab
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = id;
    }

    /// 当前内容区顶部偏移（随书签栏显隐 + 垂直标签模式变化）
    pub fn top_offset(&self) -> f64 {
        let bookmarks = *self
            .show_bookmarks_bar
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        // 基准：标签栏 + 地址栏
        let mut t = CONTENT_TOP_OFFSET_NO_BOOKMARKS;
        if self.vertical_tabs() {
            // 垂直标签：标签栏移到左侧，顶部不再占标签栏高度
            t -= TAB_BAR_HEIGHT;
        }
        if bookmarks {
            t += BOOKMARKS_BAR_HEIGHT;
        }
        t
    }

    /// 生成 UUID v4（真随机，crypto 级）
    fn uuid() -> String {
        uuid::Uuid::new_v4().to_string()
    }

    pub fn all_tabs(&self) -> Vec<Tab> {
        let tabs = self.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let order = self.order.lock().unwrap_or_else(|e| e.into_inner());
        order
            .iter()
            .filter_map(|id| tabs.get(id).cloned())
            .collect()
    }

    /// 根据窗口尺寸计算内容区域大小（逻辑像素）；top 随书签栏显隐变化
    /// 实现已收敛到引擎层（engine/layout.rs），避免公式散落多处导致布局漂移
    fn content_size(
        window: &tauri::Window,
        top: f64,
        ai_active: bool,
        vertical: bool,
    ) -> (f64, f64) {
        crate::engine::layout::content_size(window, top, ai_active, vertical)
    }

    /// 创建嵌入式 webview
    fn create_webview(app: &AppHandle, id: &str, url: &str) -> Result<(), String> {
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
            log::info!("[tabs] on_new_window: url={}", url);
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
                  log::info!("[tabs] on_new_window: 重复请求已忽略 url={}", url_str);
                  return tauri::webview::NewWindowResponse::Deny;
                }
              }
              *last = Some((url_str.clone(), now));
            }
            let app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
              let state = app.state::<TabManager>();
              if let Ok(tab) = create_tab(app.clone(), state, url_str).await {
                log::info!("[tabs] on_new_window: 新标签创建成功 id={}", tab.id);
                // 页面内 window.open 建的新标签，前端不知情 → 通知同步
                let _ = app.emit("tabs-updated", ());
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

              log::info!("[tabs] page_load_finished: tab={} title={}", tab_id, title);

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
                          let _ = app_fav.emit("tabs-updated", ());
                        }
                      }
                    }
                  },
                );
              });

              // 按设置记录浏览历史（record_history 开关，默认开）——先 clone，避免 title 被 move
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
              }
              // 页面加载完成（URL/标题变化）→ 通知前端同步（替代 2s 轮询）
              let _ = app_handle2.emit("tabs-updated", ());

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
                        "page-load-failed",
                        (tab_id_fail.clone(), failing_uri.to_string()),
                    );
                    true
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
                        if let Some(uri) = webview.uri() {
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
                            let _ = app2.emit("tabs-updated", ());
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
          crate::engine::webview::apply_default_settings(&child_gtk);
          // 下载策略（关键 · 2026-09-21）：主框架导航到"不可显示的 MIME"（apk/zip/exe/…）时，
          // WebKit 默认不当下载 → 表现为"帧加载已中断"（用户报"下载不能开始"）。
          // 这里接 decide-policy：对不支持的 MIME 调 decision.download()，
          // 触发 WebContext 的 download-started（已注册，落 ~/Downloads + 记录 downloads 表）。
          child_gtk.connect_decide_policy(|_wv, decision, decision_type| {
            use webkit2gtk::{
              PolicyDecisionExt, PolicyDecisionType, ResponsePolicyDecision,
              ResponsePolicyDecisionExt,
            };
            if decision_type == PolicyDecisionType::Response {
              if let Some(resp) = decision.downcast_ref::<ResponsePolicyDecision>() {
                if !resp.is_mime_type_supported() {
                  log::info!("[downloads] 不可显示 MIME → 转下载流程");
                  decision.download();
                  return true;
                }
              }
            }
            false
          });
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
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                let dest_dir = format!("{}/Downloads", home);
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
                    let _ = app0.emit("downloads-updated", ());
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
                    let id_now = { *id_state5.lock().unwrap_or_else(|e| e.into_inner()) };
                    let id = match id_now {
                      Some(id) => Some(id),
                      None => wait_download_id(&id_state5).await,
                    };
                    if let Some(id) = id {
                      let _ = downloads::update_download_state(&pool5, id, "completed", Some(received)).await;
                      let _ = app5.emit("downloads-updated", ());
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
                      let _ = app7.emit("downloads-updated", ());
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

    /// 标签切换专用：先 hide 旧标签再 show 新标签
    /// 注意 1：绝不能在 run_on_main_thread 闭包里再调 wv.show()/hide()——
    /// 那两个 API 内部是"主线程则直接 handle_user_message"的重入路径，
    /// 在事件循环处理 Task 时重入会导致可见性变更静默失效（切换无反应）。
    /// 注意 2：顺序必须 hide→show。GtkFixed 里后添加的 widget 盖在上面，
    /// 若先 show 新再 hide 旧，旧标签（若后创建）会盖住新标签，
    /// 表现为"切了标签内容不变，很久才变过来"。hide 先行后白屏空窗仅毫秒级。
    fn switch_visibility(
        app: &AppHandle,
        show_id: &str,
        hide_id: Option<&str>,
    ) -> Result<(), String> {
        let window = app
            .get_window("main")
            .ok_or_else(|| "主窗口不存在".to_string())?;
        let show_wv = window
            .get_webview(&format!("tab-{}", show_id))
            .ok_or_else(|| "目标标签 webview 不存在".to_string())?;
        let hide_wv = hide_id.and_then(|h| window.get_webview(&format!("tab-{}", h)));
        // 先隐藏旧标签（避免 z-order 盖住新标签），再显示新标签
        if let Some(wv) = &hide_wv {
            let _ = wv.hide();
        }
        let _ = show_wv.show();
        let _ = show_wv.set_focus();
        // 修复：show 后重新设置尺寸。hide 期间 set_size_request 不触发 GTK 重新分配，
        // 窗口 resize 后切回该标签会停留在旧尺寸（小窗口大小不跟随放大）
        let top = if let Some(state) = app.try_state::<TabManager>() {
            state.top_offset()
        } else {
            CONTENT_TOP_OFFSET
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
        let geo =
            crate::engine::layout::ContentGeometry::compute(&window, top, ai_active, vertical);
        crate::engine::layout::apply_to_webview(&window, show_id, geo);
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
                // 直接用 GDK 键值常量比对（避免 gdk 版本间 keys 常量路径差异）
                // F12=0xFFC9 I/i=0x49/0x69 T/t=0x54/0x74 W/w=0x57/0x77 R/r=0x52/0x72
                // L/l=0x4C/0x6C H/h=0x48/0x68 S/s=0x53/0x73 K/k=0x4B/0x6B Tab=0xFF09
                const KEY_F12: u32 = 0xFFC9;
                const KEY_TAB: u32 = 0xFF09;
                let key: u32 = *event.keyval();
                let state = event.state();
                let ctrl = state.contains(gtk::gdk::ModifierType::CONTROL_MASK);
                let shift = state.contains(gtk::gdk::ModifierType::SHIFT_MASK);
                let is = |lo: u32, up: u32| key == lo || key == up;

                // ① 开发者工具：不依赖焦点，任何情况都响应
                if key == KEY_F12 || (ctrl && shift && is(0x69, 0x49)) {
                    open_devtools_for_active(&app_handle);
                    return gtk::glib::Propagation::Stop;
                }

                // ② 其余浏览器级快捷键：仅在「页面持焦」时兜底。
                // UI 层持焦时前端 keydown 已处理，这里必须放行，否则会重复触发（如建两个标签）。
                if ctrl && !UI_FOCUSED.load(std::sync::atomic::Ordering::Relaxed) {
                    let action = if is(0x74, 0x54) && !shift {
                        Some("new-tab")
                    } else if is(0x74, 0x54) && shift {
                        Some("translate-panel")
                    } else if is(0x77, 0x57) {
                        Some("close-tab")
                    } else if is(0x72, 0x52) {
                        Some("reload")
                    } else if is(0x6C, 0x4C) {
                        Some("focus-address")
                    } else if is(0x68, 0x48) {
                        Some("history")
                    } else if is(0x6B, 0x4B) {
                        Some("command-palette")
                    } else if is(0x73, 0x53) && shift {
                        Some("sessions")
                    } else if key == KEY_TAB && !shift {
                        Some("next-tab")
                    } else if key == KEY_TAB && shift {
                        Some("prev-tab")
                    } else {
                        None
                    };
                    if let Some(action) = action {
                        log::debug!("[tabs] GTK 层转发快捷键: {}", action);
                        let _ = app_handle.emit("global-shortcut", action);
                        return gtk::glib::Propagation::Stop;
                    }
                }
                gtk::glib::Propagation::Proceed
            });
            log::info!("[tabs] 全局快捷键已挂载：F12 / Ctrl+Shift+I 打开开发者工具");
        }
    }

    Ok(())
}

/// 下载目标路径唯一化：同名文件已存在时加 (1)(2)... 后缀，避免覆盖
fn unique_download_path(dir: &str, filename: &str) -> String {
    let candidate = format!("{}/{}", dir, filename);
    if !std::path::Path::new(&candidate).exists() {
        return candidate;
    }
    let (stem, ext) = match filename.rfind('.') {
        Some(i) if i > 0 => (&filename[..i], &filename[i..]),
        _ => (filename, ""),
    };
    let mut n = 1;
    loop {
        let c = format!("{}/{}({}){}", dir, stem, n, ext);
        if !std::path::Path::new(&c).exists() {
            return c;
        }
        n += 1;
    }
}

/// 应用某个标签 webview 的内容区几何（按当前窗口尺寸重算）。
/// 用于"显示之后"重新分配：Fixed/webview 隐藏期间尺寸变更不会触发 GTK 重新分配，
/// 且 WebKit 内部输入窗口不跟随尺寸变化，表现为"页面能看到但点不动"。
/// 实现委托引擎层（engine/layout.rs）。
#[cfg(target_os = "linux")]
fn apply_tab_geometry(window: &tauri::Window, id: &str, top: f64, ai_active: bool) {
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
fn apply_split_layout(app: &AppHandle, state: &TabManager) -> Result<(), String> {
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

/// 创建新标签页
#[tauri::command]
pub async fn create_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    url: String,
) -> Result<Tab, String> {
    // 隐私防护：新建标签前清洗跨站追踪参数（只影响 http/https，内部页面不受影响）
    let url = clean_tracking_params(&url);
    log::info!("[tabs] create_tab: url={}", url);
    // P2：URL scheme 白名单——只允许 http/https、内部页面（file://newtab、shift://）。
    // 拒绝 file:// 其他路径（本地文件泄露）与 javascript:/data: 等危险 scheme
    // 例外：阅读模式生成的 file:///.../reader/r*.html（enter_reader_mode 写入 app_data/reader，
    // 内容经 sanitize_html 净化，路径固定）——2026-09-19 修复：此前被白名单误拒，
    // 阅读模式提取成功但新标签打不开
    let parsed = url
        .parse::<tauri::Url>()
        .map_err(|e| format!("URL 解析失败: {}", e))?;
    match parsed.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains("/newtab/")
                && !url.contains("world-nav.html")
                && !url.contains("/reader/")
            {
                return Err("不允许打开本地文件".to_string());
            }
        }
        other => return Err(format!("不允许的 URL scheme: {}", other)),
    }
    let id = TabManager::uuid();
    // 本地新标签页（file:// 路径含 /newtab/ 或 shift://newtab/worldnav）标题显示"启动页"，
    // 不显示冗长 URL/host。注意：协议改版后 URL 是 shift://newtab，旧的 contains("/newtab/")
    // 判断匹配不到，必须按 scheme+host 判断
    let title = if is_internal_newtab_url(&url) {
        "启动页".to_string()
    } else {
        url.clone()
    };

    TabManager::create_webview(&app, &id, &url)?;
    log::info!("[tabs] create_tab: webview 创建成功 id={}", id);

    let tab = Tab {
        id: id.clone(),
        url,
        title,
        favicon: String::new(),
        is_active: true,
        is_pinned: false,
        is_muted: false,
        hibernated: false,
    };
    state.touch_active(&id);

    // 更新状态：新标签激活，旧标签取消激活
    let old_active = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());

        for t in tabs.values_mut() {
            t.is_active = false;
        }
        tabs.insert(id.clone(), tab.clone());
        order.push(id.clone());
        active.replace(id.clone())
    };

    // 新建标签页：立即切换可见性（show 新、hide 旧），避免新 webview 透明期透出旧页面
    if let Some(old) = old_active {
        let _ = TabManager::switch_visibility(&app, &id, Some(&old));
    } else {
        let _ = TabManager::set_webview_visible(&app, &id, true);
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
    log::info!("[tabs] close_tab: id={}", id);
    let mut next_id: Option<String> = None;

    {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());

        let was_active = *active == Some(id.clone());
        let removed = tabs.remove(&id);
        // 记录被删标签在 order 中的位置（删除后同位置即右邻）
        let removed_pos = order.iter().position(|x| x == &id);
        if let Some(pos) = removed_pos {
            order.remove(pos);
        }

        if was_active {
            // 优先切右边的标签（删除后同位置即右邻），没有则切左边（最后一个）
            next_id = match removed_pos {
                Some(p) if p < order.len() => order.get(p).cloned(),
                _ => order.last().cloned(),
            };
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

    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(next_id.and_then(|nid| tabs.get(&nid).cloned()))
}

/// 关闭全部标签（切换工作区时清空用）
#[tauri::command]
pub async fn close_all_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    log::info!("[tabs] close_all_tabs");
    let ids: Vec<String> = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());
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
    log::info!("[tabs] switch_tab: id={}", id);
    // 先校验标签存在，再写 active_id（否则 active_id 会指向不存在的标签，
    // 导致 go_back/go_forward 报"标签不存在"、UI 显示与实际不符）
    let hibernated_info = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.get(&id).map(|t| (t.hibernated, t.url.clone()))
    };
    let Some((is_hibernated, hibernated_url)) = hibernated_info else {
        return Ok(None);
    };

    // 休眠标签的恢复：webview 此前已被销毁释放内存，这里按保存的 URL 重建。
    // 重建后需重新注入扩展/隐私防护脚本，并重新计算布局（create_webview 内部处理）。
    if is_hibernated {
        log::info!("[perf] 唤醒休眠标签: id={} url={}", id, hibernated_url);
        TabManager::create_webview(&app, &id, &hibernated_url)?;
        if let Some(t) = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&id)
        {
            t.hibernated = false;
        }
        crate::extensions::re_inject(&app).await;
    }
    // 隐藏当前活动 webview
    let old_active = {
        let mut active = state.active_id.lock().unwrap_or_else(|e| e.into_inner());
        let old = active.clone();
        *active = Some(id.clone());
        old
    };

    // 更新激活状态
    {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        for t in tabs.values_mut() {
            t.is_active = t.id == id;
        }
    }

    // 切换可见性：一次主线程投递里"先显示新标签、再隐藏旧标签"
    // （原实现 hide→show 两次投递且顺序相反，中间白屏空窗 + 双跨线程往返导致切换迟钝）
    if state.split_tab().is_some() {
        // 分屏中：重排两个窗格（活动标签变了，右窗格保持），避免把分屏标签误隐藏
        let _ = apply_split_layout(&app, &state);
    } else if old_active.as_deref() == Some(id.as_str()) {
        // 点的就是当前标签：仅确保可见
        let _ = TabManager::set_webview_visible(&app, &id, true);
    } else {
        let _ = TabManager::switch_visibility(&app, &id, old_active.as_deref());
    }

    state.touch_active(&id);
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 空闲标签休眠：销毁长时间未激活的标签 webview，回收其渲染进程内存
/// （WebKitGTK 默认不主动回收，实测 9 标签合计 3.75GB）。
/// 返回本次休眠的标签数。超时分钟数来自 settings.tab_hibernate_minutes（0 = 关闭）。
#[tauri::command]
pub async fn hibernate_idle_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    db: tauri::State<'_, Db>,
) -> Result<u32, String> {
    // 读取超时配置（默认 30 分钟；0 表示关闭该功能）
    let minutes: i64 = {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = 'tab_hibernate_minutes'")
                .fetch_optional(&db.pool)
                .await
                .ok()
                .flatten();
        row.and_then(|(v,)| v.parse::<i64>().ok()).unwrap_or(30)
    };
    if minutes <= 0 {
        return Ok(0);
    }
    let threshold = now_ts() - minutes * 60;
    let active = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();

    // 选出候选：非激活、未休眠、且最后激活时间早于阈值
    let candidates: Vec<String> = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let last = state.last_active.lock().unwrap_or_else(|e| e.into_inner());
        tabs.iter()
            .filter(|(id, t)| {
                !t.hibernated
                    && Some((*id).clone()) != active
                    && last.get(*id).map(|ts| *ts < threshold).unwrap_or(true)
            })
            .map(|(id, _)| id.clone())
            .collect()
    };
    if candidates.is_empty() {
        return Ok(0);
    }

    let Some(window) = app.get_window("main") else {
        return Ok(0);
    };
    let mut count = 0u32;
    for id in candidates {
        let label = format!("tab-{}", id);
        if let Some(wv) = window.get_webview(&label) {
            // 先隐藏再销毁，避免销毁瞬间的残影
            let _ = wv.hide();
            if let Err(e) = wv.close() {
                log::warn!("[perf] 标签休眠失败（close）: id={} err={}", id, e);
                continue;
            }
            if let Some(t) = state
                .tabs
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get_mut(&id)
            {
                t.hibernated = true;
            }
            count += 1;
            log::info!("[perf] 标签已休眠（释放渲染进程）: id={}", id);
        }
    }
    if count > 0 {
        let _ = app.emit("tabs-updated", ());
        log::info!("[perf] 本轮共休眠 {} 个空闲标签", count);
    }
    Ok(count)
}

/// 翻译当前活动标签的整个页面（供前端「翻译当前页面」按钮调用）
/// 复用既有整页翻译脚本：脚本自包含（进度浮层 + 分批调 shift://__translate__/batch + 回填 DOM），
/// 放在 tabs.rs 是为了直接访问 TabManager 的 active_id（该字段不对外暴露）
#[tauri::command]
pub async fn translate_current_page(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    let id = state
        .active_id
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "当前没有活动标签".to_string())?;
    let label = format!("tab-{}", id);
    let wv = app
        .get_webview(&label)
        .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;
    wv.eval(crate::WHOLE_PAGE_TRANSLATE_JS)
        .map_err(|e| format!("注入整页翻译脚本失败: {}", e))?;
    log::info!("[translate] 已对标签 {} 注入整页翻译脚本", id);
    Ok(())
}

/// 导航到新 URL
#[tauri::command]
pub async fn navigate_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
    url: String,
) -> Result<Option<Tab>, String> {
    // 隐私防护：导航前清洗跨站追踪参数（utm_*/fbclid/gclid/msclkid/spm 等）
    let url = clean_tracking_params(&url);
    log::info!("[tabs] navigate_tab: id={} url={}", id, url);
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
    // P2：导航同样做 scheme 白名单（与 create_tab 一致），防止注入的脚本导航到 file://
    match parsed_url.scheme() {
        "http" | "https" | "shift" => {}
        "file" => {
            if !url.contains("/newtab/") && !url.contains("world-nav.html") {
                return Err("不允许导航到本地文件".to_string());
            }
        }
        other => return Err(format!("不允许的 URL scheme: {}", other)),
    }
    webview
        .navigate(parsed_url)
        .map_err(|e| format!("导航失败: {}", e))?;

    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    let mut tab = tabs.get_mut(&id);
    if let Some(ref mut t) = tab {
        t.url = url.clone();
        t.title = url;
    }
    Ok(tab.cloned())
}

/// 刷新当前标签（Ctrl+R）
#[tauri::command]
pub async fn reload_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    log::info!("[tabs] reload_tab: id={}", id);
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;
    webview
        .with_webview(|pwv| {
            crate::engine::webview::reload(&pwv.inner());
        })
        .map_err(|e| format!("刷新失败: {}", e))?;
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 更新标签顺序（拖拽排序）；前端传新顺序，缺失的现存标签兜底追加到末尾
#[tauri::command]
pub async fn reorder_tabs(
    state: tauri::State<'_, TabManager>,
    ids: Vec<String>,
) -> Result<(), String> {
    let existing_set: std::collections::HashSet<String> = {
        let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        tabs.keys().cloned().collect()
    };
    let mut new_order: Vec<String> = ids
        .iter()
        .filter(|id| existing_set.contains(*id))
        .cloned()
        .collect();
    let mut order = state.order.lock().unwrap_or_else(|e| e.into_inner());
    for id in order.iter() {
        if !new_order.contains(id) {
            new_order.push(id.clone());
        }
    }
    *order = new_order;
    log::info!("[tabs] reorder_tabs: 新顺序 {:?}", order);
    Ok(())
}

/// 获取所有标签页（按顺序）
#[tauri::command]
pub async fn get_tabs(state: tauri::State<'_, TabManager>) -> Result<Vec<Tab>, String> {
    let tabs = state.all_tabs();
    // 诊断"前端标签列表为空/标签不显示"问题：记录每次同步的标签数（事件驱动 + 8s 轮询）
    log::info!("[tabs] get_tabs: {} 个标签", tabs.len());
    Ok(tabs)
}

/// 隐藏/显示所有标签页 webview（弹窗/菜单等 Vue UI 覆盖层需要）
/// source: 调用来源标识（前端调试用，可选）
/// seq: 单调递增令牌（P1-9）——前端快速开关面板时多次调用并发乱序，
/// 只让最新 seq 生效，过期指令直接丢弃，避免"最后完成的是旧指令"导致
/// webview 显隐状态与面板实际状态相反。
#[tauri::command]
pub async fn set_webview_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
    seq: u64,
    source: Option<String>,
) -> Result<(), String> {
    // 过期指令丢弃：并发乱序时只应用最大 seq
    {
        let mut last = state
            .webview_visible_seq
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if seq <= *last {
            log::info!(
                "[tabs] set_webview_visible 过期指令丢弃 seq={} last={} source={:?}",
                seq,
                *last,
                source
            );
            return Ok(());
        }
        *last = seq;
    }
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    // 注意：child webview 不在 window.webviews() 列表里（只有 main），
    // 必须从 TabManager 拿已知标签逐个 get_webview
    let ids: Vec<String> = state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    let mut count = 0usize;
    let ids_for_geometry = ids.clone();
    for id in ids {
        let label = format!("tab-{}", id);
        if let Some(wv) = window.get_webview(&label) {
            let wv2 = wv.clone();
            let _ = wv.with_webview(move |pwv| {
                let gtk_wv = pwv.inner();
                // 同时隐藏/显示 Fixed 容器：Fixed 覆盖内容区会拦截弹窗点击，必须一起隐藏
                crate::engine::layout::set_fixed_visible(&gtk_wv, visible);
                if visible {
                    let _ = wv2.show();
                } else {
                    let _ = wv2.hide();
                }
            });
            count += 1;
        }
    }
    // 恢复显示后重新应用几何：隐藏期间（弹窗打开时）可能改过书签栏显隐/AI 分屏/窗口尺寸，
    // 不重设会导致 webview 尺寸或位置滞后，WebKit 输入窗口不跟随 → 页面点不动
    if visible {
        if state.split_tab().is_some() {
            // 分屏中：按窗格显示（否则"恢复全部"会把分屏覆盖成全部重叠——右键菜单/弹窗关闭时触发）
            apply_split_layout(&app, &state)?;
        } else {
            let top = state.top_offset();
            let ai_active = *state
                .ai_panel_active
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            for id in &ids_for_geometry {
                apply_tab_geometry(&window, id, top, ai_active);
            }
        }
    }
    log::info!(
        "[tabs] set_webview_visible: visible={} count={} source={:?}",
        visible,
        count,
        source
    );
    Ok(())
}

/// AI 面板占用右侧空间（分屏对话）：active=true 时 child webview 宽度让出
/// AI_PANEL_WIDTH，右侧留给 HTML 层 AI 侧边栏；active=false 恢复全宽。
/// 与 set_webview_visible 不同：这里不隐藏 webview，页面保持可见可交互。
#[tauri::command]
pub async fn set_webview_ai_panel(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    active: bool,
) -> Result<(), String> {
    *state
        .ai_panel_active
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = active;
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let top = state.top_offset();
    let vertical = state.vertical_tabs();
    let geo = crate::engine::layout::ContentGeometry::compute(&window, top, active, vertical);
    let ids: Vec<String> = state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect();
    for id in ids {
        crate::engine::layout::apply_to_webview(&window, &id, geo);
    }
    log::info!(
        "[tabs] set_webview_ai_panel: active={} content={}x{}",
        active,
        geo.width,
        geo.height
    );
    // 焦点切换：AI 面板打开时把 GTK 键盘焦点交给主 webview（HTML 层），
    // 否则焦点留在 child webview（页面）上，AI 输入框收不到键盘事件。
    // 关闭时把焦点还给当前活动标签的 webview。
    if let Some(main_wv) = window.get_webview("main") {
        if active {
            let _ = main_wv.set_focus();
        } else {
            let active_id = state
                .active_id
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if let Some(id) = active_id {
                if let Some(wv) = window.get_webview(&format!("tab-{}", id)) {
                    let _ = wv.set_focus();
                }
            }
        }
    }
    Ok(())
}

/// 后退（对当前活动标签执行 history.back()）
#[tauri::command]
pub async fn go_back(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
    let id = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_back: tab={}", id);
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
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    Ok(tabs.get(&id).cloned())
}

/// 前进（对当前活动标签执行 history.forward()）
#[tauri::command]
pub async fn go_forward(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<Option<Tab>, String> {
    let id = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(id) = id else {
        return Ok(None);
    };
    log::info!("[tabs] go_forward: tab={}", id);
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
    let tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
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
    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
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

/// 切换书签栏显隐，返回新的 top_offset
#[tauri::command]
pub async fn set_bookmarks_bar_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
) -> Result<f64, String> {
    let new_top = state.set_show_bookmarks_bar(visible);
    log::info!(
        "[tabs] 书签栏显隐切换: visible={} 新 top_offset={}",
        visible,
        new_top
    );

    // 即时重算：不依赖下次窗口 Resize，直接按新的 top_offset 重算几何并应用
    // （几何计算统一走引擎层，见 engine/layout.rs）
    if let Some(window) = app.get_window("main") {
        let ai_active = *state
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let vertical = state.vertical_tabs();
        let geo =
            crate::engine::layout::ContentGeometry::compute(&window, new_top, ai_active, vertical);
        let ids: Vec<String> = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            crate::engine::layout::apply_to_webview(&window, &id, geo);
        }
    }

    Ok(new_top)
}

/// 切换垂直标签模式（阶段 3.1），返回新的 top_offset
#[tauri::command]
pub async fn set_vertical_tabs(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    active: bool,
) -> Result<f64, String> {
    state.set_vertical_tabs(active);
    let new_top = state.top_offset();
    log::info!(
        "[tabs] 垂直标签模式切换: active={} 新 top_offset={}",
        active,
        new_top
    );

    // 即时重算：标签栏从顶部移到左侧，影响内容区 top 与 left，直接重算几何并应用到所有标签
    if let Some(window) = app.get_window("main") {
        let ai_active = *state
            .ai_panel_active
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let geo =
            crate::engine::layout::ContentGeometry::compute(&window, new_top, ai_active, active);
        let ids: Vec<String> = state
            .tabs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in ids {
            crate::engine::layout::apply_to_webview(&window, &id, geo);
        }
    }

    Ok(new_top)
}

/// 开启分屏：右侧窗格显示指定标签（阶段 3.1 分屏浏览）
#[tauri::command]
pub async fn set_split_tab(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<(), String> {
    if !state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
    {
        return Err("标签不存在".to_string());
    }
    state.set_split_tab(Some(id.clone()));
    log::info!("[tabs] set_split_tab: {}", id);
    apply_split_layout(&app, &state)
}

/// 关闭分屏
#[tauri::command]
pub async fn clear_split(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    state.set_split_tab(None);
    log::info!("[tabs] clear_split");
    apply_split_layout(&app, &state)
}

/// 查询当前分屏标签（None = 未分屏）
#[tauri::command]
pub async fn get_split_tab(state: tauri::State<'_, TabManager>) -> Result<Option<String>, String> {
    Ok(state.split_tab())
}

/// 获取当前活动标签的导航状态（可后退/可前进）
#[tauri::command]
pub async fn get_nav_state(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> Result<serde_json::Value, String> {
    let id = state
        .active_id
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let Some(id) = id else {
        return Ok(json!({ "canGoBack": false, "canGoForward": false }));
    };
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let webview = window
        .get_webview(&label)
        .ok_or_else(|| format!("标签 {} 的 webview 不存在", id))?;

    // with_webview 闭包要求 'static + Send，用 Arc<Mutex> 捕获结果
    let nav = Arc::new(Mutex::new((false, false)));
    let nav_clone = nav.clone();
    webview
        .with_webview(move |pwv| {
            let (back, forward) = crate::engine::webview::navigation_state(&pwv.inner());
            let mut guard = nav_clone.lock().unwrap_or_else(|e| e.into_inner());
            guard.0 = back;
            guard.1 = forward;
        })
        .map_err(|e| format!("获取导航状态失败: {}", e))?;

    let (can_go_back, can_go_forward) = *nav.lock().unwrap_or_else(|e| e.into_inner());
    Ok(json!({ "canGoBack": can_go_back, "canGoForward": can_go_forward }))
}

/// 打开活动标签的开发者工具（命令与 GTK 全局快捷键共用）。
/// 走异步任务派发：从 GTK 信号处理器里直接调 `with_webview` 有主线程重入风险，
/// 异步派发由 Tauri 正确调度到主线程执行。
pub fn open_devtools_for_active(app: &AppHandle) {
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

/// 打开当前活动标签的开发者工具（Web Inspector）。
/// 依赖创建 webview 时设置的 `Settings.enable-developer-extras`（本文件 create 流程中已开启）。
/// 快捷键入口在 GTK 窗口层统一拦截（F12 / Ctrl+Shift+I），见 `setup_gtk_layout`：
/// 页面（child webview）持有焦点时 HTML 层收不到按键，必须在 GTK 层兜底。
#[tauri::command]
pub async fn open_devtools(
    app: AppHandle,
    _state: tauri::State<'_, TabManager>,
) -> Result<(), String> {
    open_devtools_for_active(&app);
    Ok(())
}

/// 固定/取消固定标签（原版 tab-pin IPC）
#[tauri::command]
pub async fn toggle_pin(
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
    let mut tab = tabs.get_mut(&id);
    if let Some(ref mut t) = tab {
        t.is_pinned = !t.is_pinned;
        log::info!("[tabs] toggle_pin: id={} pinned={}", id, t.is_pinned);
    }
    Ok(tab.cloned())
}

/// 静音/取消静音标签。
/// 真实现：翻转内存状态 + 异步派发到主线程调 WebKit `set_is_muted`
/// （WebKitGTK 2.36+ 支持 per-webview 静音，绑定里 `WebViewExt::set_is_muted` 存在）。
#[tauri::command]
pub async fn toggle_mute(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> Result<Option<Tab>, String> {
    let result = {
        let mut tabs = state.tabs.lock().unwrap_or_else(|e| e.into_inner());
        let mut tab = tabs.get_mut(&id);
        if let Some(ref mut t) = tab {
            t.is_muted = !t.is_muted;
            log::info!("[tabs] toggle_mute: id={} muted={}", id, t.is_muted);
        }
        tab.cloned()
    };
    let muted = result.as_ref().map(|t| t.is_muted).unwrap_or(false);

    // 真静音：异步派发到主线程（从 GTK 信号/异步上下文直接调 with_webview 有主线程重入风险）
    let app2 = app.clone();
    let id2 = id.clone();
    tauri::async_runtime::spawn(async move {
        let Some(window) = app2.get_window("main") else {
            return;
        };
        let label = format!("tab-{}", id2);
        if let Some(wv) = window.get_webview(&label) {
            let _ = wv.with_webview(move |pwv| {
                crate::engine::webview::set_muted(&pwv.inner(), muted);
            });
        }
    });

    Ok(result)
}

/// 最大化/还原主窗口（双击标签栏空白处调用）
#[tauri::command]
pub async fn toggle_maximize(app: AppHandle) -> Result<bool, String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        window.unmaximize().map_err(|e| e.to_string())?;
    } else {
        window.maximize().map_err(|e| e.to_string())?;
    }
    Ok(!maximized)
}

/// 最小化主窗口（标题栏/标签栏合并后的窗口控制按钮）
#[tauri::command]
pub async fn minimize_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.minimize().map_err(|e| e.to_string())
}

/// 关闭主窗口（标题栏/标签栏合并后的窗口控制按钮）
#[tauri::command]
pub async fn close_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.close().map_err(|e| e.to_string())
}

/// 开始拖动主窗口（无边框窗口标题栏拖拽，标签栏空白处按住调用）
#[tauri::command]
pub async fn start_drag_window(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    window.start_dragging().map_err(|e| e.to_string())
}

/// 判断 URL 是否为内部启动页（标题显示"启动页"）
/// 协议改版后新标签页 URL 是 shift://newtab（host=newtab），旧 file:// 路径
/// 含 /newtab/ 的判断匹配不到——必须按 scheme+host 判断，两处调用点（create_tab
/// 初始标题 + on_page_load 覆盖）共用同一逻辑，避免再次漂移
fn is_internal_newtab_url(url: &str) -> bool {
    match url.parse::<tauri::Url>() {
        Ok(u) => match u.scheme() {
            "shift" => matches!(u.host_str(), Some("newtab") | Some("worldnav")),
            "file" => url.contains("/newtab/") || url.contains("world-nav.html"),
            _ => false,
        },
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_newtab_url_detection() {
        // shift:// 协议（当前实际使用）
        assert!(is_internal_newtab_url("shift://newtab"));
        assert!(is_internal_newtab_url("shift://newtab/"));
        assert!(is_internal_newtab_url("shift://worldnav"));
        assert!(is_internal_newtab_url("shift://worldnav/world-nav.html"));
        // 旧 file:// 路径（兼容）
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/index.html"
        ));
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/world-nav.html"
        ));
        // 普通网页/其他协议不是启动页
        assert!(!is_internal_newtab_url("https://example.com"));
        assert!(!is_internal_newtab_url("http://example.com/newtab/"));
        assert!(!is_internal_newtab_url("file:///etc/passwd"));
        assert!(!is_internal_newtab_url("shift://__translate__/text"));
    }

    #[test]
    fn unique_path_adds_suffix_when_exists() {
        let dir = std::env::temp_dir().join("sb-unique-test");
        let _ = std::fs::create_dir_all(&dir);
        let dir = dir.to_str().unwrap();

        // 不存在 → 原文件名
        let p1 = unique_download_path(dir, "a.pdf");
        assert_eq!(p1, format!("{}/a.pdf", dir));

        // 创建后 → a(1).pdf
        std::fs::write(&p1, "x").unwrap();
        let p2 = unique_download_path(dir, "a.pdf");
        assert_eq!(p2, format!("{}/a(1).pdf", dir));

        // 再创建 → a(2).pdf
        std::fs::write(&p2, "x").unwrap();
        let p3 = unique_download_path(dir, "a.pdf");
        assert_eq!(p3, format!("{}/a(2).pdf", dir));

        // 无扩展名
        let p4 = unique_download_path(dir, "noext");
        assert_eq!(p4, format!("{}/noext", dir));
        std::fs::write(&p4, "x").unwrap();
        let p5 = unique_download_path(dir, "noext");
        assert_eq!(p5, format!("{}/noext(1)", dir));

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("sb-unique-test"));
    }

    #[test]
    fn sanitize_filename_blocks_path_traversal() {
        // 路径分隔符剥离
        assert_eq!(sanitize_filename("a/../b.txt"), "a..b.txt");
        assert_eq!(sanitize_filename("..\\evil.txt"), "evil.txt");
        // 纯穿越段 → 默认名
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename(""), "download");
        // 空字节/控制字符剥离
        assert_eq!(sanitize_filename("a\0b.txt"), "ab.txt");
        assert_eq!(sanitize_filename("a\nb.txt"), "ab.txt");
        // 正常名保留
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        // 首尾点段剥离（防隐藏文件）
        assert_eq!(sanitize_filename(".secret"), "secret");
        assert_eq!(sanitize_filename("..hidden"), "hidden");
        // 超长截断
        let long = "x".repeat(500);
        assert_eq!(sanitize_filename(&long).len(), 200);
    }
}

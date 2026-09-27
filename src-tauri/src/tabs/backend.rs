//! 标签页 GTK/webview 后端实现（R2-1 拆分：从 tabs.rs 抽出）
//!
//! 全部 GTK/webkit 相关实现：TabManager 的渲染/生命周期方法、GTK 布局、
//! 分屏几何、下载信号、崩溃重载冷却等。纯数据状态机见 super::model，
//! IPC 命令见 super::commands。

use tauri::{AppHandle, Manager};

use super::model::TabManager;
use crate::engine::layout::CONTENT_TOP_OFFSET;

/// 键盘焦点是否在 UI 层（主 webview，跑 Vue 界面）。
/// 前端快捷键监听的是 HTML 层的 keydown，只在主 webview 持焦时触发；
/// 用户点击网页后焦点落到 child webview，按键到不了 HTML 层——此时由 GTK 顶层窗口
/// 兜底拦截并转发（emit `global-shortcut`），保证 Ctrl+T/W/R/L/H 等始终可用。
pub(crate) static UI_FOCUSED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// 证书警告等"全屏覆盖层"激活时置 true：布局重排（apply_split_layout）只隐藏 webview、
/// 不再显示，否则覆盖层会被 GTK 层 child webview 盖住。由 set_webview_visible 命令维护。
pub(crate) static UI_OVERLAY_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Web 进程崩溃自动重载的冷却表（tab_id → 上次崩溃 unix 秒），防确定性崩溃导致无限重载
pub(crate) static WEB_CRASH_COOLDOWN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, i64>>> =
    std::sync::OnceLock::new();

impl TabManager {
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

    /// 切换 webview 显隐
    pub(crate) fn set_webview_visible(app: &AppHandle, id: &str, visible: bool) -> Result<(), String> {
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
    pub(crate) fn switch_visibility(
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
            state.ai_panel_active()
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
    pub(crate) fn destroy_webview(app: &AppHandle, id: &str) {
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


// backend 模块由 tabs.rs 以 #[path = "tabs/backend.rs"] 引入，子模块基准目录随之变为 tabs/，
// 故此处需显式 #[path] 指向 backend/ 子目录。
#[path = "backend/create.rs"]
mod create;
#[path = "backend/layout.rs"]
mod layout;

pub(crate) use layout::{apply_split_layout, open_devtools_for_active, setup_gtk_layout};

/// 下载目标路径唯一化：同名文件已存在时加 (1)(2)... 后缀，避免覆盖
pub(crate) fn unique_download_path(dir: &str, filename: &str) -> String {
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

/// 判断 URL 是否为内部启动页（标题显示"启动页"）
/// 协议改版后新标签页 URL 是 shift://newtab（host=newtab），旧 file:// 路径
/// 含 /newtab/ 的判断匹配不到——必须按 scheme+host 判断，两处调用点（create_tab
/// 初始标题 + on_page_load 覆盖）共用同一逻辑，避免再次漂移
pub(crate) fn is_internal_newtab_url(url: &str) -> bool {
    match url.parse::<tauri::Url>() {
        Ok(u) => match u.scheme() {
            "shift" => matches!(u.host_str(), Some("newtab") | Some("worldnav")),
            "file" => url.contains(crate::contracts::urls::NEWTAB_MARKER) || url.contains(crate::contracts::urls::WORLD_NAV_FILE),
            _ => false,
        },
        Err(_) => false,
    }
}

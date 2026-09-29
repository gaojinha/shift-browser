//! GTK 布局、分屏几何与开发者工具门面（R2-1 拆分；R1 2026-09-25 外移 gtk 实现）
//!
//! child webview 的 reparent/事件/下载信号见 `layout/gtk.rs`。

use tauri::{AppHandle, Emitter, Manager};

use super::super::model::TabManager;
use crate::engine::layout::CONTENT_TOP_OFFSET;

#[cfg(target_os = "linux")]
use gtk::prelude::*;

#[cfg(target_os = "linux")]
#[path = "layout/gtk.rs"]
mod gtk_impl;

use super::{UI_FOCUSED, UI_OVERLAY_ACTIVE};

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
                    state.ai_panel_active()
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
                        let ids: Vec<String> = state.read_state().tabs.keys().cloned().collect();
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
                // 前端 matchShortcut 明确要求无 Alt/Meta（见 App.vue），GTK 层同口径：
                // Alt+Ctrl+T 这类组合键不转发，否则页面持焦与 UI 持焦行为不一致
                let alt_or_meta = state.contains(gtk::gdk::ModifierType::MOD1_MASK)
                    || state.contains(gtk::gdk::ModifierType::META_MASK)
                    || state.contains(gtk::gdk::ModifierType::SUPER_MASK)
                    || state.contains(gtk::gdk::ModifierType::HYPER_MASK);
                if alt_or_meta {
                    return gtk::glib::Propagation::Proceed;
                }

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

    // 主循环看门狗（诊断用 · 2026-09-29）：系统"无响应"框只在主循环数秒不转时出现，
    // 但它不说是哪卡的。这里每秒打一次点，恢复后若间隔 ≥5s 就记一条 warn，
    // 下次再卡直接看日志时间线，不用猜。只记日志，不做任何干预。
    gtk::glib::timeout_add_seconds_local(1, || {
        use std::sync::atomic::{AtomicU64, Ordering};
        static LAST_TICK: AtomicU64 = AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prev = LAST_TICK.swap(now, Ordering::Relaxed);
        if prev != 0 && now.saturating_sub(prev) >= 5 {
            log::warn!(
                "[watchdog] 主循环停顿约 {} 秒（系统可能已弹无响应框），前后日志即现场",
                now - prev
            );
        }
        gtk::glib::ControlFlow::Continue
    });

    Ok(())
}

/// 应用某个标签 webview 的内容区几何（按当前窗口尺寸重算）。
/// 用于"显示之后"重新分配：Fixed/webview 隐藏期间尺寸变更不会触发 GTK 重新分配，
/// 且 WebKit 内部输入窗口不跟随尺寸变化，表现为"页面能看到但点不动"。
/// 实现委托引擎层（engine/layout.rs）。
#[cfg(target_os = "linux")]
#[allow(dead_code)] // R2 渲染层去风险重构统一接线（几何修复入口已备好）
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
    let s = state.read_state();
    let ai_active = s.ai_panel_active;
    let active = s.active_id.clone();
    let split = s.split_tab.clone();

    let full = crate::engine::layout::ContentGeometry::compute(&window, top, ai_active, vertical);
    let ids: Vec<String> = s.tabs.keys().cloned().collect();
    drop(s);

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
        let id = state.read_state().active_id.clone();
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

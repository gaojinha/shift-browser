//! 窗口/布局/可见性命令（R2-1 拆分 · 2026-09-24）
//!
//! #[tauri::command] 从 tabs.rs 迁出，本模块负责窗口/布局/可见性；
//! 生命周期/导航/标签状态命令见同级 lifecycle_cmds/nav_cmds/tab_state_cmds。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

use super::super::*;
use crate::error::{AppError, CommandResult};
use tauri::Manager;

#[tauri::command]
pub async fn set_webview_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
    seq: u64,
    source: Option<String>,
) -> CommandResult<()> {
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
    // 覆盖层锁：隐藏 webview（visible=false）视为"有全屏覆盖层"，布局重排不得再显示，
    // 否则证书警告页会被 GTK 层 webview 盖掉；恢复显示时解锁。
    UI_OVERLAY_ACTIVE.store(!visible, std::sync::atomic::Ordering::Relaxed);
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
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
    // 不重设会导致 webview 尺寸或位置滞后，WebKit 输入窗口不跟随 → 页面点不动。
    // 注意：必须走 apply_split_layout（先全部隐藏、再只显示活动标签）——
    // 若直接对全部 webview show+apply，非活动标签会叠在活动标签上面（z-order），
    // 把点击事件吃掉 → 表现"设置弹窗关闭后页面按钮点不动"。
    if visible {
        apply_split_layout(&app, &state)?;
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
) -> CommandResult<()> {
    *state
        .ai_panel_active
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = active;
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let top = state.top_offset();
    let vertical = state.vertical_tabs();
    let geo = crate::engine::layout::ContentGeometry::compute(&window, top, active, vertical);
    log::info!(
        "[tabs] set_webview_ai_panel: active={} content={}x{}",
        active,
        geo.width,
        geo.height
    );
    // 关键：不能只对每个 webview apply_to_webview 改尺寸——WebKit 输入窗口
    // 不跟随 widget 尺寸变化（已知问题，见 apply_to_webview 注释），AI 面板打开时
    // 右侧 380px 仍是旧输入窗口的命中区，点击被 webview 吞掉 → ✕ 点不动。
    // 统一走 apply_split_layout（先全部隐藏、再按活动标签/分屏重新应用几何并显示），
    // hide→show 强制 GTK 重新分配，输入窗口跟随新尺寸，右侧让给 HTML 层 AI 面板；
    // 关闭时同理：hide→show 让 webview 输入窗口恢复全宽（避免关面板后点不动）。
    apply_split_layout(&app, &state)?;
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
pub async fn set_bookmarks_bar_visible(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    visible: bool,
) -> CommandResult<f64> {
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
) -> CommandResult<f64> {
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
) -> CommandResult<()> {
    if !state
        .tabs
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains_key(&id)
    {
        return Err(AppError::validation("标签不存在").into());
    }
    state.set_split_tab(Some(id.clone()));
    log::info!("[tabs] set_split_tab: {}", id);
    apply_split_layout(&app, &state).map_err(crate::error::CommandError::from)
}

/// 关闭分屏

#[tauri::command]
pub async fn clear_split(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
) -> CommandResult<()> {
    state.set_split_tab(None);
    log::info!("[tabs] clear_split");
    apply_split_layout(&app, &state).map_err(crate::error::CommandError::from)
}

/// 查询当前分屏标签（None = 未分屏）

#[tauri::command]
pub async fn get_split_tab(state: tauri::State<'_, TabManager>) -> CommandResult<Option<String>> {
    Ok(state.split_tab())
}

/// 打开当前活动标签的开发者工具（Web Inspector）。
/// 依赖创建 webview 时设置的 `Settings.enable-developer-extras`（本文件 create 流程中已开启）。
/// 快捷键入口在 GTK 窗口层统一拦截（F12 / Ctrl+Shift+I），见 `setup_gtk_layout`：
/// 页面（child webview）持有焦点时 HTML 层收不到按键，必须在 GTK 层兜底。

#[tauri::command]
pub async fn open_devtools(
    app: AppHandle,
    _state: tauri::State<'_, TabManager>,
) -> CommandResult<()> {
    open_devtools_for_active(&app);
    Ok(())
}

/// 固定/取消固定标签（原版 tab-pin IPC）

#[tauri::command]
pub async fn toggle_maximize(app: AppHandle) -> CommandResult<bool> {
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    let maximized = window.is_maximized().unwrap_or(false);
    if maximized {
        window
            .unmaximize()
            .map_err(|e| AppError::internal(e.to_string()))?;
    } else {
        window
            .maximize()
            .map_err(|e| AppError::internal(e.to_string()))?;
    }
    Ok(!maximized)
}

/// 最小化主窗口（标题栏/标签栏合并后的窗口控制按钮）

#[tauri::command]
pub async fn minimize_window(app: AppHandle) -> CommandResult<()> {
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    window
        .minimize()
        .map_err(|e| AppError::internal(e.to_string()).into())
}

/// 关闭主窗口（标题栏/标签栏合并后的窗口控制按钮）

#[tauri::command]
pub async fn close_window(app: AppHandle) -> CommandResult<()> {
    // 前端「退出」按钮是明确要退出，先置真退出标志，
    // 否则会被「关窗=缩托盘」的 delete-event 拦截，变成只隐藏。
    crate::tray::REAL_QUIT.store(true, std::sync::atomic::Ordering::SeqCst);
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    window
        .close()
        .map_err(|e| AppError::internal(e.to_string()).into())
}

/// 开始拖动主窗口（无边框窗口标题栏拖拽，标签栏空白处按住调用）

#[tauri::command]
pub async fn start_drag_window(app: AppHandle) -> CommandResult<()> {
    let window = app
        .get_window("main")
        .ok_or_else(|| AppError::internal("主窗口不存在"))?;
    window
        .start_dragging()
        .map_err(|e| AppError::internal(e.to_string()).into())
}




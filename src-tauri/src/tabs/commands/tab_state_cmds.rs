//! 标签状态命令（R1 二次拆分 · 2026-09-24）：
//! 拖拽排序 / 列表同步 / 元数据更新 / 固定 / 静音 / 清除浏览数据。

use super::super::*;
use crate::error::CommandResult;
use tauri::Manager;

/// 更新标签顺序（拖拽排序）；前端传新顺序，缺失的现存标签兜底追加到末尾
#[tauri::command]
pub async fn reorder_tabs(
    state: tauri::State<'_, TabManager>,
    ids: Vec<String>,
) -> CommandResult<()> {
    // 读锁内一次取全（状态收敛实施）
    let new_order: Vec<String> = state.mutate(|s| {
        let mut new_order: Vec<String> = ids
            .iter()
            .filter(|id| s.tabs.contains_key(*id))
            .cloned()
            .collect();
        for id in s.order.iter() {
            if !new_order.contains(id) {
                new_order.push(id.clone());
            }
        }
        s.order = new_order.clone();
        new_order
    });
    log::info!("[tabs] reorder_tabs: 新顺序 {:?}", new_order);
    Ok(())
}

/// 获取所有标签页（按顺序）
#[tauri::command]
pub async fn get_tabs(state: tauri::State<'_, TabManager>) -> CommandResult<Vec<Tab>> {
    let tabs = state.all_tabs();
    // 诊断"前端标签列表为空/标签不显示"问题：记录每次同步的标签数（事件驱动 + 8s 轮询）
    log::info!("[tabs] get_tabs: {} 个标签", tabs.len());
    Ok(tabs)
}

/// 获取标签状态版本（前端差分同步用：版本未变跳过全量同步）
#[tauri::command]
pub async fn get_tabs_version(state: tauri::State<'_, TabManager>) -> CommandResult<u64> {
    Ok(state.snapshot_version())
}

/// 更新标签页元数据（标题/图标等，供前端同步）
#[tauri::command]
pub async fn update_tab(
    state: tauri::State<'_, TabManager>,
    id: String,
    title: Option<String>,
    favicon: Option<String>,
) -> CommandResult<Option<Tab>> {
    Ok(state.mutate(|s| {
        s.tabs.get_mut(&id).map(|t| {
            if let Some(new_title) = title {
                t.title = new_title;
            }
            if let Some(new_favicon) = favicon {
                t.favicon = new_favicon;
            }
            t.clone()
        })
    }))
}

#[tauri::command]
pub async fn toggle_pin(
    state: tauri::State<'_, TabManager>,
    id: String,
) -> CommandResult<Option<Tab>> {
    Ok(state.mutate(|s| {
        s.tabs.get_mut(&id).map(|t| {
            t.is_pinned = !t.is_pinned;
            log::info!("[tabs] toggle_pin: id={} pinned={}", id, t.is_pinned);
            t.clone()
        })
    }))
}

/// 静音/取消静音标签。
/// 真实现：翻转内存状态 + 异步派发到主线程调 WebKit `set_is_muted`
/// （WebKitGTK 2.36+ 支持 per-webview 静音，绑定里 `WebViewExt::set_is_muted` 存在）。
#[tauri::command]
pub async fn toggle_mute(
    app: AppHandle,
    state: tauri::State<'_, TabManager>,
    id: String,
) -> CommandResult<Option<Tab>> {
    let result = state.mutate(|s| {
        s.tabs.get_mut(&id).map(|t| {
            t.is_muted = !t.is_muted;
            log::info!("[tabs] toggle_mute: id={} muted={}", id, t.is_muted);
            t.clone()
        })
    });
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

#[tauri::command]
pub async fn clear_browsing_data(
    app: AppHandle,
    db: tauri::State<'_, crate::db::Db>,
    scopes: Vec<String>,
) -> CommandResult<Vec<String>> {
    let has = |k: &str| scopes.iter().any(|s| s == k || s == "all");
    let mut done: Vec<String> = Vec::new();

    // ---- 数据库侧 ----
    if has("history") {
        let _ = sqlx::query("DELETE FROM history").execute(&db.pool).await;
        done.push("history".into());
    }
    if has("downloads") {
        let _ = sqlx::query("DELETE FROM downloads").execute(&db.pool).await;
        done.push("downloads".into());
    }
    if has("sessions") {
        let _ = sqlx::query("DELETE FROM sessions").execute(&db.pool).await;
        done.push("sessions".into());
    }

    // ---- WebKit 侧（GTK 对象只能在主线程操作）----
    let need_cookies = has("cookies");
    let need_cache = has("cache");
    let need_storage = has("storage");
    if need_cookies || need_cache || need_storage {
        app.run_on_main_thread(move || {
            // P0-2：WebKit 数据清理下沉到 engine::platform
            crate::engine::platform::clear_web_data(need_cookies, need_cache, need_storage);
            log::info!(
                "[privacy] 清除浏览数据: cookies={} cache={} storage={}",
                need_cookies,
                need_cache,
                need_storage
            );
        })
        .map_err(|e| e.to_string())?;
        if need_cookies {
            done.push("cookies".into());
        }
        if need_cache {
            done.push("cache".into());
        }
        if need_storage {
            done.push("storage".into());
        }
    }

    log::info!("[privacy] 清除浏览数据完成: {:?}", done);
    Ok(done)
}

//! 标签状态命令（R1 二次拆分 · 2026-09-24）：
//! 拖拽排序 / 列表同步 / 元数据更新 / 固定 / 静音 / 清除浏览数据。

use super::super::*;
use crate::error::{AppError, CommandResult};
use tauri::Manager;

/// 拖拽排序合并（纯函数，便于测试）：前端新顺序只保留现存标签，
/// 现存但前端未报的标签兜底追加到末尾（前端漏报不丢标签）。
pub(crate) fn merged_tab_order(
    ids: &[String],
    existing: &std::collections::HashSet<String>,
    current_order: &[String],
) -> Vec<String> {
    let mut new_order: Vec<String> = ids
        .iter()
        .filter(|id| existing.contains(*id))
        .cloned()
        .collect();
    for id in current_order {
        if !new_order.contains(id) {
            new_order.push(id.clone());
        }
    }
    new_order
}

/// 更新标签顺序（拖拽排序）；前端传新顺序，缺失的现存标签兜底追加到末尾
#[tauri::command]
pub async fn reorder_tabs(
    state: tauri::State<'_, TabManager>,
    ids: Vec<String>,
) -> CommandResult<()> {
    // 读锁内一次取全（状态收敛实施）
    let new_order: Vec<String> = state.mutate(|s| {
        let existing: std::collections::HashSet<_> = s.tabs.keys().cloned().collect();
        let merged = merged_tab_order(&ids, &existing, &s.order);
        s.order = merged.clone();
        merged
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
            crate::log_if_err!("webview", wv.with_webview(move |pwv| {
                crate::engine::webview::set_muted(&pwv.inner(), muted);
            }));
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
    // ---- 数据库侧（吃池，可测；scope 匹配口径见 clear_browsing_data_db）----
    // 失败如实上抛：不能把"未真正清掉的数据"当成功回报（对齐 feed.rs 的既有口径）
    let done = clear_browsing_data_db(&db.pool, &scopes).await?;

    // ---- WebKit 侧（GTK 对象只能在主线程操作）----
    let has = |k: &str| scopes.iter().any(|s| s == k || s == "all");
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
        let mut done = done;
        if need_cookies {
            done.push("cookies".into());
        }
        if need_cache {
            done.push("cache".into());
        }
        if need_storage {
            done.push("storage".into());
        }
        log::info!("[privacy] 清除浏览数据完成: {:?}", done);
        return Ok(done);
    }

    log::info!("[privacy] 清除浏览数据完成: {:?}", done);
    Ok(done)
}

/// 清除浏览数据的 DB 侧（吃连接池，便于测试）：history/downloads/sessions 三表
/// 按 scopes 清空（"all" 等价全部 scope），返回实际执行过的 scope 清单。
///
/// 任一表删除失败即 `Err` 上抛——**不允许**把失败静默记入 done 后仍汇报"已清除"
/// （F-79 D1：用户可见的行为不一致）。
async fn clear_browsing_data_db(
    pool: &sqlx::SqlitePool,
    scopes: &[String],
) -> Result<Vec<String>, AppError> {
    let has = |k: &str| scopes.iter().any(|s| s == k || s == "all");
    let mut done: Vec<String> = Vec::new();

    if has("history") {
        sqlx::query("DELETE FROM history")
            .execute(pool)
            .await
            .map_err(|e| AppError::db(format!("清除浏览历史失败: {}", e)))?;
        done.push("history".into());
    }
    if has("downloads") {
        sqlx::query("DELETE FROM downloads")
            .execute(pool)
            .await
            .map_err(|e| AppError::db(format!("清除下载记录失败: {}", e)))?;
        done.push("downloads".into());
    }
    if has("sessions") {
        sqlx::query("DELETE FROM sessions")
            .execute(pool)
            .await
            .map_err(|e| AppError::db(format!("清除会话记录失败: {}", e)))?;
        done.push("sessions".into());
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（history/downloads/sessions 表齐全）
    async fn temp_db() -> (tempfile::TempDir, crate::db::Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    /// 三表各造一行数据
    async fn seed_all(pool: &sqlx::SqlitePool) {
        sqlx::query("INSERT INTO history (url, title, visit_count, last_visit_at) VALUES ('https://a.com', 'A', 1, datetime('now'))")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO downloads (filename, url, path, total_bytes, received_bytes) VALUES ('f.zip', 'https://a.com/f.zip', '/tmp/f.zip', 1, 1)")
            .execute(pool).await.unwrap();
        sqlx::query("INSERT INTO sessions (name, data) VALUES ('会话', '{}')")
            .execute(pool).await.unwrap();
    }

    async fn count(pool: &sqlx::SqlitePool, table: &str) -> i64 {
        sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[test]
    fn merged_order_filters_missing_and_appends_unreported() {
        // existing = 现存标签（ghost 不在其中）；current = 当前顺序
        let existing: std::collections::HashSet<_> =
            ["a", "b", "c"].into_iter().map(String::from).collect();
        let current = vec!["a".to_string(), "b".to_string(), "c".to_string()];

        // 前端报的顺序里 ghost 不存在 → 过滤；c 前端漏报 → 兜底追加末尾（不丢标签）
        let ids = vec!["b".to_string(), "ghost".to_string(), "a".to_string()];
        assert_eq!(
            merged_tab_order(&ids, &existing, &current),
            vec!["b".to_string(), "a".to_string(), "c".to_string()]
        );
        // 前端报空 → 原顺序原样保留
        assert_eq!(
            merged_tab_order(&[], &existing, &current),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
        // 顺序完全一致 + 无漏报 → 原样
        let ids: Vec<String> = current.clone();
        assert_eq!(merged_tab_order(&ids, &existing, &current), current);
    }

    #[tokio::test]
    async fn clear_db_side_by_scopes() {
        let (_dir, db) = temp_db().await;
        seed_all(&db.pool).await;

        // 单 scope：只清 history，其余两表不动
        let done = clear_browsing_data_db(&db.pool, &["history".to_string()]).await.unwrap();
        assert_eq!(done, vec!["history".to_string()]);
        assert_eq!(count(&db.pool, "history").await, 0);
        assert_eq!(count(&db.pool, "downloads").await, 1);
        assert_eq!(count(&db.pool, "sessions").await, 1);

        // "all" 等价全部 DB scope：downloads/sessions 一并清空
        let done = clear_browsing_data_db(&db.pool, &["all".to_string()]).await.unwrap();
        assert_eq!(
            done,
            vec!["history".to_string(), "downloads".to_string(), "sessions".to_string()]
        );
        assert_eq!(count(&db.pool, "downloads").await, 0);
        assert_eq!(count(&db.pool, "sessions").await, 0);
    }

    #[tokio::test]
    async fn clear_db_side_noop_on_unrelated_scope() {
        let (_dir, db) = temp_db().await;
        seed_all(&db.pool).await;
        // 只要求 WebKit 侧 scope：DB 侧零动作，三表数据原样
        let done = clear_browsing_data_db(&db.pool, &["cookies".to_string(), "cache".to_string()]).await.unwrap();
        assert!(done.is_empty());
        assert_eq!(count(&db.pool, "history").await, 1);
        assert_eq!(count(&db.pool, "downloads").await, 1);
        assert_eq!(count(&db.pool, "sessions").await, 1);
    }
}

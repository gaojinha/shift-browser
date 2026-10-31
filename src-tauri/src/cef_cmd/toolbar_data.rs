//! 工具栏页面所需的数据快照（书签 / 下载），从 `cef_cmd.rs` 拆出。
//!
//! 为什么同步读：CEF 侧异步推送实测不可靠（glib 定时器在回调链里不触发），
//! 改成生成工具栏页面时同步查一次——阻塞几十毫秒可接受，换来确定性。

use tauri::{AppHandle, Manager};

/// 同步读书签（生成工具栏页面用）。
pub fn cef_bookmarks_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "{\"folders\":[],\"bookmarks\":[]}".to_string();
    };
    let pool = db.pool.clone();
    let data = tauri::async_runtime::block_on(async move {
        let bms = sqlx::query_as::<_, (i64, String, String, Option<i64>)>(
            "SELECT id, title, url, folder_id FROM bookmarks ORDER BY sort_order, id LIMIT 300",
        )
        .fetch_all(&pool)
        .await?;
        let folders = sqlx::query_as::<_, (i64, String, Option<i64>)>(
            "SELECT id, name, parent_id FROM bookmark_folders ORDER BY sort_order, id",
        )
        .fetch_all(&pool)
        .await?;
        Ok::<_, sqlx::Error>((bms, folders))
    });
    match data {
        Ok((bms, folders)) => {
            log::info!(
                "[cef] 书签同步读取 {} 条 + {} 个文件夹",
                bms.len(),
                folders.len()
            );
            let bm_json: Vec<serde_json::Value> = bms
                .iter()
                .map(|(id, t, u, f)| {
                    serde_json::json!({ "id": id, "title": t, "url": u, "folderId": f })
                })
                .collect();
            let f_json: Vec<serde_json::Value> = folders
                .iter()
                .map(|(id, n, p)| serde_json::json!({ "id": id, "name": n, "parentId": p }))
                .collect();
            serde_json::json!({ "folders": f_json, "bookmarks": bm_json }).to_string()
        }
        Err(e) => {
            log::warn!("[cef] 读书签失败: {e}");
            "{\"folders\":[],\"bookmarks\":[]}".to_string()
        }
    }
}

/// 读下载记录给工具栏下载面板（启动时嵌进页面；后续由 shell 侧推送刷新）。
pub fn cef_downloads_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    let pool = db.pool.clone();
    match tauri::async_runtime::block_on(async move {
        crate::db::downloads::list_downloads(&pool).await
    }) {
        Ok(list) => crate::engine::cef::shell::downloads_panel_json(&list),
        Err(e) => {
            log::warn!("[cef] 读下载列表失败: {e}");
            "[]".to_string()
        }
    }
}

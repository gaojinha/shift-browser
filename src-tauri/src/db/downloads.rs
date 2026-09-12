// 下载记录管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Download {
    pub id: i64,
    pub filename: String,
    pub url: Option<String>,
    pub path: Option<String>,
    pub total_bytes: i64,
    pub received_bytes: i64,
    /// in_progress / completed / failed / cancelled
    pub state: String,
    pub created_at: String,
}

/// 记录下载开始
#[tauri::command]
pub async fn create_download(
    db: tauri::State<'_, Db>,
    filename: String,
    url: Option<String>,
    path: Option<String>,
    total_bytes: i64,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO downloads (filename, url, path, total_bytes, received_bytes, state) VALUES (?, ?, ?, ?, 0, 'in_progress')",
    )
    .bind(&filename)
    .bind(&url)
    .bind(&path)
    .bind(total_bytes)
    .execute(&db.pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 更新下载进度
#[tauri::command]
pub async fn update_download_progress(
    db: tauri::State<'_, Db>,
    id: i64,
    received_bytes: i64,
    state: Option<String>,
) -> DbResult<()> {
    if let Some(state) = state {
        sqlx::query("UPDATE downloads SET received_bytes = ?, state = ? WHERE id = ?")
            .bind(received_bytes)
            .bind(state)
            .bind(id)
            .execute(&db.pool)
            .await?;
    } else {
        sqlx::query("UPDATE downloads SET received_bytes = ? WHERE id = ?")
            .bind(received_bytes)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    Ok(())
}

/// 获取所有下载记录
#[tauri::command]
pub async fn get_downloads(
    db: tauri::State<'_, Db>,
) -> DbResult<Vec<Download>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, i64, i64, String, String)>(
        "SELECT id, filename, url, path, total_bytes, received_bytes, state, created_at FROM downloads ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&db.pool)
    .await?;

    println!("[downloads] get_downloads: {} 条", rows.len());

    Ok(rows
        .into_iter()
        .map(|(id, filename, url, path, total_bytes, received_bytes, state, created_at)| Download {
            id,
            filename,
            url,
            path,
            total_bytes,
            received_bytes,
            state,
            created_at,
        })
        .collect())
}

/// 删除下载记录
#[tauri::command]
pub async fn delete_download(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    println!("[downloads] delete_download: id={}", id);
    sqlx::query("DELETE FROM downloads WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}
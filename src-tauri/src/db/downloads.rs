// 下载记录管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

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

/// 插入下载记录（内部共用：WebKit 下载信号与 create_download 命令走同一入口，避免 SQL 双写）
/// 插入后顺带清理：只保留最近 500 条记录，防止下载历史无限增长
pub async fn insert_download(
    pool: &SqlitePool,
    filename: &str,
    url: Option<&str>,
    path: Option<&str>,
    total_bytes: i64,
) -> Result<i64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO downloads (filename, url, path, total_bytes, received_bytes, state) VALUES (?, ?, ?, ?, 0, 'in_progress')",
    )
    .bind(filename)
    .bind(url)
    .bind(path)
    .bind(total_bytes)
    .execute(pool)
    .await?;
    // 清理旧记录：保留最近 500 条（与 sessions 保留 100、history 上限 10000 同思路）
    sqlx::query(
        "DELETE FROM downloads WHERE id NOT IN (SELECT id FROM downloads ORDER BY id DESC LIMIT 500)",
    )
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 更新下载进度（内部共用；下载开始时 total 可能未知为 0，进度回调时补全）
pub async fn update_download_bytes(
    pool: &SqlitePool,
    id: i64,
    received_bytes: i64,
    total_bytes: Option<i64>,
) -> Result<(), sqlx::Error> {
    if let Some(total) = total_bytes {
        sqlx::query(
            "UPDATE downloads SET received_bytes = ?, total_bytes = CASE WHEN total_bytes = 0 THEN ? ELSE total_bytes END WHERE id = ?",
        )
        .bind(received_bytes)
        .bind(total)
        .bind(id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query("UPDATE downloads SET received_bytes = ? WHERE id = ?")
            .bind(received_bytes)
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// 更新下载状态（可同时更新已接收字节数，完成/失败回调共用）
pub async fn update_download_state(
    pool: &SqlitePool,
    id: i64,
    state: &str,
    received_bytes: Option<i64>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE downloads SET state = ?, received_bytes = COALESCE(?, received_bytes) WHERE id = ?",
    )
    .bind(state)
    .bind(received_bytes)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
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
    let id = insert_download(&db.pool, &filename, url.as_deref(), path.as_deref(), total_bytes).await?;
    Ok(id)
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

    log::info!("[downloads] get_downloads: {} 条", rows.len());

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
    log::info!("[downloads] delete_download: id={}", id);
    sqlx::query("DELETE FROM downloads WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

  /// 建临时数据库（含 downloads 表）
  async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.db");
    let options = SqliteConnectOptions::new()
      .filename(&db_path)
      .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
      .max_connections(1)
      .connect_with(options)
      .await
      .unwrap();
    sqlx::query(
      "CREATE TABLE downloads (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        filename TEXT NOT NULL,
        url TEXT,
        path TEXT,
        total_bytes INTEGER DEFAULT 0,
        received_bytes INTEGER DEFAULT 0,
        state TEXT DEFAULT 'in_progress',
        created_at TEXT DEFAULT (datetime('now'))
      )",
    )
    .execute(&pool)
    .await
    .unwrap();
    (dir, pool)
  }

  #[tokio::test]
  async fn test_downloads_cleanup_keeps_latest_500() {
    let (_dir, pool) = temp_pool().await;
    // 插入 505 条记录，验证只保留最近 500 条
    for i in 0..505 {
      insert_download(&pool, &format!("file{}.bin", i), None, None, 100).await.unwrap();
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads")
      .fetch_one(&pool)
      .await
      .unwrap();
    assert_eq!(count, 500, "超过 500 条应被清理");

    // 保留的是最新的（id 最大的 500 条）：最早的 5 条应被删除
    let oldest: i64 = sqlx::query_scalar("SELECT MIN(id) FROM downloads")
      .fetch_one(&pool)
      .await
      .unwrap();
    assert_eq!(oldest, 6, "id 1..5 应被清理，最旧保留 id=6");
  }
}
// 下载记录管理
use ts_rs::TS;
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Download {
    #[ts(type = "number")]
    pub id: i64,
    pub filename: String,
    pub url: Option<String>,
    pub path: Option<String>,
    #[ts(type = "number")]
    pub total_bytes: i64,
    #[ts(type = "number")]
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
    // P2：INSERT 与清理放同一事务——清理失败整体回滚，避免"插入已提交却上报失败"。
    // 清理排除 in_progress 活动记录：一次性排队超过 500 个下载时，
    // 不能把仍在进行的下载剪掉（否则后续进度/完成回调 UPDATE 0 行静默丢失）
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "INSERT INTO downloads (filename, url, path, total_bytes, received_bytes, state) VALUES (?, ?, ?, ?, 0, 'in_progress')",
    )
    .bind(filename)
    .bind(url)
    .bind(path)
    .bind(total_bytes)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM downloads WHERE state != 'in_progress' AND id NOT IN (
           SELECT id FROM downloads WHERE state != 'in_progress' ORDER BY id DESC LIMIT 500
         )",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
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
    let id = insert_download(
        &db.pool,
        &filename,
        url.as_deref(),
        path.as_deref(),
        total_bytes,
    )
    .await?;
    Ok(id)
}

/// 更新下载进度
/// total_bytes：可选——创建时 total 未知（0）时，进度回调可补全真实大小
#[tauri::command]
pub async fn update_download_progress(
    db: tauri::State<'_, Db>,
    id: i64,
    received_bytes: i64,
    state: Option<String>,
    total_bytes: Option<i64>,
) -> DbResult<()> {
    let res = if let Some(total) = total_bytes {
        sqlx::query(
            "UPDATE downloads SET received_bytes = ?, state = COALESCE(?, state),
             total_bytes = CASE WHEN total_bytes = 0 THEN ? ELSE total_bytes END WHERE id = ?",
        )
        .bind(received_bytes)
        .bind(state)
        .bind(total)
        .bind(id)
        .execute(&db.pool)
        .await?
    } else {
        sqlx::query(
            "UPDATE downloads SET received_bytes = ?, state = COALESCE(?, state) WHERE id = ?",
        )
        .bind(received_bytes)
        .bind(state)
        .bind(id)
        .execute(&db.pool)
        .await?
    };
    // 假成功防护：id 不存在时如实上报（活动记录被裁剪等场景不再静默吞错）
    if res.rows_affected() == 0 {
        return Err(crate::db::DbError::Other(format!("下载记录 {} 不存在", id)));
    }
    Ok(())
}

/// 获取所有下载记录
#[tauri::command]
pub async fn get_downloads(db: tauri::State<'_, Db>) -> DbResult<Vec<Download>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, i64, i64, String, String)>(
        "SELECT id, filename, url, path, total_bytes, received_bytes, state, created_at FROM downloads ORDER BY id DESC LIMIT 100",
    )
    .fetch_all(&db.pool)
    .await?;

    log::info!("[downloads] get_downloads: {} 条", rows.len());

    Ok(rows
        .into_iter()
        .map(
            |(id, filename, url, path, total_bytes, received_bytes, state, created_at)| Download {
                id,
                filename,
                url,
                path,
                total_bytes,
                received_bytes,
                state,
                created_at,
            },
        )
        .collect())
}

/// 删除下载记录
#[tauri::command]
pub async fn delete_download(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    log::info!("[downloads] delete_download: id={}", id);
    let res = sqlx::query("DELETE FROM downloads WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(crate::db::DbError::Other(format!("下载记录 {} 不存在", id)));
    }
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
        // 插入 505 条记录（in_progress），全部标记为 completed（可清理的非活动历史）
        for i in 0..505 {
            insert_download(&pool, &format!("file{}.bin", i), None, None, 100)
                .await
                .unwrap();
        }
        sqlx::query("UPDATE downloads SET state = 'completed'")
            .execute(&pool)
            .await
            .unwrap();
        // 再插入一条 in_progress 触发清理：非活动保留最近 500 条，id 1..5 被删
        insert_download(&pool, "trigger.bin", None, None, 100)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 501,
            "5 条旧记录被清理，500 条历史 + 1 条新插入 = 501"
        );

        // 保留的是最新的（id 最大的 500 条）：最早的 5 条应被删除
        let oldest: i64 = sqlx::query_scalar("SELECT MIN(id) FROM downloads")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(oldest, 6, "id 1..5 应被清理，最旧保留 id=6");
    }

    #[tokio::test]
    async fn test_downloads_cleanup_keeps_in_progress() {
        let (_dir, pool) = temp_pool().await;
        // 插入 505 条 in_progress（活动记录），不应被清理裁剪
        for i in 0..505 {
            insert_download(&pool, &format!("file{}.bin", i), None, None, 100)
                .await
                .unwrap();
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM downloads")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            count, 505,
            "活动记录不能被清理裁剪（否则进度回调会静默丢失）"
        );
    }
}

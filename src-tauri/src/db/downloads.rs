// 下载记录管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use ts_rs::TS;

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
    /// in_progress / completed / failed / cancelled / interrupted（中断：进程退出时未完成）
    pub state: String,
    pub created_at: String,
}

/// 合法下载状态白名单。sweep 只认 in_progress，写脏状态会导致记录永不被扫尾/统计，
/// 因此两个状态入口（内部回调与前端命令）统一校验，非法直接拒。
pub const VALID_STATES: &[&str] = &[
    "in_progress",
    "completed",
    "failed",
    "cancelled",
    "interrupted",
];

fn check_state(state: &str) -> Result<(), crate::db::DbError> {
    if VALID_STATES.contains(&state) {
        Ok(())
    } else {
        Err(crate::db::DbError::Other(format!(
            "非法下载状态: {}（仅允许 {:?}）",
            state, VALID_STATES
        )))
    }
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
) -> DbResult<()> {
    check_state(state)?;
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
    // 脏状态防护：白名单外直接拒，避免记录永不被扫尾/统计
    if let Some(ref s) = state {
        check_state(s)?;
    }
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

/// 启动扫尾：上次进程退出时仍是 in_progress 的记录，其 WebKit 下载对象已随进程消失，
/// 进度回调再也不会来。全部置为 interrupted，避免重启后永久显示“下载中”僵尸。
/// 返回本次扫尾的条数（0 = 无残留）。在 Db 就绪后、锁屏前调用一次即可。
pub async fn sweep_interrupted_downloads(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let res = sqlx::query("UPDATE downloads SET state = 'interrupted' WHERE state = 'in_progress'")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

/// 获取所有下载记录（内部共用：Tauri 命令与 CEF 工具栏面板走同一入口）
pub async fn list_downloads(pool: &SqlitePool) -> Result<Vec<Download>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, i64, i64, String, String)>(
        "SELECT id, filename, url, path, total_bytes, received_bytes, state, created_at FROM downloads ORDER BY id DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;

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

/// 获取所有下载记录
#[tauri::command]
pub async fn get_downloads(db: tauri::State<'_, Db>) -> DbResult<Vec<Download>> {
    let list = list_downloads(&db.pool).await?;
    log::info!("[downloads] get_downloads: {} 条", list.len());
    Ok(list)
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

/// 清空已结束（非 in_progress）的下载记录，返回删除行数。
/// in_progress 活动记录不碰——同 insert_download 的清理排除口径。
/// CEF 面板 dlclear 用；默认构建无调用方，feature 条件下允许 dead_code。
#[cfg_attr(not(feature = "cef-engine"), allow(dead_code))]
pub async fn clear_finished(pool: &SqlitePool) -> DbResult<u64> {
    let res = sqlx::query("DELETE FROM downloads WHERE state != 'in_progress'")
        .execute(pool)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
#[path = "downloads_tests.rs"]
mod tests;

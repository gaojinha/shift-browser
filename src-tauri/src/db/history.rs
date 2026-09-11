// 历史记录管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HistoryEntry {
  pub id: i64,
  pub url: String,
  pub title: Option<String>,
  pub visit_count: i64,
  pub last_visit_at: String,
}

/// 记录一次访问
#[tauri::command]
pub async fn record_history(
  db: tauri::State<'_, Db>,
  url: String,
  title: Option<String>,
) -> DbResult<()> {
  // 已存在则更新计数，否则插入
  let existing: Option<(i64,)> =
    sqlx::query_as("SELECT id FROM history WHERE url = ?")
      .bind(&url)
      .fetch_optional(&db.pool)
      .await?;

  if let Some((id,)) = existing {
    sqlx::query(
      "UPDATE history SET visit_count = visit_count + 1, last_visit_at = datetime('now'),
       title = COALESCE(?, title) WHERE id = ?",
    )
    .bind(&title)
    .bind(id)
    .execute(&db.pool)
    .await?;
  } else {
    sqlx::query("INSERT INTO history (url, title) VALUES (?, ?)")
      .bind(&url)
      .bind(&title)
      .execute(&db.pool)
      .await?;
  }

  // 清理超过 10000 条的旧记录
  sqlx::query(
    "DELETE FROM history WHERE id IN (
      SELECT id FROM history ORDER BY last_visit_at DESC LIMIT -1 OFFSET 10000
    )",
  )
  .execute(&db.pool)
  .await?;

  Ok(())
}

/// 获取最近历史
#[tauri::command]
pub async fn get_recent_history(
  db: tauri::State<'_, Db>,
  limit: Option<i64>,
) -> DbResult<Vec<HistoryEntry>> {
  let limit = limit.unwrap_or(100).min(1000);
  let rows = sqlx::query_as::<_, (i64, String, Option<String>, i64, String)>(
    "SELECT id, url, title, visit_count, last_visit_at FROM history
     ORDER BY last_visit_at DESC LIMIT ?",
  )
  .bind(limit)
  .fetch_all(&db.pool)
  .await?;

  Ok(rows
    .into_iter()
    .map(|(id, url, title, visit_count, last_visit_at)| HistoryEntry {
      id,
      url,
      title,
      visit_count,
      last_visit_at,
    })
    .collect())
}

/// 搜索历史
#[tauri::command]
pub async fn search_history(
  db: tauri::State<'_, Db>,
  query: String,
  limit: Option<i64>,
) -> DbResult<Vec<HistoryEntry>> {
  let limit = limit.unwrap_or(50).min(500);
  let like = format!("%{}%", query);
  let rows = sqlx::query_as::<_, (i64, String, Option<String>, i64, String)>(
    "SELECT id, url, title, visit_count, last_visit_at FROM history
     WHERE url LIKE ? OR title LIKE ? ORDER BY last_visit_at DESC LIMIT ?",
  )
  .bind(&like)
  .bind(&like)
  .bind(limit)
  .fetch_all(&db.pool)
  .await?;

  Ok(rows
    .into_iter()
    .map(|(id, url, title, visit_count, last_visit_at)| HistoryEntry {
      id,
      url,
      title,
      visit_count,
      last_visit_at,
    })
    .collect())
}

/// 获取访问最多的网站（新标签页快速访问用）
#[tauri::command]
pub async fn get_most_visited(
  db: tauri::State<'_, Db>,
  limit: Option<i64>,
) -> DbResult<Vec<HistoryEntry>> {
  let limit = limit.unwrap_or(8).min(50);
  let rows = sqlx::query_as::<_, (i64, String, Option<String>, i64, String)>(
    "SELECT id, url, title, visit_count, last_visit_at FROM history
     ORDER BY visit_count DESC, last_visit_at DESC LIMIT ?",
  )
  .bind(limit)
  .fetch_all(&db.pool)
  .await?;

  Ok(rows
    .into_iter()
    .map(|(id, url, title, visit_count, last_visit_at)| HistoryEntry {
      id,
      url,
      title,
      visit_count,
      last_visit_at,
    })
    .collect())
}

/// 清除全部历史
#[tauri::command]
pub async fn clear_history(db: tauri::State<'_, Db>) -> DbResult<()> {
  sqlx::query("DELETE FROM history")
    .execute(&db.pool)
    .await?;
  Ok(())
}

/// 按时间范围清除历史
#[tauri::command]
pub async fn clear_history_before(db: tauri::State<'_, Db>, before: String) -> DbResult<()> {
  sqlx::query("DELETE FROM history WHERE last_visit_at < ?")
    .bind(before)
    .execute(&db.pool)
    .await?;
  Ok(())
}

/// 删除单条历史
#[tauri::command]
pub async fn delete_history_entry(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
  sqlx::query("DELETE FROM history WHERE id = ?")
    .bind(id)
    .execute(&db.pool)
    .await?;
  Ok(())
}
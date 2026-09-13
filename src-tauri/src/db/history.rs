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
  // UPSERT 原子化：多标签并发加载同一 URL 时不会重复插入（url 有唯一索引）
  sqlx::query(
    "INSERT INTO history (url, title, visit_count, last_visit_at) VALUES (?, ?, 1, datetime('now'))
     ON CONFLICT(url) DO UPDATE SET
       visit_count = visit_count + 1,
       last_visit_at = datetime('now'),
       title = COALESCE(excluded.title, title)",
  )
  .bind(&url)
  .bind(&title)
  .execute(&db.pool)
  .await?;

  // 超过 10000 条才清理旧记录（避免每次访问都跑全表 DELETE）
  let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history")
    .fetch_one(&db.pool)
    .await?;
  if count > 10_000 {
    sqlx::query(
      "DELETE FROM history WHERE id IN (
        SELECT id FROM history ORDER BY last_visit_at DESC LIMIT -1 OFFSET 10000
      )",
    )
    .execute(&db.pool)
    .await?;
  }

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

#[cfg(test)]
mod tests {
  use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

  /// 建临时数据库（含 history 表 + 唯一索引，模拟迁移 v3 后的结构）
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
      "CREATE TABLE history (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        url TEXT NOT NULL,
        title TEXT,
        visit_count INTEGER DEFAULT 1,
        last_visit_at TEXT DEFAULT (datetime('now'))
      )",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("CREATE UNIQUE INDEX idx_history_url_uniq ON history(url)")
      .execute(&pool)
      .await
      .unwrap();
    (dir, pool)
  }

  /// 直接对 pool 执行 record_history 的核心 UPSERT 逻辑（命令签名带 tauri::State，测试里不便构造）
  async fn upsert(pool: &SqlitePool, url: &str, title: Option<&str>) -> sqlx::Result<()> {
    sqlx::query(
      "INSERT INTO history (url, title, visit_count, last_visit_at) VALUES (?, ?, 1, datetime('now'))
       ON CONFLICT(url) DO UPDATE SET
         visit_count = visit_count + 1,
         last_visit_at = datetime('now'),
         title = COALESCE(excluded.title, title)",
    )
    .bind(url)
    .bind(title)
    .execute(pool)
    .await?;
    Ok(())
  }

  #[tokio::test]
  async fn test_history_upsert_dedupes_and_counts() {
    let (_dir, pool) = temp_pool().await;
    // 同一 URL 并发/重复访问 3 次：只应有 1 行，visit_count 累加到 3
    upsert(&pool, "https://example.com/a", Some("标题A")).await.unwrap();
    upsert(&pool, "https://example.com/a", None).await.unwrap();
    upsert(&pool, "https://example.com/a", Some("标题A2")).await.unwrap();

    let (count, visits, title): (i64, i64, Option<String>) =
      sqlx::query_as("SELECT COUNT(*), visit_count, title FROM history")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "同 URL 重复访问不得产生多行");
    assert_eq!(visits, 3, "visit_count 应累加");
    assert_eq!(title.as_deref(), Some("标题A2"), "非空新标题应覆盖旧标题");

    // 不同 URL 正常各一行
    upsert(&pool, "https://example.com/b", Some("标题B")).await.unwrap();
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history")
      .fetch_one(&pool)
      .await
      .unwrap();
    assert_eq!(total, 2);
  }
}
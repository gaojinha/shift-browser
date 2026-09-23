// 历史记录管理
use ts_rs::TS;
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct HistoryEntry {
    #[ts(type = "number")]
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    #[ts(type = "number")]
    pub visit_count: i64,
    pub last_visit_at: String,
}

/// 访问计数：每 64 次记录才做一次上限检查。
/// 原先每次导航都跑全表 COUNT(*)，记录近万条时每跳一个页面都有可感开销。
static RECORD_TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 记录一次访问
#[tauri::command]
pub async fn record_history(
    db: tauri::State<'_, Db>,
    url: String,
    title: Option<String>,
) -> DbResult<()> {
    // 2026-09-19：标题为空时用 URL 域名兜底（页面加载完成时 document.title 可能还没就绪，
    // 历史里存了一堆空标题，最近访问只剩网址没有名字）
    let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| {
        url.parse::<tauri::Url>()
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| url.clone())
    });
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

    // 上限检查降频：每 64 次访问才查一次（清理本身是低频兜底，不必每次导航都做）
    let tick = RECORD_TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if tick % 64 != 0 {
        return Ok(());
    }

    // 快速短路：id 自增，max(id) 不超过上限时行数必然不超上限（走主键索引，比 COUNT 快）
    let max_id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM history")
        .fetch_one(&db.pool)
        .await?;
    if max_id <= 10_000 {
        return Ok(());
    }

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
pub async fn backfill_history_titles(db: tauri::State<'_, Db>) -> DbResult<usize> {
    // 2026-09-19：历史里标题为空的记录用 URL 域名兜底（页面加载时 document.title 未就绪导致）。
    // 一次性回填旧数据，避免最近访问只剩网址没有名字。
    let rows: Vec<(i64, String)> =
        sqlx::query_as("SELECT id, url FROM history WHERE title IS NULL OR title = ''")
            .fetch_all(&db.pool)
            .await?;
    let mut fixed = 0usize;
    for (id, url) in rows {
        if let Some(host) = url
            .parse::<tauri::Url>()
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
        {
            sqlx::query("UPDATE history SET title = ? WHERE id = ?")
                .bind(&host)
                .bind(id)
                .execute(&db.pool)
                .await?;
            fixed += 1;
        }
    }
    Ok(fixed)
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
        .map(
            |(id, url, title, visit_count, last_visit_at)| HistoryEntry {
                id,
                url,
                title,
                visit_count,
                last_visit_at,
            },
        )
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
    // LIKE 通配符转义：% 和 _ 是 SQL LIKE 的通配符，用户输入含这些字符时按字面量匹配
    // （先转义反斜杠本身，再转义 % 和 _，配合 ESCAPE '\' 使用）
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let like = format!("%{}%", escaped);
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, i64, String)>(
        "SELECT id, url, title, visit_count, last_visit_at FROM history
     WHERE url LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\'
     ORDER BY last_visit_at DESC LIMIT ?",
    )
    .bind(&like)
    .bind(&like)
    .bind(limit)
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, url, title, visit_count, last_visit_at)| HistoryEntry {
                id,
                url,
                title,
                visit_count,
                last_visit_at,
            },
        )
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
        .map(
            |(id, url, title, visit_count, last_visit_at)| HistoryEntry {
                id,
                url,
                title,
                visit_count,
                last_visit_at,
            },
        )
        .collect())
}

/// 清除全部历史
#[tauri::command]
pub async fn clear_history(db: tauri::State<'_, Db>) -> DbResult<()> {
    sqlx::query("DELETE FROM history").execute(&db.pool).await?;
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
    let res = sqlx::query("DELETE FROM history WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    if res.rows_affected() == 0 {
        return Err(crate::db::DbError::Other(format!("历史记录 {} 不存在", id)));
    }
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
        // 与生产迁移 v6 保持一致（get_recent_history 排序/清理用）
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_history_last_visit ON history(last_visit_at DESC)",
        )
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
        upsert(&pool, "https://example.com/a", Some("标题A"))
            .await
            .unwrap();
        upsert(&pool, "https://example.com/a", None).await.unwrap();
        upsert(&pool, "https://example.com/a", Some("标题A2"))
            .await
            .unwrap();

        let (count, visits, title): (i64, i64, Option<String>) =
            sqlx::query_as("SELECT COUNT(*), visit_count, title FROM history")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 1, "同 URL 重复访问不得产生多行");
        assert_eq!(visits, 3, "visit_count 应累加");
        assert_eq!(title.as_deref(), Some("标题A2"), "非空新标题应覆盖旧标题");

        // 不同 URL 正常各一行
        upsert(&pool, "https://example.com/b", Some("标题B"))
            .await
            .unwrap();
        let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM history")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total, 2);
    }

    /// 对 pool 执行 search_history 的核心 SQL（命令签名带 tauri::State，测试里不便构造）
    async fn search(pool: &SqlitePool, query: &str) -> Vec<String> {
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let like = format!("%{}%", escaped);
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT url FROM history
       WHERE url LIKE ? ESCAPE '\\' OR title LIKE ? ESCAPE '\\'
       ORDER BY last_visit_at DESC",
        )
        .bind(&like)
        .bind(&like)
        .fetch_all(pool)
        .await
        .unwrap();
        rows.into_iter().map(|r| r.0).collect()
    }

    #[tokio::test]
    async fn test_search_history_escapes_like_wildcards() {
        let (_dir, pool) = temp_pool().await;
        // 造数据：URL 含 % 和 _ 字面量，以及普通 URL
        upsert(&pool, "https://example.com/100%_off", Some("打折 100%_off"))
            .await
            .unwrap();
        upsert(&pool, "https://example.com/plain", Some("普通页面"))
            .await
            .unwrap();

        // 搜 "100" 应命中含 % 的那条（% 按字面量，不吞掉 100 后面的内容）
        let r = search(&pool, "100").await;
        assert!(
            r.iter().any(|u| u.contains("100%_off")),
            "搜 100 应命中 100%_off"
        );
        assert_eq!(r.len(), 1, "通配符不应让 100 匹配到无关行");

        // 搜 "off" 命中含 _ 字面量的那条（_ 不当作单字符通配符）
        let r = search(&pool, "off").await;
        assert!(
            r.iter().any(|u| u.contains("100%_off")),
            "off 是 _off 的字面量部分"
        );

        // 搜 "plain" 命中普通行
        let r = search(&pool, "plain").await;
        assert_eq!(r.len(), 1);
        assert!(r[0].contains("plain"));
    }
}

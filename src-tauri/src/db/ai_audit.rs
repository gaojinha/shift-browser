// AI 读取审计（K 类上下文与审计 · O3 审计持久化 · 2026-09-25）
//
// 目的：记录开放 API 的 AI 操作，使"AI 看了什么"可追溯；写入 db 表，重启不丢。
// 隐私（N5）：只记 命令 / url / tab_id / 结果码，**绝不记页面内容 / 表单值**。

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize, Clone)]
pub struct AiAuditEntry {
    pub id: i64,
    pub ts: String,
    pub cmd: String,
    pub url: String,
    pub tab_id: String,
    pub ok: bool,
    pub error_code: String,
    pub origin: String,
}

/// 审计保留上限：只留最近 5000 条，高频调用场景下表不无限增长
pub const AUDIT_KEEP: i64 = 5000;

/// 记一条审计（尽力而为：失败只记日志，绝不打断主流程）。
/// 写入与裁剪同一事务：只保留最近 AUDIT_KEEP 条。
pub async fn record(pool: &SqlitePool, cmd: &str, url: &str, tab_id: &str, ok: bool, error_code: &str) {
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(e) => {
            log::warn!("[ai_audit] 记录失败: {}", e);
            return;
        }
    };
    if let Err(e) = sqlx::query(
        "INSERT INTO ai_audit (cmd, url, tab_id, ok, error_code) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(cmd)
    .bind(url)
    .bind(tab_id)
    .bind(if ok { 1i64 } else { 0i64 })
    .bind(error_code)
    .execute(&mut *tx)
    .await
    {
        log::warn!("[ai_audit] 记录失败: {}", e);
        return;
    }
    if let Err(e) = sqlx::query(
        "DELETE FROM ai_audit WHERE id NOT IN (SELECT id FROM ai_audit ORDER BY id DESC LIMIT ?)",
    )
    .bind(AUDIT_KEEP)
    .execute(&mut *tx)
    .await
    {
        log::warn!("[ai_audit] 裁剪失败: {}", e);
        return;
    }
    if let Err(e) = tx.commit().await {
        log::warn!("[ai_audit] 提交失败: {}", e);
    }
}

/// 最近 N 条（按 id 倒序 = 时间倒序；同秒并列以 id 决胜）。
pub async fn recent(pool: &SqlitePool, limit: i64) -> Result<Vec<AiAuditEntry>, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, String, i64, String, String)>(
        "SELECT id, ts, cmd, url, tab_id, ok, error_code, origin \
         FROM ai_audit ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, ts, cmd, url, tab_id, ok, error_code, origin)| AiAuditEntry {
            id,
            ts,
            cmd,
            url,
            tab_id,
            ok: ok != 0,
            error_code,
            origin,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool(dir: &std::path::Path) -> SqlitePool {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let options = SqliteConnectOptions::new()
            .filename(dir.join("test.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../migrations/20260925000000_ai_audit.sql"))
            .execute(&pool)
            .await
            .unwrap();
        pool
    }

    #[tokio::test]
    async fn record_and_recent_roundtrip_no_content() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(dir.path()).await;
        record(&pool, "open_and_read", "https://example.com/a", "t1", true, "").await;
        record(&pool, "read_page", "https://example.com/b", "t1", false, "E_VALIDATION").await;
        let items = recent(&pool, 10).await.unwrap();
        assert_eq!(items.len(), 2);
        // 倒序：最后记的在最前
        assert_eq!(items[0].cmd, "read_page");
        assert!(!items[0].ok);
        assert_eq!(items[0].error_code, "E_VALIDATION");
        assert_eq!(items[1].cmd, "open_and_read");
        assert!(items[1].ok);
        assert_eq!(items[1].url, "https://example.com/a");
    }

    #[tokio::test]
    async fn recent_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(dir.path()).await;
        for i in 0..5 {
            record(&pool, "read_status", &format!("https://x/{i}"), "", true, "").await;
        }
        assert_eq!(recent(&pool, 3).await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn record_trims_to_keep_limit() {
        let dir = tempfile::tempdir().unwrap();
        let pool = test_pool(dir.path()).await;
        // 直插超限旧记录，再记一条触发裁剪
        for i in 0..(AUDIT_KEEP + 5) {
            sqlx::query("INSERT INTO ai_audit (cmd, url) VALUES ('x', ?)")
                .bind(format!("https://old/{i}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        record(&pool, "read_page", "https://new/1", "", true, "").await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ai_audit")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, AUDIT_KEEP);
        // 留的是最新的
        let newest: String = sqlx::query_scalar("SELECT url FROM ai_audit ORDER BY id DESC LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(newest, "https://new/1");
    }
}

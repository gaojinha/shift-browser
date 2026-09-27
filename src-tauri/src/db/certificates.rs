//! 证书数据层（管理 / 监测 / 验证 · 2026-09-22）
//!
//! 三张表：
//! - certificates    证书快照（host:port 唯一），指纹变更 → change_count++
//! - cert_exceptions 用户放行的证书例外（"继续前往"）
//! - cert_events     事件流（临期 / 变更 / 校验失败 / 放行）

use crate::tls::CertInfo;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Serialize)]
pub struct CertRecord {
    pub host: String,
    pub port: i64,
    pub fingerprint_sha256: String,
    pub subject: Option<String>,
    pub issuer: Option<String>,
    pub not_before: Option<String>,
    pub not_after: Option<String>,
    pub dns_names: Option<String>,
    pub ip_addresses: Option<String>,
    pub errors: i64,
    pub first_seen: String,
    pub last_seen: String,
    pub change_count: i64,
    /// 距到期天数（负数=已过期；NULL=无到期信息）
    pub days_left: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertException {
    pub host: String,
    pub port: i64,
    pub fingerprint_sha256: String,
    pub added_at: String,
    pub expires_at: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CertEvent {
    pub id: i64,
    pub host: String,
    pub port: i64,
    pub kind: String,
    pub severity: String,
    pub detail: Option<String>,
    pub created_at: String,
}

fn row_to_record(r: &sqlx::sqlite::SqliteRow) -> CertRecord {
    CertRecord {
        host: r.get("host"),
        port: r.get("port"),
        fingerprint_sha256: r.get("fingerprint_sha256"),
        subject: r.get("subject"),
        issuer: r.get("issuer"),
        not_before: r.get("not_before"),
        not_after: r.get("not_after"),
        dns_names: r.get("dns_names"),
        ip_addresses: r.get("ip_addresses"),
        errors: r.get("errors"),
        first_seen: r.get("first_seen"),
        last_seen: r.get("last_seen"),
        change_count: r.get("change_count"),
        days_left: r
            .try_get::<Option<f64>, _>("days_left")
            .ok()
            .flatten()
            .map(|d| d as i64),
    }
}

/// 记录/更新证书快照。返回 `Some(变更类型)`：
/// - Some("first_seen") 首次见到
/// - Some("cert_changed") 指纹与旧记录不同（疑似中间人 / 证书轮换）
/// - Some("verify_error") 本次校验有错误位
/// - None 无变化
pub async fn record(pool: &SqlitePool, info: &CertInfo) -> Option<&'static str> {
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT fingerprint_sha256 FROM certificates WHERE host = ? AND port = ?",
    )
    .bind(&info.host)
    .bind(info.port as i64)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();

    let dns = info.dns_names.join(", ");
    let ips = info.ip_addresses.join(", ");
    let chain = serde_json::to_string(&info.chain).unwrap_or_else(|_| "[]".into());

    match existing {
        None => {
            let _ = sqlx::query(
                "INSERT INTO certificates
                 (host, port, fingerprint_sha256, subject, issuer, not_before, not_after,
                  dns_names, ip_addresses, errors, chain_json, last_checked)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, datetime('now'))
                 ON CONFLICT(host, port) DO UPDATE SET
                   fingerprint_sha256=excluded.fingerprint_sha256, subject=excluded.subject,
                   issuer=excluded.issuer, not_before=excluded.not_before, not_after=excluded.not_after,
                   dns_names=excluded.dns_names, ip_addresses=excluded.ip_addresses,
                   errors=excluded.errors, chain_json=excluded.chain_json,
                   last_seen=datetime('now'), last_checked=datetime('now')",
            )
            .bind(&info.host)
            .bind(info.port as i64)
            .bind(&info.fingerprint_sha256)
            .bind(&info.subject)
            .bind(&info.issuer)
            .bind(&info.not_before)
            .bind(&info.not_after)
            .bind(&dns)
            .bind(&ips)
            .bind(info.errors as i64)
            .bind(&chain)
            .execute(pool)
            .await;
            log::info!("[cert] 首次记录 {}:{} fp={}", info.host, info.port, &info.fingerprint_sha256[..12.min(info.fingerprint_sha256.len())]);
            Some("first_seen")
        }
        Some(old_fp) if old_fp != info.fingerprint_sha256 => {
            let _ = sqlx::query(
                "UPDATE certificates SET
                   fingerprint_sha256=?, subject=?, issuer=?, not_before=?, not_after=?,
                   dns_names=?, ip_addresses=?, errors=?, chain_json=?,
                   last_seen=datetime('now'), last_checked=datetime('now'),
                   change_count = change_count + 1
                 WHERE host=? AND port=?",
            )
            .bind(&info.fingerprint_sha256)
            .bind(&info.subject)
            .bind(&info.issuer)
            .bind(&info.not_before)
            .bind(&info.not_after)
            .bind(&dns)
            .bind(&ips)
            .bind(info.errors as i64)
            .bind(&chain)
            .bind(&info.host)
            .bind(info.port as i64)
            .execute(pool)
            .await;
            log::warn!("[cert] 证书指纹变更 {}:{} {} → {}", info.host, info.port, &old_fp[..12.min(old_fp.len())], &info.fingerprint_sha256[..12.min(info.fingerprint_sha256.len())]);
            Some("cert_changed")
        }
        Some(_) => {
            let _ = sqlx::query(
                "UPDATE certificates SET last_seen=datetime('now'), last_checked=datetime('now'),
                   errors=?, not_after=?, dns_names=?, ip_addresses=? WHERE host=? AND port=?",
            )
            .bind(info.errors as i64)
            .bind(&info.not_after)
            .bind(&dns)
            .bind(&ips)
            .bind(&info.host)
            .bind(info.port as i64)
            .execute(pool)
            .await;
            if !info.valid {
                Some("verify_error")
            } else {
                None
            }
        }
    }
}

/// 列出监测中的证书（临期优先）
pub async fn list(pool: &SqlitePool) -> Result<Vec<CertRecord>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT *, CAST(julianday(not_after) - julianday('now') AS INTEGER) AS days_left
         FROM certificates
         ORDER BY (days_left IS NULL), days_left ASC, host ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_record).collect())
}

pub async fn get(pool: &SqlitePool, host: &str, port: i64) -> Result<Option<CertRecord>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT *, CAST(julianday(not_after) - julianday('now') AS INTEGER) AS days_left
         FROM certificates WHERE host=? AND port=?",
    )
    .bind(host)
    .bind(port)
    .fetch_optional(pool)
    .await?;
    Ok(row.as_ref().map(row_to_record))
}

pub async fn delete(pool: &SqlitePool, host: &str, port: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM certificates WHERE host=? AND port=?")
        .bind(host)
        .bind(port)
        .execute(pool)
        .await?;
    Ok(())
}

/// 临期/已过期证书（days 天数窗口内）
pub async fn expiring_within(pool: &SqlitePool, days: i64) -> Result<Vec<CertRecord>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT *, CAST(julianday(not_after) - julianday('now') AS INTEGER) AS days_left
         FROM certificates
         WHERE not_after IS NOT NULL AND julianday(not_after) - julianday('now') <= ?
         ORDER BY days_left ASC",
    )
    .bind(days as f64)
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_record).collect())
}

// ---------------- 例外（用户放行） ----------------

pub async fn add_exception(
    pool: &SqlitePool,
    host: &str,
    port: i64,
    fp: &str,
    reason: &str,
) -> Result<(), sqlx::Error> {
    // 安全（#9 · 2026-09-22）：例外默认 90 天有效期，到期自动失效（避免永久放行）
    sqlx::query(
        "INSERT OR REPLACE INTO cert_exceptions
         (host, port, fingerprint_sha256, reason, added_at, expires_at)
         VALUES (?, ?, ?, ?, datetime('now'), datetime('now', '+90 days'))",
    )
    .bind(host)
    .bind(port)
    .bind(fp)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn list_exceptions(pool: &SqlitePool) -> Result<Vec<CertException>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT host, port, fingerprint_sha256, added_at, expires_at, reason
         FROM cert_exceptions ORDER BY added_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| CertException {
            host: r.get("host"),
            port: r.get("port"),
            fingerprint_sha256: r.get("fingerprint_sha256"),
            added_at: r.get("added_at"),
            expires_at: r.get("expires_at"),
            reason: r.get("reason"),
        })
        .collect())
}

pub async fn delete_exception(
    pool: &SqlitePool,
    host: &str,
    port: i64,
    fp: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM cert_exceptions WHERE host=? AND port=? AND fingerprint_sha256=?")
        .bind(host)
        .bind(port)
        .bind(fp)
        .execute(pool)
        .await?;
    Ok(())
}

/// 该 host:port:fp 是否已有"未过期"的放行例外
pub async fn has_exception(pool: &SqlitePool, host: &str, port: i64, fp: &str) -> bool {
    let n: Option<i64> = sqlx::query_scalar(
        "SELECT COUNT(1) FROM cert_exceptions
         WHERE host=? AND port=? AND fingerprint_sha256=?
           AND (expires_at IS NULL OR expires_at > datetime('now'))",
    )
    .bind(host)
    .bind(port)
    .bind(fp)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten();
    n.unwrap_or(0) > 0
}

// ---------------- 事件流 ----------------

/// 事件保留上限：只留最近 1000 条，证书事件低频但长期运行只增不减
pub const EVENTS_KEEP: i64 = 1000;

pub async fn add_event(
    pool: &SqlitePool,
    host: &str,
    port: i64,
    kind: &str,
    severity: &str,
    detail: &str,
) {
    if sqlx::query(
        "INSERT INTO cert_events (host, port, kind, severity, detail) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(host)
    .bind(port)
    .bind(kind)
    .bind(severity)
    .bind(detail)
    .execute(pool)
    .await
    .is_err()
    {
        return;
    }
    // 尽力裁剪：失败不影响主流程
    let _ = sqlx::query(
        "DELETE FROM cert_events WHERE id NOT IN (SELECT id FROM cert_events ORDER BY id DESC LIMIT ?)",
    )
    .bind(EVENTS_KEEP)
    .execute(pool)
    .await;
}

pub async fn list_events(pool: &SqlitePool, limit: i64) -> Result<Vec<CertEvent>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, host, port, kind, severity, detail, created_at
         FROM cert_events ORDER BY id DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .map(|r| CertEvent {
            id: r.get("id"),
            host: r.get("host"),
            port: r.get("port"),
            kind: r.get("kind"),
            severity: r.get("severity"),
            detail: r.get("detail"),
            created_at: r.get("created_at"),
        })
        .collect())
}

pub async fn clear_events(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM cert_events").execute(pool).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_pool() -> (tempfile::TempDir, SqlitePool) {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("cert_test.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20260922000000_certificates.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn add_event_trims_to_keep_limit() {
        let (_dir, pool) = test_pool().await;
        for i in 0..(EVENTS_KEEP + 5) {
            sqlx::query("INSERT INTO cert_events (host, kind) VALUES (?, 'info')")
                .bind(format!("old-{i}.example.com"))
                .execute(&pool)
                .await
                .unwrap();
        }
        add_event(&pool, "new.example.com", 443, "first_seen", "info", "").await;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cert_events")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count, EVENTS_KEEP);
        let newest: String =
            sqlx::query_scalar("SELECT host FROM cert_events ORDER BY id DESC LIMIT 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(newest, "new.example.com");
    }
}

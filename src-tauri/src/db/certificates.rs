// LINES-EXEMPT: 证书数据层（管理/监测/验证），拆分排期 R4（2026-10-14 红线卡标注）
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
            .try_get::<Option<i64>, _>("days_left")
            .unwrap_or_else(|e| {
                // F-66：SQL 侧 CAST AS INTEGER，解码端对齐 i64；类型不匹配不再静默吞成
                // None（此前 f64 解码 INTEGER 恒失败 → 前端「剩余天数」列恒空），至少落一条日志
                log::warn!("[certificates] days_left 解码失败: {e:?}");
                None
            }),
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
            if let Err(e) = sqlx::query(
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
            .await
            {
                log::debug!("[cert] 写入首见记录失败（best-effort）: {}", e);
            }
            log::info!(
                "[cert] 首次记录 {}:{} fp={}",
                info.host,
                info.port,
                &info.fingerprint_sha256[..12.min(info.fingerprint_sha256.len())]
            );
            Some("first_seen")
        }
        Some(old_fp) if old_fp != info.fingerprint_sha256 => {
            if let Err(e) = sqlx::query(
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
            .await
            {
                log::debug!("[cert] 记录指纹变更失败（best-effort）: {}", e);
            }
            log::warn!(
                "[cert] 证书指纹变更 {}:{} {} → {}",
                info.host,
                info.port,
                &old_fp[..12.min(old_fp.len())],
                &info.fingerprint_sha256[..12.min(info.fingerprint_sha256.len())]
            );
            Some("cert_changed")
        }
        Some(_) => {
            if let Err(e) = sqlx::query(
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
            .await
            {
                log::debug!("[cert] 更新最近见到时间失败（best-effort）: {}", e);
            }
            if !info.valid {
                Some("verify_error")
            } else {
                None
            }
        }
    }
}

/// 列出监测中的证书（临期优先）。
/// F-CEF-21：Tauri 命令与 CEF 面板共用的唯一 SQL 实现，两侧不许复制 SQL。
pub async fn list_certificates(pool: &SqlitePool) -> Result<Vec<CertRecord>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT *, CAST(julianday(not_after) - julianday('now') AS INTEGER) AS days_left
         FROM certificates
         ORDER BY (days_left IS NULL), days_left ASC, host ASC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.iter().map(row_to_record).collect())
}

pub async fn get(
    pool: &SqlitePool,
    host: &str,
    port: i64,
) -> Result<Option<CertRecord>, sqlx::Error> {
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
    if let Err(e) = sqlx::query(
        "DELETE FROM cert_events WHERE id NOT IN (SELECT id FROM cert_events ORDER BY id DESC LIMIT ?)",
    )
    .bind(EVENTS_KEEP)
    .execute(pool)
    .await
    {
        log::debug!("[cert] 裁剪旧事件失败（best-effort）: {}", e);
    }
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

    fn info(host: &str, port: u16, fp: &str, not_after: Option<&str>) -> CertInfo {
        // Default 的 valid=false，这里显式置 true（"无变化"分支的默认路径）
        CertInfo {
            host: host.into(),
            port,
            fingerprint_sha256: fp.into(),
            not_after: not_after.map(String::from),
            valid: true,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn record_state_machine_first_unchanged_verify_error_changed() {
        let (_dir, pool) = test_pool().await;
        // 首见：first_seen
        assert_eq!(
            record(
                &pool,
                &info("a.com", 443, "fpA", Some("2026-12-01 00:00:00"))
            )
            .await,
            Some("first_seen")
        );
        // 同指纹再校验：无变化 → None，change_count 保持 0
        assert_eq!(
            record(
                &pool,
                &info("a.com", 443, "fpA", Some("2026-12-01 00:00:00"))
            )
            .await,
            None
        );
        // 同指纹但校验带错误位：verify_error，不算指纹变更
        let bad = CertInfo {
            valid: false,
            ..info("a.com", 443, "fpA", Some("2026-12-01 00:00:00"))
        };
        assert_eq!(record(&pool, &bad).await, Some("verify_error"));
        let cc: i64 = sqlx::query_scalar(
            "SELECT change_count FROM certificates WHERE host='a.com' AND port=443",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cc, 0, "verify_error 不递增 change_count");
        // 指纹变更：cert_changed，change_count+1，新指纹落库
        assert_eq!(
            record(
                &pool,
                &info("a.com", 443, "fpB", Some("2027-01-01 00:00:00"))
            )
            .await,
            Some("cert_changed")
        );
        let (cc, fp): (i64, String) = sqlx::query_as(
            "SELECT change_count, fingerprint_sha256 FROM certificates WHERE host='a.com' AND port=443",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(cc, 1);
        assert_eq!(fp, "fpB");
        // 变更后稳定：回到 None
        assert_eq!(
            record(
                &pool,
                &info("a.com", 443, "fpB", Some("2027-01-01 00:00:00"))
            )
            .await,
            None
        );
        // 同 host 不同 port：独立快照行（host:port 主键）
        assert_eq!(
            record(&pool, &info("a.com", 8443, "fpC", None)).await,
            Some("first_seen")
        );
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM certificates")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 2);
        assert!(get(&pool, "a.com", 443).await.unwrap().is_some());
        assert!(get(&pool, "a.com", 8443).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn expiring_within_window_boundaries_and_null_last() {
        let (_dir, pool) = test_pool().await;
        // not_after 用 SQL 的 datetime('now', ...) 生成，与查询同钟源，避免外部时间漂移
        for (host, delta) in [
            ("expired.com", "-5 days"),
            ("soon.com", "+20 days"),
            ("edge.com", "+30 days"),
            ("far.com", "+31 days"),
        ] {
            let na: String = sqlx::query_scalar(&format!("SELECT datetime('now', '{delta}')"))
                .fetch_one(&pool)
                .await
                .unwrap();
            record(&pool, &info(host, 443, &format!("fp-{host}"), Some(&na))).await;
        }
        // 无到期信息：not_after = NULL
        record(&pool, &info("forever.com", 443, "fp-f", None)).await;

        // 30 天窗口：正好 +30 天的边界——查询的 'now' 不早于插入时刻，差值恒 ≤30 → 入窗；
        // +31 天恒 >30 → 出窗；NULL 恒出窗
        let got = expiring_within(&pool, 30).await.unwrap();
        let hosts: Vec<String> = got.iter().map(|r| r.host.clone()).collect();
        assert_eq!(
            hosts,
            vec![
                "expired.com".to_string(),
                "soon.com".to_string(),
                "edge.com".to_string()
            ],
            "days_left 升序：已过期(-5) → 20 天 → 30 天边界；31 天与 NULL 不入窗"
        );
        // days_left 解码（F-66 已修：try_get::<Option<i64>> 对齐 SQL 的 CAST AS INTEGER）。
        // 插入与查询同用 SQL datetime('now') 钟源，查询时刻恒不早于插入时刻，差值为
        // ±0 的极小负偏移：CAST 截断后 soon/edge 可能落 19/20、29/30 边界，按区间断言。
        let days = |host: &str| -> i64 {
            got.iter()
                .find(|r| r.host == host)
                .and_then(|r| r.days_left)
                .unwrap_or_else(|| panic!("days_left 解码失败（{}）", host))
        };
        assert_eq!(days("expired.com"), -5, "已过期 -5 天：负值截断后恒 -5");
        assert!(
            (19..=20).contains(&days("soon.com")),
            "soon.com 应为 +20 天边界（实际 {}）",
            days("soon.com")
        );
        assert!(
            (29..=30).contains(&days("edge.com")),
            "edge.com 应为 +30 天边界（实际 {}）",
            days("edge.com")
        );
        // 窄窗口（4 天）：只剩已过期
        let got4 = expiring_within(&pool, 4).await.unwrap();
        assert_eq!(got4.len(), 1);
        assert_eq!(got4[0].host, "expired.com");

        // list()：5 行全量、最临期在前、无到期信息恒排最后
        let all = list_certificates(&pool).await.unwrap();
        assert_eq!(all.len(), 5);
        assert_eq!(all[0].host, "expired.com");
        assert_eq!(all[4].host, "forever.com");
        assert_eq!(all[4].days_left, None);
    }

    #[tokio::test]
    async fn exception_add_replace_delete_idempotent_and_expiry() {
        let (_dir, pool) = test_pool().await;
        add_exception(&pool, "b.com", 443, "fp1", "自签")
            .await
            .unwrap();
        let ex = list_exceptions(&pool).await.unwrap();
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].host, "b.com");
        assert_eq!(ex[0].fingerprint_sha256, "fp1");
        assert_eq!(ex[0].reason.as_deref(), Some("自签"));
        assert!(
            ex[0].expires_at.is_some(),
            "例外默认 90 天有效期（防永久放行）"
        );
        assert!(has_exception(&pool, "b.com", 443, "fp1").await);
        // 例外按指纹锁定：同 host:port 的其它指纹不受放行
        assert!(!has_exception(&pool, "b.com", 443, "other-fp").await);
        // 同三元组重复放行 = 覆盖：行数不变、reason 更新
        add_exception(&pool, "b.com", 443, "fp1", "更新原因")
            .await
            .unwrap();
        let ex2 = list_exceptions(&pool).await.unwrap();
        assert_eq!(ex2.len(), 1, "INSERT OR REPLACE 不产生重复行");
        assert_eq!(ex2[0].reason.as_deref(), Some("更新原因"));
        assert!(has_exception(&pool, "b.com", 443, "fp1").await);
        // 删除 + 重复删除：幂等无错
        delete_exception(&pool, "b.com", 443, "fp1").await.unwrap();
        delete_exception(&pool, "b.com", 443, "fp1").await.unwrap();
        assert!(!has_exception(&pool, "b.com", 443, "fp1").await);
        assert_eq!(list_exceptions(&pool).await.unwrap().len(), 0);
        // 已过期例外不再算有效放行，但列表仍展示
        add_exception(&pool, "b.com", 443, "fp2", "过期测试")
            .await
            .unwrap();
        sqlx::query(
            "UPDATE cert_exceptions SET expires_at = datetime('now', '-1 day')
             WHERE fingerprint_sha256='fp2'",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert!(
            !has_exception(&pool, "b.com", 443, "fp2").await,
            "过期例外不再算有效放行"
        );
        assert_eq!(
            list_exceptions(&pool).await.unwrap().len(),
            1,
            "list 不过滤过期（展示归 UI）"
        );
    }

    // ---- F-CEF-21：list_certificates（Tauri 命令与 CEF 面板共用）----

    #[tokio::test]
    async fn list_certificates_empty_db_returns_empty() {
        let (_dir, pool) = test_pool().await;
        assert!(
            list_certificates(&pool).await.unwrap().is_empty(),
            "空库应返回空数组（CEF 面板空态依赖）"
        );
    }

    #[tokio::test]
    async fn list_certificates_orders_by_days_then_host() {
        let (_dir, pool) = test_pool().await;
        // not_after 用 SQL 的 datetime('now', ...) 生成，与查询同钟源；
        // 档位间隔 ≥4 天，CAST 截断 ±1 不会重排
        for (host, port, fp, delta) in [
            ("z.com", 443, "fp-z", "+30 days"),
            ("a.com", 443, "fp-a", "+5 days"),
            ("m.com", 443, "fp-m", "+30 days"),
            ("a.com", 8443, "fp-a8", "+10 days"),
        ] {
            let na: String =
                sqlx::query_scalar(&format!("SELECT datetime('now', '{delta}')"))
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            record(&pool, &info(host, port, fp, Some(&na))).await;
        }
        let got = list_certificates(&pool).await.unwrap();
        let keys: Vec<String> = got.iter().map(|r| format!("{}:{}", r.host, r.port)).collect();
        assert_eq!(
            keys,
            vec![
                "a.com:443".to_string(),
                "a.com:8443".to_string(),
                "m.com:443".to_string(),
                "z.com:443".to_string(),
            ],
            "days_left 升序（5/10/30）；days_left 相同时 host 升序兜底（m < z）"
        );
    }

    #[tokio::test]
    async fn list_certificates_field_roundtrip() {
        let (_dir, pool) = test_pool().await;
        let na: String = sqlx::query_scalar("SELECT datetime('now', '+66 days')")
            .fetch_one(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO certificates
             (host, port, fingerprint_sha256, subject, issuer, not_before, not_after,
              dns_names, ip_addresses, errors, first_seen, last_seen, change_count)
             VALUES ('rt.example', 8443, 'fp-rt', 'CN=rt.example', 'CN=test-ca',
                     '2026-01-01 00:00:00', ?,
                     'rt.example, www.rt.example', '203.0.113.7', 3,
                     '2026-01-02 03:04:05', '2026-01-03 06:07:08', 2)",
        )
        .bind(&na)
        .execute(&pool)
        .await
        .unwrap();
        let got = list_certificates(&pool).await.unwrap();
        assert_eq!(got.len(), 1);
        let r = &got[0];
        assert_eq!(r.host, "rt.example");
        assert_eq!(r.port, 8443);
        assert_eq!(r.fingerprint_sha256, "fp-rt");
        assert_eq!(r.subject.as_deref(), Some("CN=rt.example"));
        assert_eq!(r.issuer.as_deref(), Some("CN=test-ca"));
        assert_eq!(r.not_before.as_deref(), Some("2026-01-01 00:00:00"));
        assert_eq!(r.dns_names.as_deref(), Some("rt.example, www.rt.example"));
        assert_eq!(r.ip_addresses.as_deref(), Some("203.0.113.7"));
        assert_eq!(r.errors, 3);
        assert_eq!(r.first_seen, "2026-01-02 03:04:05");
        assert_eq!(r.last_seen, "2026-01-03 06:07:08");
        assert_eq!(r.change_count, 2);
        // days_left：not_after = now+66d 同钟源 → 查询时刻恒略晚于插入，取值 {65,66}
        let dl = r
            .days_left
            .expect("days_left 应解码成功（not_after 非 NULL，F-66 后不吞错）");
        assert!((65..=66).contains(&dl), "not_after +66 天 → days_left 应为 65~66（实际 {dl}）");
    }
}

// 定时任务（阶段3 · 自动化/工作流）
// 设置驱动：`auto_clear_history_days`（0=关，单位天）、`auto_refresh_feeds_minutes`（0=关，单位分钟）。
// 调度：启动时起一个后台 tokio 任务，每 60 秒检查一次是否到期（到期才执行）。
// 读数走 settings_cache（四键已进白名单，写时同步更新）：默认全关时每轮零库查询，
// 分钟级触发精度保持 60 秒不变（若改 300 秒轮询，1 分钟的任务会被拖成 5 分钟）。
// 到期判定用进程内 last_run（重启后重新计时；"天/分钟"级任务，重启重置可接受）。
use crate::db::Db;
use tauri::{AppHandle, Emitter, Manager};

/// 解析设置值为正整数（0=关 / 负数 / 非法 → None）。
/// 三个调度块共用同一口径：trim + parse + >0。
fn parse_positive(v: &str) -> Option<i64> {
    v.trim().parse::<i64>().ok().filter(|n| *n > 0)
}

/// 到期判定：间隔 >0 且距上次执行 ≥ 间隔（last_run 重启后从 0 起算，见模块注释）。
fn due(last_run: i64, now: i64, interval_secs: i64) -> bool {
    interval_secs > 0 && now - last_run >= interval_secs
}

/// 起调度器（lib.rs setup 调用）
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last_history: i64 = 0;
        let mut last_feeds: i64 = 0;
        let mut last_agent: i64 = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let Some(db) = app.try_state::<Db>() else {
                continue;
            };

            // 定时清理历史：删 N 天前的记录
            if let Some(v) =
                crate::db::settings_cache::get(&db.pool, "auto_clear_history_days").await
            {
                if let Some(days) = parse_positive(&v) {
                    if due(last_history, now, 86_400) {
                        match clear_history_older(&db.pool, days).await {
                            Ok(n) => {
                                log::info!("[auto] 定时清理历史：删除 {} 条（{} 天前）", n, days)
                            }
                            Err(e) => log::warn!("[auto] 定时清理历史失败: {}", e),
                        }
                        last_history = now;
                    }
                }
            }

            // 定时刷新订阅
            if let Some(v) =
                crate::db::settings_cache::get(&db.pool, "auto_refresh_feeds_minutes").await
            {
                if let Some(mins) = parse_positive(&v) {
                    if due(last_feeds, now, mins * 60) {
                        let n = crate::feed::refresh_all(&db).await;
                        log::info!("[auto] 定时刷新订阅：{} 个源", n);
                        last_feeds = now;
                    }
                }
            }

            // 定时跑 Agent 提示词（阶段3 Agent 编排：调度器 ↔ AI 打通）
            // 到点发事件，由前端打开 AI 侧栏并发送该提示词（AI 的多步动作仍走原有确认流程）
            if let Some(v) = crate::db::settings_cache::get(&db.pool, "auto_agent_minutes").await {
                if let Some(mins) = parse_positive(&v) {
                    if due(last_agent, now, mins * 60) {
                        if let Some(prompt) =
                            crate::db::settings_cache::get(&db.pool, "auto_agent_prompt").await
                        {
                            let p = prompt.trim().to_string();
                            if !p.is_empty() {
                                let _ = app.emit(crate::contracts::events::AUTO_AGENT, p);
                                log::info!("[auto] 触发 Agent 提示词（每 {} 分钟）", mins);
                            }
                        }
                        last_agent = now;
                    }
                }
            }
        }
    });
    log::info!("[auto] 定时任务调度器已启动（每 60s 检查一次）");
}

async fn clear_history_older(pool: &sqlx::SqlitePool, days: i64) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM history WHERE last_visit_at < datetime('now', ?)")
        .bind(format!("-{} days", days))
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（history 表齐全）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    /// 插一条指定"天偏移"的历史（负数=过去）
    async fn seed_history(pool: &sqlx::SqlitePool, url: &str, day_offset: i64) {
        sqlx::query(
            "INSERT INTO history (url, title, visit_count, last_visit_at)
             VALUES (?, ?, 1, datetime('now', ?))",
        )
        .bind(url)
        .bind(url)
        .bind(format!("{} days", day_offset))
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn clear_history_older_keeps_recent_deletes_old() {
        let (_dir, db) = temp_db().await;
        seed_history(&db.pool, "https://now.com", 0).await;
        seed_history(&db.pool, "https://yesterday.com", -1).await;
        seed_history(&db.pool, "https://ancient.com", -10).await;

        let n = clear_history_older(&db.pool, 5).await.unwrap();
        assert_eq!(n, 1, "只删 5 天前的 1 条");

        // 幂等：再跑一遍应为 0
        let n2 = clear_history_older(&db.pool, 5).await.unwrap();
        assert_eq!(n2, 0, "重复清理应 0 条");

        // 幸存者正确：近期两条在，古董条目没了
        let remaining: Vec<String> =
            sqlx::query_scalar("SELECT url FROM history ORDER BY url").fetch_all(&db.pool)
                .await.unwrap();
        assert_eq!(remaining, vec!["https://now.com", "https://yesterday.com"]);
    }

    #[test]
    fn parse_positive_common_thresholds() {
        assert_eq!(parse_positive("7"), Some(7));
        assert_eq!(parse_positive("  30  "), Some(30), "trim 后可解析");
        // 0=关：不触发（None 与 Ok(0)+>0 判定等价，这里统一收敛为 None）
        assert_eq!(parse_positive("0"), None);
        assert_eq!(parse_positive("-5"), None);
        assert_eq!(parse_positive("abc"), None);
        assert_eq!(parse_positive(""), None);
        assert_eq!(parse_positive("1.5"), None);
    }

    #[test]
    fn due_boundaries_and_fresh_start() {
        // 重启后 last_run=0：首个轮询窗口即可达（天级任务重启即跑一次，与原口径一致）
        assert!(due(0, 60, 60));
        // 恰好到点（>= 判定）
        assert!(due(100, 160, 60));
        assert!(!due(100, 159, 60), "未满间隔不触发");
        // 间隔非正永不触发（防御：解析层已滤 0/负，这里锁死纯函数语义）
        assert!(!due(0, 1_000_000, 0));
        assert!(!due(0, 1_000_000, -60));
    }
}

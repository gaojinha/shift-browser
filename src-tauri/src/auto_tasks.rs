// 定时任务（阶段3 · 自动化/工作流）
// 设置驱动：`auto_clear_history_days`（0=关，单位天）、`auto_refresh_feeds_minutes`（0=关，单位分钟）。
// 调度：启动时起一个后台 tokio 任务，每 60 秒检查一次是否到期（到期才执行）。
// 到期判定用进程内 last_run（重启后重新计时；"天/分钟"级任务，重启重置可接受）。
use crate::db::Db;
use tauri::{AppHandle, Emitter, Manager};

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
            if let Ok(Some(v)) = read_setting(&db.pool, "auto_clear_history_days").await {
                if let Ok(days) = v.trim().parse::<i64>() {
                    if days > 0 && now - last_history >= 86_400 {
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
            if let Ok(Some(v)) = read_setting(&db.pool, "auto_refresh_feeds_minutes").await {
                if let Ok(mins) = v.trim().parse::<i64>() {
                    if mins > 0 && now - last_feeds >= mins * 60 {
                        let n = crate::feed::refresh_all(&db).await;
                        log::info!("[auto] 定时刷新订阅：{} 个源", n);
                        last_feeds = now;
                    }
                }
            }

            // 定时跑 Agent 提示词（阶段3 Agent 编排：调度器 ↔ AI 打通）
            // 到点发事件，由前端打开 AI 侧栏并发送该提示词（AI 的多步动作仍走原有确认流程）
            if let Ok(Some(v)) = read_setting(&db.pool, "auto_agent_minutes").await {
                if let Ok(mins) = v.trim().parse::<i64>() {
                    if mins > 0 && now - last_agent >= mins * 60 {
                        if let Ok(Some(prompt)) = read_setting(&db.pool, "auto_agent_prompt").await {
                            let p = prompt.trim().to_string();
                            if !p.is_empty() {
                                let _ = app.emit("auto-agent", p);
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

async fn read_setting(pool: &sqlx::SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

async fn clear_history_older(pool: &sqlx::SqlitePool, days: i64) -> Result<u64, sqlx::Error> {
    let r = sqlx::query("DELETE FROM history WHERE last_visit_at < datetime('now', ?)")
        .bind(format!("-{} days", days))
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

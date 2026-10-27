// 会话管理：保存/恢复标签页状态
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Session {
    #[ts(type = "number")]
    pub id: i64,
    pub name: String,
    #[ts(type = "number | null")]
    pub workspace_id: Option<i64>,
    /// 标签页 JSON 快照：[{"url":"...","title":"..."}]
    pub data: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct SessionTab {
    pub url: String,
    pub title: Option<String>,
    /// 标签分组归属（持久化；null = 未分组）。serde(default) 兼容旧快照无此字段
    #[serde(default, rename = "groupId")]
    #[ts(type = "number | null")]
    pub group_id: Option<i64>,
}

/// 保存当前会话（标签页快照）
/// workspace_id：所属工作区（None = 默认工作区）
#[tauri::command]
pub async fn save_session(
    db: tauri::State<'_, Db>,
    name: String,
    workspace_id: Option<i64>,
    tabs: Vec<SessionTab>,
) -> DbResult<i64> {
    save_session_inner(&db.pool, &name, workspace_id, &tabs).await
}

/// 保存会话核心逻辑（pool 级，便于测试）
pub async fn save_session_inner(
    pool: &sqlx::SqlitePool,
    name: &str,
    workspace_id: Option<i64>,
    tabs: &[SessionTab],
) -> DbResult<i64> {
    let data = serde_json::to_string(tabs)?;
    // P2：INSERT 与自动保存清理放同一事务——清理失败时整体回滚，
    // 避免"插入已提交却上报失败"，前端重试产生重复会话
    let mut tx = pool.begin().await?;
    let result = sqlx::query("INSERT INTO sessions (name, workspace_id, data) VALUES (?, ?, ?)")
        .bind(name)
        .bind(workspace_id)
        .bind(&data)
        .execute(&mut *tx)
        .await?;
    // 自动保存记录只保留最近 100 条，防止 sessions 表无限增长
    // （手动命名会话走下面的每工作区 50 条规则）
    sqlx::query(
        "DELETE FROM sessions WHERE name = '自动保存' AND id NOT IN (
           SELECT id FROM sessions WHERE name = '自动保存' ORDER BY id DESC LIMIT 100
         )",
    )
    .execute(&mut *tx)
    .await?;
    // 手动命名会话每工作区只保留最近 50 条（workspace_id NULL 用 -1 归一，自增 id 永不为负）：
    // 只在本次存的是手动会话时执行，自动保存走上面的 100 条规则，两套互不干扰
    if name != "自动保存" {
        sqlx::query(
            "DELETE FROM sessions WHERE name != '自动保存'
             AND IFNULL(workspace_id, -1) = IFNULL(?, -1) AND id NOT IN (
               SELECT id FROM sessions WHERE name != '自动保存'
               AND IFNULL(workspace_id, -1) = IFNULL(?, -1) ORDER BY id DESC LIMIT 50
             )",
        )
        .bind(workspace_id)
        .bind(workspace_id)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(result.last_insert_rowid())
}

/// 加载指定工作区最近一次会话（启动恢复/切换工作区用）
/// workspace_id：None = 默认工作区
#[tauri::command]
pub async fn load_last_session(
    db: tauri::State<'_, Db>,
    workspace_id: Option<i64>,
) -> DbResult<Option<Session>> {
    let row = if let Some(wid) = workspace_id {
        sqlx::query_as::<_, (i64, String, Option<i64>, String, String)>(
            "SELECT id, name, workspace_id, data, created_at FROM sessions
             WHERE workspace_id = ? ORDER BY id DESC LIMIT 1",
        )
        .bind(wid)
        .fetch_optional(&db.pool)
        .await?
    } else {
        sqlx::query_as::<_, (i64, String, Option<i64>, String, String)>(
            "SELECT id, name, workspace_id, data, created_at FROM sessions
             WHERE workspace_id IS NULL ORDER BY id DESC LIMIT 1",
        )
        .fetch_optional(&db.pool)
        .await?
    };

    let session = row.map(|(id, name, workspace_id, data, created_at)| Session {
        id,
        name,
        workspace_id,
        data,
        created_at,
    });

    // 快照损坏防护：data 不是合法标签数组时视为无会话，避免前端清空后恢复一半
    // （坏快照在前端 JSON.parse 才爆，那时旧标签已清空找不回来）
    if let Some(ref s) = session {
        if serde_json::from_str::<Vec<SessionTab>>(&s.data).is_err() {
            log::warn!(
                "[sessions] 快照损坏已跳过: id={} workspace_id={:?}",
                s.id,
                s.workspace_id
            );
            return Ok(None);
        }
    }
    Ok(session)
}

/// 列出所有会话（最近 50 条，id 倒序 ≈ 时间倒序）。
/// F-CEF-26：Tauri 命令与 CEF 面板共用的唯一 SQL 实现，两侧不许复制 SQL。
pub async fn list_sessions_inner(pool: &sqlx::SqlitePool) -> DbResult<Vec<Session>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<i64>, String, String)>(
        "SELECT id, name, workspace_id, data, created_at FROM sessions ORDER BY id DESC LIMIT 50",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, workspace_id, data, created_at)| Session {
            id,
            name,
            workspace_id,
            data,
            created_at,
        })
        .collect())
}

/// 列出所有会话
#[tauri::command]
pub async fn list_sessions(db: tauri::State<'_, Db>) -> DbResult<Vec<Session>> {
    list_sessions_inner(&db.pool).await
}

/// 删除会话
#[tauri::command]
pub async fn delete_session(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    delete_session_inner(&db.pool, id).await
}

/// 删除会话核心逻辑（pool 级，便于测试）
pub async fn delete_session_inner(pool: &sqlx::SqlitePool, id: i64) -> DbResult<()> {
    let res = sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    // 假成功防护：id 不存在时如实上报，前端可提示"会话不存在"
    if res.rows_affected() == 0 {
        return Err(crate::db::DbError::Other(format!("会话 {} 不存在", id)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

    /// 建临时数据库（含 sessions 表，模拟迁移 v2 后的结构）
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
            "CREATE TABLE sessions (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              workspace_id INTEGER,
              data TEXT NOT NULL,
              created_at TEXT DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn save_auto_save_keeps_latest_100() {
        let (_dir, pool) = temp_pool().await;
        // 写入 105 条自动保存，应只剩最近 100 条
        for i in 0..105 {
            save_session_inner(
                &pool,
                "自动保存",
                None,
                &[super::SessionTab {
                    url: format!("https://e.com/{}", i),
                    title: None,
                    group_id: None,
                }],
            )
            .await
            .unwrap();
        }
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 100);
        // 保留的是最新的（id 大的一侧）
        let min_id: (i64,) = sqlx::query_as("SELECT MIN(id) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(min_id.0, 6); // 105 - 100 + 1
    }

    #[tokio::test]
    async fn delete_missing_session_reports_error() {
        let (_dir, pool) = temp_pool().await;
        // 假成功防护：删不存在的会话必须报错
        let err = delete_session_inner(&pool, 999).await.unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }

    #[tokio::test]
    async fn delete_existing_session_ok() {
        let (_dir, pool) = temp_pool().await;
        let id = save_session_inner(&pool, "手动会话", None, &[])
            .await
            .unwrap();
        delete_session_inner(&pool, id).await.unwrap();
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn manual_sessions_capped_per_workspace() {
        let (_dir, pool) = temp_pool().await;
        // 默认工作区（NULL）存 55 条手动会话，应只剩 50 条
        for i in 0..55 {
            save_session_inner(&pool, &format!("手动{i}"), None, &[])
                .await
                .unwrap();
        }
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 50);
        // 别的空间不受影响：wid=7 存 3 条，全留
        for i in 0..3 {
            save_session_inner(&pool, &format!("w7-{i}"), Some(7), &[])
                .await
                .unwrap();
        }
        let w7: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE workspace_id = 7")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(w7.0, 3);
        let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total.0, 53);
        // 自动保存规则不受影响：插一条自动保存不触发手动裁剪
        save_session_inner(&pool, "自动保存", None, &[])
            .await
            .unwrap();
        let total2: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(total2.0, 54);
    }

    // ---- F-CEF-26：list_sessions_inner（Tauri 命令与 CEF 面板共用）----

    #[tokio::test]
    async fn list_sessions_empty_db_returns_empty() {
        let (_dir, pool) = temp_pool().await;
        assert!(
            list_sessions_inner(&pool).await.unwrap().is_empty(),
            "空库应返回空数组（CEF 面板空态依赖）"
        );
    }

    #[tokio::test]
    async fn list_sessions_orders_by_id_desc() {
        let (_dir, pool) = temp_pool().await;
        let id1 = save_session_inner(&pool, "a", None, &[]).await.unwrap();
        let id2 = save_session_inner(&pool, "b", None, &[]).await.unwrap();
        let id3 = save_session_inner(&pool, "c", None, &[]).await.unwrap();
        let got = list_sessions_inner(&pool).await.unwrap();
        assert_eq!(got.len(), 3);
        // id 倒序 = 最近创建在前
        assert_eq!(got[0].id, id3);
        assert_eq!(got[1].id, id2);
        assert_eq!(got[2].id, id1);
    }

    #[tokio::test]
    async fn list_sessions_field_roundtrip() {
        let (_dir, pool) = temp_pool().await;
        let tabs = vec![
            super::SessionTab {
                url: "https://example.com/x".to_string(),
                title: Some("标题".to_string()),
                group_id: Some(7),
            },
            super::SessionTab {
                url: "https://e.com/y".to_string(),
                title: None,
                group_id: None,
            },
        ];
        let id = save_session_inner(&pool, "会话名", Some(3), &tabs)
            .await
            .unwrap();
        let got = list_sessions_inner(&pool).await.unwrap();
        assert_eq!(got.len(), 1);
        let s = &got[0];
        assert_eq!(s.id, id);
        assert_eq!(s.name, "会话名");
        assert_eq!(s.workspace_id, Some(3));
        assert!(!s.created_at.is_empty());
        // data 是可解析的标签 JSON 快照，字段原样回读（CEF 侧只取 len 当标签数）
        let parsed: Vec<super::SessionTab> = serde_json::from_str(&s.data).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].url, "https://example.com/x");
        assert_eq!(parsed[0].title.as_deref(), Some("标题"));
        assert_eq!(parsed[0].group_id, Some(7));
        assert_eq!(parsed[1].url, "https://e.com/y");
        assert_eq!(parsed[1].group_id, None);
    }
}

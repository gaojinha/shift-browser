// 会话管理：保存/恢复标签页状态
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Session {
    pub id: i64,
    pub name: String,
    pub workspace_id: Option<i64>,
    /// 标签页 JSON 快照：[{"url":"...","title":"..."}]
    pub data: String,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SessionTab {
    pub url: String,
    pub title: Option<String>,
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
    let data = serde_json::to_string(&tabs)?;
    let result = sqlx::query(
        "INSERT INTO sessions (name, workspace_id, data) VALUES (?, ?, ?)",
    )
    .bind(&name)
    .bind(&workspace_id)
    .bind(&data)
    .execute(&db.pool)
    .await?;
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

    Ok(row.map(|(id, name, workspace_id, data, created_at)| Session {
        id,
        name,
        workspace_id,
        data,
        created_at,
    }))
}

/// 列出所有会话
#[tauri::command]
pub async fn list_sessions(
    db: tauri::State<'_, Db>,
) -> DbResult<Vec<Session>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<i64>, String, String)>(
        "SELECT id, name, workspace_id, data, created_at FROM sessions ORDER BY id DESC LIMIT 50",
    )
    .fetch_all(&db.pool)
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

/// 删除会话
#[tauri::command]
pub async fn delete_session(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}
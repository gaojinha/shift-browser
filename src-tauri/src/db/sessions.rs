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
#[tauri::command]
pub async fn save_session(
    db: tauri::State<'_, Db>,
    name: String,
    tabs: Vec<SessionTab>,
) -> DbResult<i64> {
    let data = serde_json::to_string(&tabs)?;
    let result = sqlx::query(
        "INSERT INTO sessions (name, data) VALUES (?, ?)",
    )
    .bind(&name)
    .bind(&data)
    .execute(&db.pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 加载最近一次会话（启动恢复用）
#[tauri::command]
pub async fn load_last_session(
    db: tauri::State<'_, Db>,
) -> DbResult<Option<Session>> {
    let row = sqlx::query_as::<_, (i64, String, Option<i64>, String, String)>(
        "SELECT id, name, workspace_id, data, created_at FROM sessions ORDER BY id DESC LIMIT 1",
    )
    .fetch_optional(&db.pool)
    .await?;

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
// 工作区管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub created_at: String,
}

/// 创建工作区
#[tauri::command]
pub async fn create_workspace(
    db: tauri::State<'_, Db>,
    name: String,
) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO workspaces (name, sort_order) VALUES (?, COALESCE((SELECT MAX(sort_order) + 1 FROM workspaces), 0))",
    )
    .bind(&name)
    .execute(&db.pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 获取所有工作区
#[tauri::command]
pub async fn get_workspaces(
    db: tauri::State<'_, Db>,
) -> DbResult<Vec<Workspace>> {
    let rows = sqlx::query_as::<_, (i64, String, i64, String)>(
        "SELECT id, name, sort_order, created_at FROM workspaces ORDER BY sort_order, id",
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, sort_order, created_at)| Workspace {
            id,
            name,
            sort_order,
            created_at,
        })
        .collect())
}

/// 更新工作区
#[tauri::command]
pub async fn update_workspace(
    db: tauri::State<'_, Db>,
    id: i64,
    name: Option<String>,
    sort_order: Option<i64>,
) -> DbResult<()> {
    if let Some(name) = name {
        sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    if let Some(sort_order) = sort_order {
        sqlx::query("UPDATE workspaces SET sort_order = ? WHERE id = ?")
            .bind(sort_order)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    Ok(())
}

/// 删除工作区
#[tauri::command]
pub async fn delete_workspace(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}
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
pub async fn create_workspace(db: tauri::State<'_, Db>, name: String) -> DbResult<i64> {
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
pub async fn get_workspaces(db: tauri::State<'_, Db>) -> DbResult<Vec<Workspace>> {
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
    update_workspace_inner(&db.pool, id, name, sort_order).await
}

/// 更新工作区核心逻辑（pool 级，便于测试）
pub async fn update_workspace_inner(
    pool: &sqlx::SqlitePool,
    id: i64,
    name: Option<String>,
    sort_order: Option<i64>,
) -> DbResult<()> {
    // 多字段更新放同一事务，避免部分成功
    let mut tx = pool.begin().await?;
    if let Some(name) = name {
        sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(sort_order) = sort_order {
        sqlx::query("UPDATE workspaces SET sort_order = ? WHERE id = ?")
            .bind(sort_order)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
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

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

    /// 建临时数据库（含 workspaces 表）
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
            "CREATE TABLE workspaces (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              sort_order INTEGER DEFAULT 0,
              created_at TEXT DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
        .await
        .unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn update_workspace_partial_fields_ok() {
        let (_dir, pool) = temp_pool().await;
        let id = sqlx::query("INSERT INTO workspaces (name, sort_order) VALUES ('默认', 0)")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
        // 只更新名字，sort_order 不变
        super::update_workspace_inner(&pool, id, Some("工作区A".into()), None)
            .await
            .unwrap();
        let (name, sort_order): (String, i64) =
            sqlx::query_as("SELECT name, sort_order FROM workspaces WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "工作区A");
        assert_eq!(sort_order, 0);
    }
}

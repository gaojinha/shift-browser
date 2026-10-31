// 工作区管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Workspace {
    pub id: i64,
    pub name: String,
    /// 空间主题色（阶段 3.2）；空字符串 = 跟随默认强调色
    pub color: String,
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
    list_workspaces_inner(&db.pool).await
}

/// 获取所有工作区核心逻辑（pool 级，便于测试；CEF 分组/工作区面板共用，F-CEF-27）
pub async fn list_workspaces_inner(pool: &sqlx::SqlitePool) -> DbResult<Vec<Workspace>> {
    let rows = sqlx::query_as::<_, (i64, String, String, i64, String)>(
        "SELECT id, name, color, sort_order, created_at FROM workspaces ORDER BY sort_order, id",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, color, sort_order, created_at)| Workspace {
            id,
            name,
            color,
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
    color: Option<String>,
) -> DbResult<()> {
    update_workspace_inner(&db.pool, id, name, sort_order, color).await
}

/// 更新工作区核心逻辑（pool 级，便于测试）
pub async fn update_workspace_inner(
    pool: &sqlx::SqlitePool,
    id: i64,
    name: Option<String>,
    sort_order: Option<i64>,
    color: Option<String>,
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
    if let Some(color) = color {
        sqlx::query("UPDATE workspaces SET color = ? WHERE id = ?")
            .bind(color)
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
    delete_workspace_inner(&db.pool, id).await
}

/// 删除工作区核心逻辑（pool 级，便于测试）
/// 同一事务内先清该工作区的会话再删工作区，避免孤儿会话导致“一切换标签全丢且回不来”。
pub async fn delete_workspace_inner(pool: &sqlx::SqlitePool, id: i64) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM sessions WHERE workspace_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let res = sqlx::query("DELETE FROM workspaces WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    // 假成功防护：id 不存在如实上报，前端可提示
    if res.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(crate::db::DbError::Other(format!("工作区 {} 不存在", id)));
    }
    tx.commit().await?;
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
              color TEXT NOT NULL DEFAULT '',
              sort_order INTEGER DEFAULT 0,
              created_at TEXT DEFAULT (datetime('now'))
            )",
        )
        .execute(&pool)
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
    async fn list_workspaces_empty_db_returns_empty() {
        let (_dir, pool) = temp_pool().await;
        let workspaces = super::list_workspaces_inner(&pool).await.unwrap();
        assert!(workspaces.is_empty());
    }

    #[tokio::test]
    async fn list_workspaces_reads_fields_and_sorts() {
        let (_dir, pool) = temp_pool().await;
        // 插反顺序：sort_order 0/2/1，期望按 sort_order 排序回读
        for (name, order) in [("默认", 0), ("工作", 2), ("生活", 1)] {
            sqlx::query("INSERT INTO workspaces (name, sort_order, color) VALUES (?, ?, ?)")
                .bind(name)
                .bind(order)
                .bind(if name == "生活" { "#12ab34" } else { "" })
                .execute(&pool)
                .await
                .unwrap();
        }
        let workspaces = super::list_workspaces_inner(&pool).await.unwrap();
        assert_eq!(
            workspaces
                .iter()
                .map(|w| w.name.as_str())
                .collect::<Vec<_>>(),
            vec!["默认", "生活", "工作"]
        );
        assert_eq!(workspaces[1].color, "#12ab34");
        assert_eq!(workspaces[1].sort_order, 1);
        assert!(!workspaces[1].created_at.is_empty());
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
        super::update_workspace_inner(&pool, id, Some("工作区A".into()), None, None)
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

    #[tokio::test]
    async fn delete_workspace_cleans_orphan_sessions() {
        let (_dir, pool) = temp_pool().await;
        let id = sqlx::query("INSERT INTO workspaces (name, sort_order) VALUES ('待删', 0)")
            .execute(&pool)
            .await
            .unwrap()
            .last_insert_rowid();
        sqlx::query("INSERT INTO sessions (name, workspace_id, data) VALUES ('自动保存', ?, '[]')")
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
        super::delete_workspace_inner(&pool, id).await.unwrap();
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM sessions WHERE workspace_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0);
    }

    #[tokio::test]
    async fn delete_missing_workspace_reports_error() {
        let (_dir, pool) = temp_pool().await;
        let err = super::delete_workspace_inner(&pool, 999).await.unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }
}

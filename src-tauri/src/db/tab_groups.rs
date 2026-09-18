// 标签分组管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TabGroup {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub collapsed: bool,
    pub created_at: String,
}

/// 创建标签分组
#[tauri::command]
pub async fn create_tab_group(
    db: tauri::State<'_, Db>,
    name: String,
    color: Option<String>,
) -> DbResult<i64> {
    let color = color.unwrap_or_else(|| "#4a9eff".to_string());
    let result = sqlx::query("INSERT INTO tab_groups (name, color) VALUES (?, ?)")
        .bind(&name)
        .bind(&color)
        .execute(&db.pool)
        .await?;
    Ok(result.last_insert_rowid())
}

/// 获取所有标签分组
#[tauri::command]
pub async fn get_tab_groups(db: tauri::State<'_, Db>) -> DbResult<Vec<TabGroup>> {
    let rows = sqlx::query_as::<_, (i64, String, String, i64, String)>(
        "SELECT id, name, color, collapsed, created_at FROM tab_groups ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, name, color, collapsed, created_at)| TabGroup {
            id,
            name,
            color,
            collapsed: collapsed != 0,
            created_at,
        })
        .collect())
}

/// 更新标签分组
#[tauri::command]
pub async fn update_tab_group(
    db: tauri::State<'_, Db>,
    id: i64,
    name: Option<String>,
    color: Option<String>,
    collapsed: Option<bool>,
) -> DbResult<()> {
    update_tab_group_inner(&db.pool, id, name, color, collapsed).await
}

/// 更新标签分组核心逻辑（pool 级，便于测试）
pub async fn update_tab_group_inner(
    pool: &sqlx::SqlitePool,
    id: i64,
    name: Option<String>,
    color: Option<String>,
    collapsed: Option<bool>,
) -> DbResult<()> {
    // P2：最多 3 条 UPDATE 放同一事务——中途失败整体回滚，
    // 避免"名字改了、颜色没改"的部分更新
    let mut tx = pool.begin().await?;
    if let Some(name) = name {
        sqlx::query("UPDATE tab_groups SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(color) = color {
        sqlx::query("UPDATE tab_groups SET color = ? WHERE id = ?")
            .bind(color)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(collapsed) = collapsed {
        sqlx::query("UPDATE tab_groups SET collapsed = ? WHERE id = ?")
            .bind(collapsed as i64)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// 删除标签分组
/// 除删组行外，还会清理已保存会话快照里的 groupId 引用（P2）：
/// sessions.data 是 JSON 快照，恢复会话时若 groupId 指向已删分组，
/// 前端按组渲染会失效。删除组后把快照里匹配的 groupId 置空。
#[tauri::command]
pub async fn delete_tab_group(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    delete_tab_group_inner(&db.pool, id).await
}

/// 删除标签分组核心逻辑（pool 级，便于测试）
pub async fn delete_tab_group_inner(pool: &sqlx::SqlitePool, id: i64) -> DbResult<()> {
    let mut tx = pool.begin().await?;
    // 1. 删组行；不存在则报错（假成功防护）
    let res = sqlx::query("DELETE FROM tab_groups WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if res.rows_affected() == 0 {
        tx.rollback().await?;
        return Err(crate::db::DbError::Other(format!("分组 {} 不存在", id)));
    }
    // 2. 清理所有会话快照里指向该分组的 groupId
    let sessions: Vec<(i64, String)> = sqlx::query_as("SELECT id, data FROM sessions")
        .fetch_all(&mut *tx)
        .await?;
    for (sid, data) in sessions {
        // 快照格式：SessionTab 数组（url/title/groupId）。解析失败（旧格式）跳过
        let Ok(mut tabs) = serde_json::from_str::<Vec<serde_json::Value>>(&data) else {
            continue;
        };
        let mut changed = false;
        for tab in tabs.iter_mut() {
            if let Some(g) = tab.get("groupId") {
                if g.as_i64() == Some(id) {
                    // 置 null 保留结构（前端 serde(default) 兼容），等价于未分组
                    tab["groupId"] = serde_json::Value::Null;
                    changed = true;
                }
            }
        }
        if changed {
            let new_data = serde_json::to_string(&tabs)?;
            sqlx::query("UPDATE sessions SET data = ? WHERE id = ?")
                .bind(&new_data)
                .bind(sid)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

    /// 建临时数据库（含 tab_groups + sessions 表）
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
            "CREATE TABLE tab_groups (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              color TEXT NOT NULL,
              collapsed INTEGER DEFAULT 0,
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

    async fn insert_group(pool: &SqlitePool, name: &str) -> i64 {
        sqlx::query("INSERT INTO tab_groups (name, color) VALUES (?, '#fff')")
            .bind(name)
            .execute(pool)
            .await
            .unwrap()
            .last_insert_rowid()
    }

    #[tokio::test]
    async fn update_group_partial_fields_ok() {
        let (_dir, pool) = temp_pool().await;
        let id = insert_group(&pool, "工作").await;
        // 只更新名字，颜色/折叠不变
        super::update_tab_group_inner(&pool, id, Some("新名字".into()), None, None)
            .await
            .unwrap();
        let (name, color, collapsed): (String, String, i64) =
            sqlx::query_as("SELECT name, color, collapsed FROM tab_groups WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(name, "新名字");
        assert_eq!(color, "#fff");
        assert_eq!(collapsed, 0);
    }

    #[tokio::test]
    async fn delete_group_cleans_session_snapshots() {
        let (_dir, pool) = temp_pool().await;
        let g1 = insert_group(&pool, "工作").await;
        let g2 = insert_group(&pool, "娱乐").await;
        // 会话快照：两条 tab 挂 g1，一条挂 g2（旧格式快照带 groupId）
        let data = serde_json::json!([
            {"url": "https://a.com", "title": "A", "groupId": g1},
            {"url": "https://b.com", "title": "B", "groupId": g1},
            {"url": "https://c.com", "title": "C", "groupId": g2}
        ]);
        sqlx::query("INSERT INTO sessions (name, data) VALUES ('快照', ?)")
            .bind(data.to_string())
            .execute(&pool)
            .await
            .unwrap();
        // 删 g1：快照里 g1 引用置 null，g2 引用保留
        super::delete_tab_group_inner(&pool, g1).await.unwrap();
        let stored: (String,) = sqlx::query_as("SELECT data FROM sessions LIMIT 1")
            .fetch_one(&pool)
            .await
            .unwrap();
        let tabs: Vec<serde_json::Value> = serde_json::from_str(&stored.0).unwrap();
        assert_eq!(tabs[0]["groupId"], serde_json::Value::Null);
        assert_eq!(tabs[1]["groupId"], serde_json::Value::Null);
        assert_eq!(tabs[2]["groupId"].as_i64(), Some(g2));
    }

    #[tokio::test]
    async fn delete_missing_group_reports_error() {
        let (_dir, pool) = temp_pool().await;
        let err = super::delete_tab_group_inner(&pool, 999).await.unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }
}

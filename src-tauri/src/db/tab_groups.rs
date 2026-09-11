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
pub async fn get_tab_groups(
    db: tauri::State<'_, Db>,
) -> DbResult<Vec<TabGroup>> {
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
    if let Some(name) = name {
        sqlx::query("UPDATE tab_groups SET name = ? WHERE id = ?")
            .bind(name)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    if let Some(color) = color {
        sqlx::query("UPDATE tab_groups SET color = ? WHERE id = ?")
            .bind(color)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    if let Some(collapsed) = collapsed {
        sqlx::query("UPDATE tab_groups SET collapsed = ? WHERE id = ?")
            .bind(collapsed as i64)
            .bind(id)
            .execute(&db.pool)
            .await?;
    }
    Ok(())
}

/// 删除标签分组
#[tauri::command]
pub async fn delete_tab_group(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM tab_groups WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}
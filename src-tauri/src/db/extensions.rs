// 扩展管理（用户脚本/样式注入）
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Extension {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub created_at: String,
}

/// 列出全部扩展
pub async fn list(db: &Db) -> DbResult<Vec<Extension>> {
    let rows = sqlx::query_as::<_, (i64, String, String, i64, String)>(
        "SELECT id, name, path, enabled, created_at FROM extensions ORDER BY id DESC",
    )
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name, path, enabled, created_at)| Extension {
            id,
            name,
            path,
            enabled: enabled != 0,
            created_at,
        })
        .collect())
}

/// 创建扩展记录
pub async fn create(db: &Db, name: &str, path: &str) -> DbResult<i64> {
    let result = sqlx::query(
        "INSERT INTO extensions (name, path, enabled) VALUES (?, ?, 1)",
    )
    .bind(name)
    .bind(path)
    .execute(&db.pool)
    .await?;
    Ok(result.last_insert_rowid())
}

/// 启用/禁用
pub async fn set_enabled(db: &Db, id: i64, enabled: bool) -> DbResult<()> {
    sqlx::query("UPDATE extensions SET enabled = ? WHERE id = ?")
        .bind(if enabled { 1 } else { 0 })
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 删除扩展
pub async fn delete(db: &Db, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM extensions WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 查询单个扩展（不存在返回 None）
pub async fn get(db: &Db, id: i64) -> DbResult<Option<Extension>> {
    let row = sqlx::query_as::<_, (i64, String, String, i64, String)>(
        "SELECT id, name, path, enabled, created_at FROM extensions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&db.pool)
    .await?;
    Ok(row.map(|(id, name, path, enabled, created_at)| Extension {
        id,
        name,
        path,
        enabled: enabled != 0,
        created_at,
    }))
}
// 设置管理
use crate::db::{Db, DbError, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Setting {
  pub key: String,
  pub value: String,
}

/// 获取单个设置
#[tauri::command]
pub async fn get_setting(db: tauri::State<'_, Db>, key: String) -> DbResult<Option<String>> {
  let row: Option<(String,)> =
    sqlx::query_as("SELECT value FROM settings WHERE key = ?")
      .bind(&key)
      .fetch_optional(&db.pool)
      .await?;
  Ok(row.map(|r| r.0))
}

/// 获取所有设置
#[tauri::command]
pub async fn get_all_settings(db: tauri::State<'_, Db>) -> DbResult<Vec<Setting>> {
  let rows: Vec<(String, String)> =
    sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
      .fetch_all(&db.pool)
      .await?;
  Ok(rows
    .into_iter()
    .map(|(key, value)| Setting { key, value })
    .collect())
}

/// 设置单个值
#[tauri::command]
pub async fn set_setting(
  db: tauri::State<'_, Db>,
  key: String,
  value: String,
) -> DbResult<()> {
  sqlx::query(
    "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
  )
  .bind(&key)
  .bind(&value)
  .execute(&db.pool)
  .await?;
  Ok(())
}

/// 删除设置
#[tauri::command]
pub async fn delete_setting(db: tauri::State<'_, Db>, key: String) -> DbResult<()> {
  sqlx::query("DELETE FROM settings WHERE key = ?")
    .bind(&key)
    .execute(&db.pool)
    .await?;
  Ok(())
}

/// 批量设置（用于导入配置）
#[tauri::command]
pub async fn set_settings_bulk(
  db: tauri::State<'_, Db>,
  settings: Vec<Setting>,
) -> DbResult<()> {
  let mut tx = db.pool.begin().await?;
  for s in settings {
    sqlx::query(
      "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(&s.key)
    .bind(&s.value)
    .execute(&mut *tx)
    .await?;
  }
  tx.commit().await?;
  Ok(())
}

impl DbError {
  pub fn not_found() -> Self {
    DbError::NotFound
  }
}
// 设置管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Setting {
  pub key: String,
  pub value: String,
}

/// 敏感设置键（禁止返回/写入前端）
const SENSITIVE_KEYS: &[&str] = &["master_key"];

/// 是否为敏感设置键
fn is_sensitive(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key)
}

/// API Key 掩码（保留前 4 后 4，中间 ****；太短则全掩）
fn mask_api_key(v: &str) -> String {
    let v = v.trim();
    if v.len() <= 8 {
        "****".to_string()
    } else {
        format!("{}****{}", &v[..4], &v[v.len() - 4..])
    }
}

/// 获取单个设置
#[tauri::command]
pub async fn get_setting(db: tauri::State<'_, Db>, key: String) -> DbResult<Option<String>> {
  // 主密钥等敏感设置不允许前端读取
  if is_sensitive(&key) {
    return Ok(None);
  }
  let row: Option<(String,)> =
    sqlx::query_as("SELECT value FROM settings WHERE key = ?")
      .bind(&key)
      .fetch_optional(&db.pool)
      .await?;
  Ok(row.map(|r| r.0))
}

/// 获取所有设置（敏感项脱敏：master_key 不返回，ai_api_key 返回掩码）
#[tauri::command]
pub async fn get_all_settings(db: tauri::State<'_, Db>) -> DbResult<Vec<Setting>> {
  let rows: Vec<(String, String)> =
    sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
      .fetch_all(&db.pool)
      .await?;
  Ok(rows
    .into_iter()
    .filter(|(key, _)| !is_sensitive(key))
    .map(|(key, value)| Setting {
      key: key.clone(),
      value: if key == "ai_api_key" && !value.is_empty() {
        mask_api_key(&value)
      } else {
        value
      },
    })
    .collect())
}

/// 设置单个值
#[tauri::command]
pub async fn set_setting(
  db: tauri::State<'_, Db>,
  key: String,
  value: String,
) -> DbResult<()> {
  // 主密钥等敏感设置不允许前端写入
  if is_sensitive(&key) {
    return Err(crate::db::DbError::Other(format!(
      "设置 {} 受保护，不允许通过前端修改",
      key
    )));
  }
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
    // 敏感设置不允许批量写入
    if is_sensitive(&s.key) {
      continue;
    }
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
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

/// 前端允许读写的设置键白名单（防止非法键写入垃圾数据）
const ALLOWED_KEYS: &[&str] = &[
  "homepage",
  "search_engine",
  "theme",
  "show_bookmarks_bar",
  "record_history",
  "ai_model",
  "ai_base_url",
  "ai_api_key",
  "adblock_enabled",
  "adblock_keywords",
];

/// 是否为敏感设置键
fn is_sensitive(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key)
}

/// 是否为白名单允许的键
fn is_allowed(key: &str) -> bool {
    ALLOWED_KEYS.contains(&key)
}

/// API Key 掩码（保留前 4 后 4，中间 ****；太短则全掩）
/// 按字符操作而非字节切片——多字节字符（中文等）在字节边界切片会 panic
fn mask_api_key(v: &str) -> String {
    let v = v.trim();
    let cs: Vec<char> = v.chars().collect();
    if cs.len() <= 8 {
        "****".to_string()
    } else {
        let head: String = cs[..4].iter().collect();
        let tail: String = cs[cs.len() - 4..].iter().collect();
        format!("{}****{}", head, tail)
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
  // 与 get_all_settings 的脱敏契约一致：ai_api_key 返回掩码，明文 key 不出库
  Ok(row.map(|r| {
    if key == "ai_api_key" && !r.0.is_empty() {
      mask_api_key(&r.0)
    } else {
      r.0
    }
  }))
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
  // 白名单校验：未知键拒绝写入，防止垃圾数据/错别字键
  if !is_allowed(&key) {
    return Err(crate::db::DbError::Other(format!(
      "设置 {} 不在白名单内，拒绝写入",
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
    // 白名单校验：未知键跳过（批量导入容忍脏数据，单个 set 才报错）
    if !is_allowed(&s.key) {
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
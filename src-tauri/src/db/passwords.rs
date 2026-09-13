// 密码管理（AES-256-GCM 加密存储）
// 主密钥存独立密钥文件（app_data/master.key，权限 600），与数据库分离——
// 即使数据库文件泄露，没有密钥文件也无法解密密码。
// 每条密码：随机 12 字节 nonce + 密文，base64 编码后存 password 字段
use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 写入主密钥文件（权限 600，仅当前用户可读写）
fn write_key_file(path: &Path, encoded: &str) -> DbResult<()> {
    std::fs::write(path, encoded)
        .map_err(|e| DbError::Other(format!("写入主密钥文件失败: {}", e)))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// 获取主密钥（不存在则生成随机 32 字节并写入独立密钥文件）
/// 迁移兼容：旧版本主密钥存在 settings 表（key=master_key），首次启动时
/// 自动迁移到密钥文件并删除数据库里的明文残留。
async fn get_master_key(db: &Db) -> DbResult<[u8; 32]> {
    // 1. 优先从密钥文件读取
    if let Ok(encoded) = std::fs::read_to_string(&db.key_path) {
        let encoded = encoded.trim();
        if let Ok(bytes) = BASE64.decode(encoded) {
            if bytes.len() == 32 {
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }
    }

    // 2. 文件不存在 → 尝试迁移旧值（settings 表 master_key）
    let legacy: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
            .fetch_optional(&db.pool)
            .await?;
    if let Some(encoded) = legacy {
        let encoded = encoded.trim().to_string();
        if let Ok(bytes) = BASE64.decode(&encoded) {
            if bytes.len() == 32 {
                write_key_file(&db.key_path, &encoded)?;
                // 删除数据库里的明文密钥残留
                sqlx::query("DELETE FROM settings WHERE key = 'master_key'")
                    .execute(&db.pool)
                    .await?;
                println!("[ShiftBrowser] 主密钥已迁移到独立文件并清理数据库残留");
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                return Ok(key);
            }
        }
    }

    // 3. 都没有 → 生成新主密钥
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let encoded = BASE64.encode(key);
    write_key_file(&db.key_path, &encoded)?;
    Ok(key)
}

/// 加密明文
fn encrypt(key: &[u8; 32], plaintext: &str) -> DbResult<String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| DbError::Other(format!("创建加密器失败: {}", e)))?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| DbError::Other(format!("加密失败: {}", e)))?;
    // 存储格式：nonce + ciphertext，base64
    let mut combined = nonce_bytes.to_vec();
    combined.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(combined))
}

/// 解密密文
fn decrypt(key: &[u8; 32], encoded: &str) -> DbResult<String> {
    let combined = BASE64
        .decode(encoded)
        .map_err(|e| DbError::Other(format!("密文解码失败: {}", e)))?;
    if combined.len() < 12 {
        return Err(DbError::Other("密文长度无效".to_string()));
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| DbError::Other(format!("创建解密器失败: {}", e)))?;
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| DbError::Other(format!("解密失败: {}", e)))?;
    String::from_utf8(plaintext).map_err(|e| DbError::Other(format!("明文解码失败: {}", e)))
}

/// 保存密码（加密后存储）
#[tauri::command]
pub async fn save_password(
    db: tauri::State<'_, Db>,
    domain: String,
    username: Option<String>,
    password: String,
    notes: Option<String>,
) -> DbResult<i64> {
    let key = get_master_key(&db).await?;
    let encrypted = encrypt(&key, &password)?;

    // 同 domain+username 存在则更新
    let existing: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM passwords WHERE domain = ? AND username IS ?",
    )
    .bind(&domain)
    .bind(&username)
    .fetch_optional(&db.pool)
    .await?;

    if let Some(id) = existing {
        sqlx::query(
            "UPDATE passwords SET password = ?, notes = ?, updated_at = datetime('now') WHERE id = ?",
        )
        .bind(&encrypted)
        .bind(&notes)
        .bind(id)
        .execute(&db.pool)
        .await?;
        Ok(id)
    } else {
        let result = sqlx::query(
            "INSERT INTO passwords (domain, username, password, notes) VALUES (?, ?, ?, ?)",
        )
        .bind(&domain)
        .bind(&username)
        .bind(&encrypted)
        .bind(&notes)
        .execute(&db.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }
}

/// 获取密码（解密返回）
#[tauri::command]
pub async fn get_password(
    db: tauri::State<'_, Db>,
    domain: String,
) -> DbResult<Option<PasswordEntry>> {
    let row = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, String, String)>(
        "SELECT id, domain, username, password, notes, created_at, updated_at FROM passwords WHERE domain = ? LIMIT 1",
    )
    .bind(&domain)
    .fetch_optional(&db.pool)
    .await?;

    let Some((id, domain, username, encrypted, notes, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let key = get_master_key(&db).await?;
    let plaintext = decrypt(&key, &encrypted)?;
    Ok(Some(PasswordEntry {
        id,
        domain,
        username,
        password: plaintext,
        notes,
        created_at,
        updated_at,
    }))
}

/// 按 id 获取单条密码（解密返回）。同域名多账号时前端用 id 精确定位
#[tauri::command]
pub async fn get_password_by_id(
    db: tauri::State<'_, Db>,
    id: i64,
) -> DbResult<Option<PasswordEntry>> {
    let row = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, String, String)>(
        "SELECT id, domain, username, password, notes, created_at, updated_at FROM passwords WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(&db.pool)
    .await?;

    let Some((id, domain, username, encrypted, notes, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let key = get_master_key(&db).await?;
    let plaintext = decrypt(&key, &encrypted)?;
    Ok(Some(PasswordEntry {
        id,
        domain,
        username,
        password: plaintext,
        notes,
        created_at,
        updated_at,
    }))
}

/// 获取某域名下所有密码记录（解密）
#[tauri::command]
pub async fn get_passwords_for_domain(
    db: tauri::State<'_, Db>,
    domain: String,
) -> DbResult<Vec<PasswordEntry>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, String, String)>(
        "SELECT id, domain, username, password, notes, created_at, updated_at FROM passwords WHERE domain LIKE ? ORDER BY id",
    )
    .bind(format!("%{}%", domain))
    .fetch_all(&db.pool)
    .await?;

    let key = get_master_key(&db).await?;
    let mut result = Vec::new();
    for (id, domain, username, encrypted, notes, created_at, updated_at) in rows {
        let plaintext = decrypt(&key, &encrypted)?;
        result.push(PasswordEntry {
            id,
            domain,
            username,
            password: plaintext,
            notes,
            created_at,
            updated_at,
        });
    }
    Ok(result)
}

/// 列出所有密码记录（不含明文，前端展示列表用）
#[tauri::command]
pub async fn list_passwords(
    db: tauri::State<'_, Db>,
) -> DbResult<Vec<PasswordMeta>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, String, String)>(
        "SELECT id, domain, username, notes, created_at, updated_at FROM passwords ORDER BY domain, id",
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(id, domain, username, notes, created_at, updated_at)| PasswordMeta {
            id,
            domain,
            username,
            notes,
            created_at,
            updated_at,
        })
        .collect())
}

/// 删除密码记录
#[tauri::command]
pub async fn delete_password(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM passwords WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 密码元信息（不含明文）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PasswordMeta {
    pub id: i64,
    pub domain: String,
    pub username: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 解密后的密码条目（返回给前端）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PasswordEntry {
    pub id: i64,
    pub domain: String,
    pub username: Option<String>,
    pub password: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[cfg(test)]
mod tests {
  use super::*;
  use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

  /// 建临时数据库（含 settings 表）
  async fn temp_db() -> (tempfile::TempDir, Db) {
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
      "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT)",
    )
    .execute(&pool)
    .await
    .unwrap();
    let db = Db {
      pool,
      key_path: dir.path().join("master.key"),
    };
    (dir, db)
  }

  #[test]
  fn test_encrypt_decrypt_roundtrip() {
    let key = [7u8; 32];
    let plaintext = "MySecret123!@#";
    let encrypted = encrypt(&key, plaintext).unwrap();
    assert_ne!(encrypted, plaintext, "密文不应等于明文");
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(decrypted, plaintext, "解密应还原明文");
  }

  #[test]
  fn test_encrypt_random_nonce() {
    let key = [7u8; 32];
    let e1 = encrypt(&key, "same").unwrap();
    let e2 = encrypt(&key, "same").unwrap();
    assert_ne!(e1, e2, "每次加密应使用随机 nonce，密文不同");
  }

  #[test]
  fn test_decrypt_wrong_key_fails() {
    let key1 = [1u8; 32];
    let key2 = [2u8; 32];
    let encrypted = encrypt(&key1, "secret").unwrap();
    let result = decrypt(&key2, &encrypted);
    assert!(result.is_err(), "错误密钥解密应失败");
  }

  #[tokio::test]
  async fn test_master_key_migrates_from_settings_to_file() {
    let (_dir, db) = temp_db().await;
    // 模拟旧版本：master_key 在 settings 表
    let legacy_key = [9u8; 32];
    let encoded = BASE64.encode(legacy_key);
    sqlx::query("INSERT INTO settings (key, value) VALUES ('master_key', ?)")
      .bind(&encoded)
      .execute(&db.pool)
      .await
      .unwrap();

    // 首次调用应迁移：生成密钥文件 + 删除 settings 残留
    let key = get_master_key(&db).await.unwrap();
    assert_eq!(key, legacy_key, "迁移后应沿用旧密钥（不重新生成）");

    let file_content = std::fs::read_to_string(&db.key_path).unwrap();
    assert_eq!(file_content.trim(), encoded, "密钥文件内容应与旧值一致");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mode = std::fs::metadata(&db.key_path).unwrap().permissions().mode();
      assert_eq!(mode & 0o777, 0o600, "密钥文件权限应为 600");
    }

    let leftover: Option<String> =
      sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
        .fetch_optional(&db.pool)
        .await
        .unwrap();
    assert!(leftover.is_none(), "settings 表不应再残留 master_key");

    // 再次调用应从文件读取，结果一致
    let key2 = get_master_key(&db).await.unwrap();
    assert_eq!(key2, legacy_key);
  }

  #[tokio::test]
  async fn test_master_key_generates_new_when_absent() {
    let (_dir, db) = temp_db().await;
    let key = get_master_key(&db).await.unwrap();
    assert_eq!(key.len(), 32);
    assert!(db.key_path.exists(), "密钥文件应已生成");
    // 无旧值时不应写 settings 表
    let leftover: Option<String> =
      sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
        .fetch_optional(&db.pool)
        .await
        .unwrap();
    assert!(leftover.is_none());
  }
}

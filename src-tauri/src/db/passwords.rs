// 密码管理（AES-256-GCM 加密存储）
// 主密钥存 settings 表（key = master_key），首次使用时生成随机 32 字节
// 每条密码：随机 12 字节 nonce + 密文，base64 编码后存 password 字段
use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredPassword {
    pub id: i64,
    pub domain: String,
    pub username: Option<String>,
    /// 加密后的密码（密文）
    pub password: String,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 获取主密钥（不存在则生成随机 32 字节并存入 settings）
async fn get_master_key(db: &Db) -> DbResult<[u8; 32]> {
    let existing: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
            .fetch_optional(&db.pool)
            .await?;

    if let Some(encoded) = existing {
        let bytes = BASE64
            .decode(&encoded)
            .map_err(|e| DbError::Other(format!("主密钥解码失败: {}", e)))?;
        if bytes.len() == 32 {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }

    // 生成新主密钥
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let encoded = BASE64.encode(key);
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES ('master_key', ?)")
        .bind(&encoded)
        .execute(&db.pool)
        .await?;
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

/// 删除密码记录
#[tauri::command]
pub async fn delete_password(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM passwords WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
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
}

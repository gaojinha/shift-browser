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
/// 写入主密钥文件（权限 600）
///
/// 安全：必须用 OpenOptions::mode(0o600) 在创建时就收紧权限，不能用 std::fs::write ——
/// 后者先按进程 umask 落盘（常见 644）、再补 chmod，中间存在同机其他用户可读的窗口。
/// 注意：本函数是"覆盖写"语义（truncate），生产路径已改用 write_key_file_create_new
/// （create_new 防并发双写）；此处保留仅供测试验证权限收紧行为。
#[cfg(test)]
fn write_key_file(path: &Path, encoded: &str) -> DbResult<()> {
    let write_err = |e: std::io::Error| DbError::Other(format!("写入主密钥文件失败: {}", e));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(write_err)?;
        f.write_all(encoded.as_bytes()).map_err(write_err)?;
        f.sync_all().map_err(write_err)?;
        // mode 仅在创建时生效；若文件早已存在（权限可能被放宽过），补一次收紧兜底
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, encoded).map_err(write_err)?;
    }
    Ok(())
}

/// 读取主密钥文件并校验（文件存在但非法 → 报错停用，绝不静默覆盖/重生）
fn read_key_file(path: &Path) -> DbResult<[u8; 32]> {
    let encoded = std::fs::read_to_string(path).map_err(|e| {
        DbError::Other(format!(
            "主密钥文件读取失败（{}），密码功能已停用。请检查 {} 权限或从备份恢复",
            e,
            path.display()
        ))
    })?;
    let encoded = encoded.trim();
    let bytes = BASE64.decode(encoded).map_err(|_| {
        DbError::Other(format!(
            "主密钥文件格式损坏，密码功能已停用。请从备份恢复 {}",
            path.display()
        ))
    })?;
    if bytes.len() != 32 {
        return Err(DbError::Other(format!(
            "主密钥文件内容非法（长度 {}），密码功能已停用。请从备份恢复 {}",
            bytes.len(),
            path.display()
        )));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// 原子创建主密钥文件（create_new：并发首用时只有一个成功，其余读回已有文件）。
/// 权限 600，创建即收紧（OpenOptions.mode 无 644 窗口期）。
fn write_key_file_create_new(path: &Path, encoded: &str) -> DbResult<()> {
    let create_err = |e: std::io::Error| DbError::Other(format!("创建主密钥文件失败: {}", e));
    let write_err = |e: std::io::Error| DbError::Other(format!("写入主密钥文件失败: {}", e));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(create_err)?;
        f.write_all(encoded.as_bytes()).map_err(write_err)?;
        f.sync_all().map_err(write_err)?;
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(create_err)?;
        f.write_all(encoded.as_bytes()).map_err(write_err)?;
        f.sync_all().map_err(write_err)?;
    }
    Ok(())
}

/// 对外暴露主密钥获取（供 db::vault 设置主密码时包裹用）。
pub(crate) async fn master_key(db: &Db) -> DbResult<[u8; 32]> {
    get_master_key(db).await
}

/// 获取主密钥（不存在则生成随机 32 字节并写入独立密钥文件）
/// 迁移兼容：旧版本主密钥存在 settings 表（key=master_key），首次启动时
/// 自动迁移到密钥文件并删除数据库里的明文残留。
/// 注意：密钥文件存在但损坏/无法读取时**绝不静默覆盖**——否则存量密码全部不可解密。
/// 并发安全：Db.master_key 的 tokio Mutex 串行化"检查-生成-写入"整段，
/// 落盘用 create_new 原子创建——并发首用不会各自生成不同密钥互相覆盖。
async fn get_master_key(db: &Db) -> DbResult<[u8; 32]> {
    // 主密码保护（阶段 2）：密钥只来自解锁会话（内存），绝不从磁盘明文读取。
    // 未解锁 → 直接拒绝，所有密码操作都会被拦在这里（单一闸门）。
    if crate::db::vault::is_protected(db) {
        return crate::db::vault::unlocked_key().ok_or_else(|| {
            DbError::Other("密码库已锁定，请先输入主密码解锁".to_string())
        });
    }
    // 串行化整段（含 await 点），防止并发首用各自生成不同密钥互相覆盖
    let mut cache = db.master_key.lock().await;
    if let Some(k) = *cache {
        return Ok(k);
    }

    // 1. 优先从密钥文件读取；文件存在但非法 → 报错停用，不重新生成
    if db.key_path.exists() {
        let key = read_key_file(&db.key_path)?;
        *cache = Some(key);
        return Ok(key);
    }

    // 2. 文件不存在 → 尝试迁移旧值（settings 表 master_key）
    let legacy: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'master_key'")
            .fetch_optional(&db.pool)
            .await?;
    if let Some(encoded) = legacy {
        let encoded = encoded.trim().to_string();
        match BASE64.decode(&encoded) {
            Ok(bytes) if bytes.len() == 32 => {
                // 原子创建写文件（并发窗口内文件已存在则读回已有文件）
                match write_key_file_create_new(&db.key_path, &encoded) {
                    Ok(()) => {}
                    Err(_) if db.key_path.exists() => {
                        // 并发窗口：另一调用者已写入，读回其密钥（绝不覆盖）
                        let existing = read_key_file(&db.key_path)?;
                        *cache = Some(existing);
                        return Ok(existing);
                    }
                    Err(e) => return Err(e),
                }
                // 删除数据库里的明文密钥残留
                sqlx::query("DELETE FROM settings WHERE key = 'master_key'")
                    .execute(&db.pool)
                    .await?;
                log::info!("[ShiftBrowser] 主密钥已迁移到独立文件并清理数据库残留");
                let mut key = [0u8; 32];
                key.copy_from_slice(&bytes);
                *cache = Some(key);
                return Ok(key);
            }
            _ => {
                // 遗留值存在但非法：绝不静默重生（否则存量密码全部不可解密），
                // 与密钥文件损坏的处理保持一致——报错停用，要求人工处置
                return Err(DbError::Other(
          "settings 表遗留 master_key 格式非法（编码或长度错误），密码功能已停用。请从备份恢复。".to_string(),
        ));
            }
        }
    }

    // 3. 都没有 → 生成新主密钥（create_new 原子落盘；并发窗口内文件已存在则读回）
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let encoded = BASE64.encode(key);
    match write_key_file_create_new(&db.key_path, &encoded) {
        Ok(()) => {
            *cache = Some(key);
            Ok(key)
        }
        Err(_) if db.key_path.exists() => {
            // 并发窗口：另一调用者已写入，读回其密钥（绝不覆盖）
            let existing = read_key_file(&db.key_path)?;
            *cache = Some(existing);
            Ok(existing)
        }
        Err(e) => Err(e),
    }
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

    // UPSERT：靠迁移 7 的 (domain, username) 唯一索引原子去重。
    // 原实现是先 SELECT 判重再 INSERT/UPDATE，两步之间有竞态窗口——
    // 并发保存同一 domain+username 时可能 INSERT 出重复行（迁移 7 清理过了历史重复，
    // 现在唯一索引保证不会再有；但 save_password 自身必须也对齐 UPSERT，
    // 否则 UNIQUE 违反直接报错而不是静默去重）。
    // 注意：username 为 NULL 时 SQLite 不触发唯一冲突（NULL 之间不相等），
    // 走 INSERT 分支，允许 NULL username 重复——这是有意的（空白 username 不设约束）。
    sqlx::query(
        "INSERT INTO passwords (domain, username, password, notes) VALUES (?, ?, ?, ?)
         ON CONFLICT(domain, username) DO UPDATE SET
           password = excluded.password,
           notes = excluded.notes,
           updated_at = datetime('now')",
    )
    .bind(&domain)
    .bind(&username)
    .bind(&encrypted)
    .bind(&notes)
    .execute(&db.pool)
    .await?;

    Ok(0) // UPSERT 不返回可靠 rowid，调用方（前端）以 domain+username 查询为准
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
pub async fn list_passwords(db: tauri::State<'_, Db>) -> DbResult<Vec<PasswordMeta>> {
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, String, String)>(
        "SELECT id, domain, username, notes, created_at, updated_at FROM passwords ORDER BY domain, id",
    )
    .fetch_all(&db.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(
            |(id, domain, username, notes, created_at, updated_at)| PasswordMeta {
                id,
                domain,
                username,
                notes,
                created_at,
                updated_at,
            },
        )
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
            vault_path: dir.path().join("vault.key"),
            master_key: tokio::sync::Mutex::new(None),
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
            let mode = std::fs::metadata(&db.key_path)
                .unwrap()
                .permissions()
                .mode();
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

    /// P0-1：并发首次生成主密钥必须得到同一密钥（tokio Mutex 串行化 + create_new 原子落盘）
    #[tokio::test]
    async fn test_master_key_concurrent_generation_same_key() {
        let (_dir, db) = temp_db().await;
        let db = std::sync::Arc::new(db);
        let db1 = db.clone();
        let db2 = db.clone();
        let (k1, k2) = tokio::join!(async { get_master_key(&db1).await.unwrap() }, async {
            get_master_key(&db2).await.unwrap()
        },);
        assert_eq!(
            k1, k2,
            "并发首次生成必须得到同一密钥（不能各自生成互相覆盖）"
        );
        assert!(db.key_path.exists(), "密钥文件应已生成");
        let file_key = read_key_file(&db.key_path).unwrap();
        assert_eq!(k1, file_key, "文件内容应与返回密钥一致");
    }

    /// P1-7：settings 表遗留 master_key 非法时绝不静默重生（否则存量密码全部不可解密）
    #[tokio::test]
    async fn test_master_key_legacy_invalid_errors() {
        let (_dir, db) = temp_db().await;
        // 模拟旧版本：master_key 在 settings 表但格式非法（长度不对）
        sqlx::query("INSERT INTO settings (key, value) VALUES ('master_key', ?)")
            .bind("not-a-valid-32-byte-key")
            .execute(&db.pool)
            .await
            .unwrap();
        let err = get_master_key(&db).await.unwrap_err();
        assert!(
            err.to_string().contains("停用"),
            "非法遗留值应报错停用而非静默重生: {}",
            err
        );
        assert!(!db.key_path.exists(), "非法遗留值不应生成新密钥文件");
    }

    /// P2-5：密钥文件损坏时绝不静默覆盖（原行为：重新生成新密钥导致存量密码全废）
    #[tokio::test]
    async fn test_master_key_corrupt_file_errors() {
        let (_dir, db) = temp_db().await;
        std::fs::write(&db.key_path, "corrupted-content").unwrap();
        let err = get_master_key(&db).await.unwrap_err();
        assert!(
            err.to_string().contains("停用"),
            "损坏的密钥文件应报错停用而非静默覆盖: {}",
            err
        );
        assert_eq!(
            std::fs::read_to_string(&db.key_path).unwrap(),
            "corrupted-content",
            "损坏文件不应被覆盖"
        );
    }

    /// P3-2：密钥文件权限必须"创建即 600"，且覆盖写后收紧回 600
    #[test]
    fn test_write_key_file_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("master.key");

        // 1. 新建：内容写入且权限 600（OpenOptions.mode 在创建时即生效，无 644 窗口期）
        write_key_file(&path, "first-value").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first-value");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "新建密钥文件应立即为 600"
            );
        }

        // 2. 文件已被放宽到 644 时覆盖写：内容更新且权限收紧回 600
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        write_key_file(&path, "second-value").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second-value");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600,
                "覆盖写后应收紧回 600"
            );
        }
    }
}

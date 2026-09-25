//! 密码库密钥与加解密原语（R1 拆分 · 2026-09-24）
//!
//! 主密钥存独立密钥文件（app_data/master.key，权限 600），与数据库分离——
//! 即使数据库文件泄露，没有密钥文件也无法解密密码。
//! 每条密码：随机 12 字节 nonce + 密文，base64 编码后存 password 字段。
//!
//! 单测在同目录 `password_crypto/tests.rs`（密钥迁移/并发首用/损坏停用/权限 600）。

#[cfg(test)]
mod tests;

use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use std::path::Path;

/// 写入主密钥文件（权限 600，仅当前用户可读写）
///
/// 安全：必须用 OpenOptions::mode(0o600) 在创建时就收紧权限，不能用 std::fs::write ——
/// 后者先按进程 umask 落盘（常见 644）、再补 chmod，中间存在同机其他用户可读的窗口。
/// 注意：本函数是"覆盖写"语义（truncate），生产路径已改用 write_key_file_create_new
/// （create_new 防并发双写）；此处保留仅供测试验证权限收紧行为。
#[cfg(test)]
pub(super) fn write_key_file(path: &Path, encoded: &str) -> DbResult<()> {
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
pub(super) fn read_key_file(path: &Path) -> DbResult<[u8; 32]> {
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
pub(super) async fn get_master_key(db: &Db) -> DbResult<[u8; 32]> {
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
pub(super) fn encrypt(key: &[u8; 32], plaintext: &str) -> DbResult<String> {
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
pub(super) fn decrypt(key: &[u8; 32], encoded: &str) -> DbResult<String> {
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

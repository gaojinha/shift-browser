//! 密码库主密码（阶段 2 · 数据主权底线）
//!
//! 现状：`master.key` 是机器自动生成的密钥文件（权限 600），只防"数据库文件泄露"
//! ——同机拿到 `master.key` 即可解密全部密码。
//!
//! 本模块补上**用户主密码**：
//! - 用主密码经 PBKDF2-HMAC-SHA256（随机 salt）派生包裹密钥
//! - 用包裹密钥以 AES-256-GCM 加密「真正的主密钥」，存 `vault.key`（权限 600）
//! - 设了主密码后，密码库启动即**锁定**：必须先 `unlock_vault` 解锁，
//!   主密钥才在内存中可用——同机拿到文件也无法解密
//!
//! 存储格式（vault.key，base64）：salt(16) || nonce(12) || AES-GCM 密文
//! 与 `master.key` 分离：vault.key 只包裹主密钥，不直接存密码。

use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tauri::State;

/// PBKDF2 迭代次数（OWASP 对 PBKDF2-HMAC-SHA256 建议 ≥ 600k；
/// 取 200k 平衡桌面解锁延迟与抗暴力）
const PBKDF2_ROUNDS: u32 = 200_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// 主密码最短长度
const MIN_PASSWORD_LEN: usize = 8;

/// 内存中的解锁会话（Some = 已解锁，持有主密钥）
static UNLOCKED: OnceLock<Mutex<Option<[u8; KEY_LEN]>>> = OnceLock::new();

fn unlocked_cell() -> &'static Mutex<Option<[u8; KEY_LEN]>> {
    UNLOCKED.get_or_init(|| Mutex::new(None))
}

/// 是否已设主密码（vault.key 存在）
pub fn is_protected(db: &Db) -> bool {
    db.vault_path.exists()
}

/// 当前会话是否已解锁
pub fn is_unlocked() -> bool {
    unlocked_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

/// 取解锁后的主密钥（未解锁返回 None）
pub fn unlocked_key() -> Option<[u8; KEY_LEN]> {
    *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner())
}

/// 锁定：清除内存中的主密钥
pub fn lock() {
    *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn set_unlocked(key: [u8; KEY_LEN]) {
    *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
}

/// PBKDF2-HMAC-SHA256 派生包裹密钥
pub fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let mut out = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(password.as_bytes(), salt, PBKDF2_ROUNDS, &mut out);
    out
}

/// 包裹主密钥 → base64(salt || nonce || 密文)
pub fn wrap_key(master: &[u8; KEY_LEN], password: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let dk = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&dk).expect("32 字节密钥必合法");
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), master.as_ref())
        .expect("AES-GCM 加密不应失败");
    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ct.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    BASE64.encode(blob)
}

/// 解包主密钥（主密码错误返回 Err）
pub fn unwrap_key(encoded: &str, password: &str) -> Result<[u8; KEY_LEN], String> {
    let blob = BASE64
        .decode(encoded.trim())
        .map_err(|e| format!("主密码文件格式非法: {}", e))?;
    if blob.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err("主密码文件长度不足".to_string());
    }
    let salt = &blob[..SALT_LEN];
    let nonce = &blob[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ct = &blob[SALT_LEN + NONCE_LEN..];
    let dk = derive_key(password, salt);
    let cipher = Aes256Gcm::new_from_slice(&dk).map_err(|e| format!("密钥派生失败: {}", e))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "主密码错误".to_string())?;
    if pt.len() != KEY_LEN {
        return Err("解出的主密钥长度非法".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&pt);
    Ok(key)
}

/// 写入 vault.key（权限 600，create_new 防并发双写）
fn write_vault_file(path: &Path, encoded: &str) -> DbResult<()> {
    let map_err = |e: std::io::Error| DbError::Other(format!("写入主密码文件失败: {}", e));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(map_err)?;
        f.write_all(encoded.as_bytes()).map_err(map_err)?;
        f.sync_all().map_err(map_err)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, encoded).map_err(map_err)
    }
}

fn read_vault_file(path: &Path) -> DbResult<String> {
    std::fs::read_to_string(path)
        .map_err(|e| DbError::Other(format!("读取主密码文件失败: {}", e)))
}

/// 密码库状态
#[derive(Debug, Serialize, Clone)]
pub struct VaultStatus {
    /// 是否已设主密码
    pub protected: bool,
    /// 当前会话是否已解锁
    pub unlocked: bool,
}

/// 查询密码库状态
#[tauri::command]
pub async fn vault_status(db: State<'_, Db>) -> Result<VaultStatus, String> {
    Ok(VaultStatus {
        protected: is_protected(&db),
        unlocked: is_unlocked(),
    })
}

/// 设置主密码（把当前主密钥用主密码包裹存起来）
#[tauri::command]
pub async fn set_vault_password(db: State<'_, Db>, password: String) -> Result<(), String> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!("主密码至少 {} 位", MIN_PASSWORD_LEN));
    }
    if is_protected(&db) {
        return Err("密码库已设置主密码（如需更换请先移除）".to_string());
    }
    // 取当前主密钥（此时尚未设主密码，走文件路径）
    let master = crate::db::passwords::master_key(&db)
        .await
        .map_err(|e| e.to_string())?;
    let wrapped = wrap_key(&master, &password);
    write_vault_file(&db.vault_path, &wrapped).map_err(|e| e.to_string())?;
    // 设置完成即视为已解锁（用户刚输入过密码）
    set_unlocked(master);
    log::info!("[vault] 主密码已设置，密码库进入保护状态");
    Ok(())
}

/// 解锁密码库（验证主密码并把主密钥载入内存）
#[tauri::command]
pub async fn unlock_vault(db: State<'_, Db>, password: String) -> Result<(), String> {
    if !is_protected(&db) {
        return Err("密码库未设主密码".to_string());
    }
    let encoded = read_vault_file(&db.vault_path).map_err(|e| e.to_string())?;
    let master = unwrap_key(&encoded, &password)?;
    set_unlocked(master);
    log::info!("[vault] 密码库已解锁");
    Ok(())
}

/// 锁定密码库（清除内存主密钥）
#[tauri::command]
pub async fn lock_vault() -> Result<(), String> {
    lock();
    log::info!("[vault] 密码库已锁定");
    Ok(())
}

/// 移除主密码（验证密码后删除 vault.key，回到"仅密钥文件"模式）
#[tauri::command]
pub async fn remove_vault_password(db: State<'_, Db>, password: String) -> Result<(), String> {
    if !is_protected(&db) {
        return Err("密码库未设主密码".to_string());
    }
    let encoded = read_vault_file(&db.vault_path).map_err(|e| e.to_string())?;
    let _ = unwrap_key(&encoded, &password)?; // 验证主密码
    std::fs::remove_file(&db.vault_path)
        .map_err(|e| format!("删除主密码文件失败: {}", e))?;
    // 移除后主密钥回到文件模式，保持内存缓存可用
    lock();
    log::info!("[vault] 主密码已移除");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_unwrap_roundtrip() {
        let master = [7u8; KEY_LEN];
        let wrapped = wrap_key(&master, "correct horse battery");
        let back = unwrap_key(&wrapped, "correct horse battery").unwrap();
        assert_eq!(back, master);
    }

    #[test]
    fn wrong_password_rejected() {
        let master = [9u8; KEY_LEN];
        let wrapped = wrap_key(&master, "right-password-1");
        let err = unwrap_key(&wrapped, "wrong-password-1").unwrap_err();
        assert!(err.contains("主密码错误"), "错误密码应被拒绝，实际: {}", err);
    }

    #[test]
    fn wrap_is_randomized() {
        let master = [1u8; KEY_LEN];
        // 同一主密码两次包裹：salt/nonce 随机 → 密文不同（防确定性泄露）
        let a = wrap_key(&master, "same-password-xx");
        let b = wrap_key(&master, "same-password-xx");
        assert_ne!(a, b);
        assert_eq!(unwrap_key(&a, "same-password-xx").unwrap(), master);
        assert_eq!(unwrap_key(&b, "same-password-xx").unwrap(), master);
    }

    #[test]
    fn garbage_rejected() {
        assert!(unwrap_key("not-base64!!!", "x").is_err());
        assert!(unwrap_key(&BASE64.encode([0u8; 4]), "x").is_err());
    }

    #[test]
    fn derive_key_deterministic() {
        let salt = [3u8; SALT_LEN];
        assert_eq!(derive_key("pw", &salt), derive_key("pw", &salt));
        assert_ne!(derive_key("pw", &salt), derive_key("pw2", &salt));
    }
}

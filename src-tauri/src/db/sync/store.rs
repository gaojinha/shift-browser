//! E2E 同步的凭据存储与加密原语（R1 拆分 · 2026-09-24）：
//! 主钥闸门、AES-256-GCM 封装、sync.key（600）读写、WebDAV 客户端与 URL 拼接。

use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};

/// 主钥包裹后的 WebDAV 配置文件（与 vault.key 同目录、同权限 600）
pub(super) const SYNC_KEY_FILE: &str = "sync.key";
/// 远端目录与仓库文件（固定单文件，避免碎片化）
const REMOTE_DIR: &str = "shift-browser";
const REMOTE_FILE: &str = "sync.v1.e2e";
const NONCE_LEN: usize = 12;

/// 同步配置（整体主钥包裹后落盘；只有内存里才是明文）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct SyncStore {
    pub(super) url: String,
    pub(super) username: String,
    pub(super) password: String,
    #[serde(default)]
    pub(super) last_push_at: Option<String>,
    #[serde(default)]
    pub(super) last_pull_at: Option<String>,
}

/// 上云闸门：必须已设主密码 + 已解锁，返回会话主密钥。
/// 未设主密码时 master.key 裸奔，上云等于把钥匙交出去 —— 一律拒绝。
pub(super) fn require_master(db: &Db) -> DbResult<[u8; 32]> {
    if !crate::db::vault::is_protected(db) {
        return Err(DbError::Other(
            "尚未设置主密码：上云同步必须先设主密码（数据主权铁律）。".into(),
        ));
    }
    if !crate::db::vault::is_unlocked() {
        return Err(DbError::Other(
            "密码库已锁定：请先解锁主密码再上云同步。".into(),
        ));
    }
    crate::db::vault::unlocked_key()
        .ok_or_else(|| DbError::Other("主密钥不在会话中：请重新解锁主密码。".into()))
}

/// AES-256-GCM 封装（随机 nonce，输出 base64(nonce || 密文)）
pub(super) fn seal(key: &[u8; 32], plaintext: &[u8]) -> DbResult<String> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| DbError::Other(format!("初始化加密器失败: {}", e)))?;
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
        .map_err(|e| DbError::Other(format!("加密失败: {}", e)))?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok(BASE64.encode(out))
}

/// 解封装（输入 base64(nonce || 密文)）
pub(super) fn open(key: &[u8; 32], encoded: &str) -> DbResult<Vec<u8>> {
    let blob = BASE64
        .decode(encoded.trim())
        .map_err(|e| DbError::Other(format!("密文解码失败: {}", e)))?;
    if blob.len() <= NONCE_LEN {
        return Err(DbError::Other("密文长度非法".into()));
    }
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| DbError::Other(format!("初始化解密器失败: {}", e)))?;
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), ciphertext)
        .map_err(|e| DbError::Other(format!("解密失败（主密码不符或密文损坏）: {}", e)))
}

/// sync.key 路径（与 vault.key 同目录）
pub(super) fn store_path(db: &Db) -> std::path::PathBuf {
    db.vault_path.with_file_name(SYNC_KEY_FILE)
}

/// 写文件并收紧权限 600（创建即收紧，无 umask 窗口）
fn write_600(path: &std::path::Path, data: &str) -> DbResult<()> {
    let write_err = |e: std::io::Error| DbError::Other(format!("写入同步配置失败: {}", e));
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
        f.write_all(data.as_bytes()).map_err(write_err)?;
        f.sync_all().map_err(write_err)?;
        // mode 仅在创建时生效；已存在文件补一次收紧兜底
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, data).map_err(write_err)?;
    }
    Ok(())
}

/// 读同步配置（未配置返回 None；需要已解锁才能解包）
pub(super) async fn load_store(db: &Db) -> DbResult<Option<SyncStore>> {
    let path = store_path(db);
    if !path.exists() {
        return Ok(None);
    }
    let key = require_master(db)?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| DbError::Other(format!("读取同步配置失败: {}", e)))?;
    let plain = open(&key, raw.trim())?;
    let store = serde_json::from_slice(&plain)
        .map_err(|e| DbError::Other(format!("同步配置解析失败: {}", e)))?;
    Ok(Some(store))
}

/// 写同步配置（主钥包裹 + 权限 600）
pub(super) fn save_store(db: &Db, store: &SyncStore, key: &[u8; 32]) -> DbResult<()> {
    let json = serde_json::to_vec(store)
        .map_err(|e| DbError::Other(format!("同步配置序列化失败: {}", e)))?;
    write_600(&store_path(db), &seal(key, &json)?)
}

/// HTTP 客户端（复用 feed.rs 同款构建）
pub(super) fn http_client() -> DbResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(crate::contracts::timeout::SYNC_REQUEST_SECS))
        .user_agent(format!(
            "ShiftBrowser/{} (E2E sync)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| DbError::Other(format!("创建 HTTP 客户端失败: {}", e)))
}

pub(super) fn repo_url(store: &SyncStore) -> String {
    format!(
        "{}/{}/{}",
        store.url.trim_end_matches('/'),
        REMOTE_DIR,
        REMOTE_FILE
    )
}

pub(super) fn dir_url(store: &SyncStore) -> String {
    format!("{}/{}", store.url.trim_end_matches('/'), REMOTE_DIR)
}

/// 当前时间（用 SQLite 统一时区，避免引入 chrono 依赖）
pub(super) async fn now_str(db: &Db) -> String {
    sqlx::query_scalar::<_, String>("SELECT datetime('now')")
        .fetch_one(&db.pool)
        .await
        .unwrap_or_default()
}

/// 脱敏主机名（只留 host:port）
pub(super) fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

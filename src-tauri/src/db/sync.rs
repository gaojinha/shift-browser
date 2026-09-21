//! 第 45 章 · 服务器零知识 E2E 加密上云同步
//!
//! 高哥拍板：上云包与 WebDAV 凭据一律复用「密码库主密码」这把钥匙。
//! 密码库必须**已设主密码且已解锁**（第 44 章 vault 闸门）才允许上云；
//! 服务器（Nextcloud）自始至终只见到 AES-256-GCM 密文，不落一字明文。
//!
//! - WebDAV 凭据（url/user/pass）→ 主钥包裹 → `sync.key`（权限 600，与 vault.key 同目录）
//! - 同步包 → 密码库全量（password 列本就是密文）→ JSON → 主钥整包封装 → base64 Blob
//! - 拉取 = 合并（按 domain+username UPSERT，绝不删除本地独有记录）

use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

/// 主钥包裹后的 WebDAV 配置文件（与 vault.key 同目录、同权限 600）
const SYNC_KEY_FILE: &str = "sync.key";
/// 远端目录与仓库文件（固定单文件，避免碎片化）
const REMOTE_DIR: &str = "shift-browser";
const REMOTE_FILE: &str = "sync.v1.e2e";
const NONCE_LEN: usize = 12;

/// 同步配置（整体主钥包裹后落盘；只有内存里才是明文）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SyncStore {
    url: String,
    username: String,
    password: String,
    #[serde(default)]
    last_push_at: Option<String>,
    #[serde(default)]
    last_pull_at: Option<String>,
}

/// 前端可见的同步状态（脱敏，绝不含凭据）
#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub vault_protected: bool,
    pub unlocked: bool,
    pub configured: bool,
    pub ready: bool,
    pub host: Option<String>,
    pub last_push_at: Option<String>,
    pub last_pull_at: Option<String>,
}

/// 上云闸门：必须已设主密码 + 已解锁，返回会话主密钥。
/// 未设主密码时 master.key 裸奔，上云等于把钥匙交出去 —— 一律拒绝。
fn require_master(db: &Db) -> DbResult<[u8; 32]> {
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
fn seal(key: &[u8; 32], plaintext: &[u8]) -> DbResult<String> {
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
fn open(key: &[u8; 32], encoded: &str) -> DbResult<Vec<u8>> {
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
fn store_path(db: &Db) -> std::path::PathBuf {
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
async fn load_store(db: &Db) -> DbResult<Option<SyncStore>> {
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
fn save_store(db: &Db, store: &SyncStore, key: &[u8; 32]) -> DbResult<()> {
    let json = serde_json::to_vec(store)
        .map_err(|e| DbError::Other(format!("同步配置序列化失败: {}", e)))?;
    write_600(&store_path(db), &seal(key, &json)?)
}

/// HTTP 客户端（复用 feed.rs 同款构建）
fn http_client() -> DbResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent(format!(
            "ShiftBrowser/{} (E2E sync)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| DbError::Other(format!("创建 HTTP 客户端失败: {}", e)))
}

fn repo_url(store: &SyncStore) -> String {
    format!(
        "{}/{}/{}",
        store.url.trim_end_matches('/'),
        REMOTE_DIR,
        REMOTE_FILE
    )
}

fn dir_url(store: &SyncStore) -> String {
    format!("{}/{}", store.url.trim_end_matches('/'), REMOTE_DIR)
}

/// 当前时间（用 SQLite 统一时区，避免引入 chrono 依赖）
async fn now_str(db: &Db) -> String {
    sqlx::query_scalar::<_, String>("SELECT datetime('now')")
        .fetch_one(&db.pool)
        .await
        .unwrap_or_default()
}

/// 脱敏主机名（只留 host:port）
fn host_of(url: &str) -> String {
    url.split("//")
        .nth(1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

/// 导出密码库：password 列**原样密文**（服务器零知识的关键：绝不解密）
async fn export_payload(db: &Db) -> DbResult<Vec<u8>> {
    let rows = sqlx::query_as::<_, (String, Option<String>, String, Option<String>, String, String)>(
        "SELECT domain, username, password, notes, created_at, updated_at FROM passwords ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await?;

    let entries: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|(domain, username, password, notes, created_at, updated_at)| {
            serde_json::json!({
                "domain": domain,
                "username": username,
                "password": password,
                "notes": notes,
                "created_at": created_at,
                "updated_at": updated_at,
            })
        })
        .collect();

    let exported_at = now_str(db).await;
    let payload = serde_json::json!({
        "version": 1,
        "exported_at": exported_at,
        "entries": entries,
    });
    serde_json::to_vec(&payload).map_err(|e| DbError::Other(format!("导出序列化失败: {}", e)))
}

/// 合并导入：按 domain+username UPSERT，仅当远端更新时覆盖；绝不删除本地独有
async fn import_payload(db: &Db, plain: &[u8]) -> DbResult<usize> {
    let v: serde_json::Value = serde_json::from_slice(plain)
        .map_err(|e| DbError::Other(format!("同步包解析失败: {}", e)))?;
    let entries = v
        .get("entries")
        .and_then(|e| e.as_array())
        .ok_or_else(|| DbError::Other("同步包格式非法：缺少 entries".into()))?;

    let mut count = 0usize;
    for e in entries {
        let domain = e.get("domain").and_then(|x| x.as_str()).unwrap_or("");
        if domain.is_empty() {
            continue;
        }
        let username = e.get("username").and_then(|x| x.as_str());
        let password = e.get("password").and_then(|x| x.as_str()).unwrap_or("");
        let notes = e.get("notes").and_then(|x| x.as_str());
        let created_at = e.get("created_at").and_then(|x| x.as_str()).unwrap_or("");
        let updated_at = e.get("updated_at").and_then(|x| x.as_str()).unwrap_or("");

        sqlx::query(
            "INSERT INTO passwords (domain, username, password, notes, created_at, updated_at)
             VALUES (?, ?, ?, ?, COALESCE(NULLIF(?, ''), datetime('now')), COALESCE(NULLIF(?, ''), datetime('now')))
             ON CONFLICT(domain, username) DO UPDATE SET
               password = excluded.password,
               notes = excluded.notes,
               updated_at = excluded.updated_at
             WHERE excluded.updated_at > passwords.updated_at",
        )
        .bind(domain)
        .bind(username)
        .bind(password)
        .bind(notes)
        .bind(created_at)
        .bind(updated_at)
        .execute(&db.pool)
        .await
        .map_err(|e| DbError::Other(format!("合并写入失败: {}", e)))?;
        count += 1;
    }
    Ok(count)
}

/// 同步状态（脱敏）
#[tauri::command]
pub async fn sync_status(db: tauri::State<'_, Db>) -> DbResult<SyncStatus> {
    let vault_protected = crate::db::vault::is_protected(&db);
    let unlocked = crate::db::vault::is_unlocked();
    let configured = store_path(&db).exists();
    let (host, last_push_at, last_pull_at) = if configured && unlocked {
        match load_store(&db).await {
            Ok(Some(s)) => (Some(host_of(&s.url)), s.last_push_at, s.last_pull_at),
            _ => (None, None, None),
        }
    } else {
        (None, None, None)
    };
    Ok(SyncStatus {
        vault_protected,
        unlocked,
        configured,
        ready: vault_protected && unlocked && configured,
        host,
        last_push_at,
        last_pull_at,
    })
}

/// 配置 WebDAV（主钥包裹落盘；需已解锁主密码）
#[tauri::command]
pub async fn sync_configure(
    db: tauri::State<'_, Db>,
    url: String,
    username: String,
    password: String,
) -> DbResult<()> {
    let key = require_master(&db)?;
    let url = url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DbError::Other(
            "WebDAV 地址必须以 http:// 或 https:// 开头".into(),
        ));
    }
    if username.trim().is_empty() {
        return Err(DbError::Other("WebDAV 用户名不能为空".into()));
    }
    let mut store = load_store(&db).await?.unwrap_or_default();
    store.url = url;
    store.username = username.trim().to_string();
    store.password = password;
    save_store(&db, &store, &key)?;
    Ok(())
}

/// 推送：全量密码库（密文原样）→ 主钥整包封装 → WebDAV PUT
#[tauri::command]
pub async fn sync_push(db: tauri::State<'_, Db>) -> DbResult<String> {
    let key = require_master(&db)?;
    let mut store = load_store(&db)
        .await?
        .ok_or_else(|| DbError::Other("尚未配置 WebDAV 同步".into()))?;

    let payload = export_payload(&db).await?;
    let count = serde_json::from_slice::<serde_json::Value>(&payload)
        .ok()
        .and_then(|v| v.get("entries").and_then(|e| e.as_array()).map(|a| a.len()))
        .unwrap_or(0);
    let sealed = seal(&key, &payload)?;

    let client = http_client()?;
    // 确保远端目录存在（已存在时 MKCOL 返回 405，忽略即可）
    let mkcol = reqwest::Method::from_bytes(b"MKCOL")
        .map_err(|e| DbError::Other(format!("构造 MKCOL 失败: {}", e)))?;
    let _ = client
        .request(mkcol, dir_url(&store))
        .basic_auth(&store.username, Some(&store.password))
        .send()
        .await;

    let resp = client
        .put(repo_url(&store))
        .basic_auth(&store.username, Some(&store.password))
        .header("Content-Type", "application/octet-stream")
        .body(sealed)
        .send()
        .await
        .map_err(|e| DbError::Other(format!("上传失败: {}", e)))?;

    let status = resp.status();
    if !status.is_success() {
        return Err(DbError::Other(format!(
            "上传失败：服务器返回 HTTP {}（请检查 WebDAV 地址与账号密码）",
            status
        )));
    }

    store.last_push_at = Some(now_str(&db).await);
    save_store(&db, &store, &key)?;
    Ok(format!("已加密推送 {} 条密码到云端（服务器仅见密文）", count))
}

/// 拉取：WebDAV GET → 主钥解包 → 合并导入（不删本地独有）
#[tauri::command]
pub async fn sync_pull(db: tauri::State<'_, Db>) -> DbResult<String> {
    let key = require_master(&db)?;
    let mut store = load_store(&db)
        .await?
        .ok_or_else(|| DbError::Other("尚未配置 WebDAV 同步".into()))?;

    let client = http_client()?;
    let resp = client
        .get(repo_url(&store))
        .basic_auth(&store.username, Some(&store.password))
        .send()
        .await
        .map_err(|e| DbError::Other(format!("下载失败: {}", e)))?;

    let status = resp.status();
    if status == StatusCode::NOT_FOUND {
        return Err(DbError::Other(
            "云端还没有同步包，请先在一台设备上推送".into(),
        ));
    }
    if !status.is_success() {
        return Err(DbError::Other(format!("下载失败：服务器返回 HTTP {}", status)));
    }

    let sealed = resp
        .text()
        .await
        .map_err(|e| DbError::Other(format!("读取响应失败: {}", e)))?;
    let plain = open(&key, &sealed)?;
    let count = import_payload(&db, &plain).await?;

    store.last_pull_at = Some(now_str(&db).await);
    save_store(&db, &store, &key)?;
    Ok(format!("已从云端合并 {} 条密码（本地独有记录未删除）", count))
}

/// 清除本地同步配置（云端同步包保留）
#[tauri::command]
pub async fn sync_clear(db: tauri::State<'_, Db>) -> DbResult<()> {
    let path = store_path(&db);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| DbError::Other(format!("删除同步配置失败: {}", e)))?;
    }
    Ok(())
}

//! 第 45 章 · 服务器零知识 E2E 加密上云同步（R1 拆分 · 2026-09-24）
//!
//! 高哥拍板：上云包与 WebDAV 凭据一律复用「密码库主密码」这把钥匙。
//! 密码库必须**已设主密码且已解锁**（第 44 章 vault 闸门）才允许上云；
//! 服务器（Nextcloud）自始至终只见到 AES-256-GCM 密文，不落一字明文。
//!
//! - WebDAV 凭据（url/user/pass）→ 主钥包裹 → `sync.key`（权限 600，与 vault.key 同目录）
//! - 同步包 → 密码库全量（password 列本就是密文）→ JSON → 主钥整包封装 → base64 Blob
//! - 拉取 = 合并（按 domain+username UPSERT，绝不删除本地独有记录）
//!
//! 本文件只放 5 个 Tauri 命令；纯逻辑拆分：
//! - [`store`]：主钥闸门 / GCM 封装 / sync.key 读写 / WebDAV 客户端
//! - [`payload`]：同步包导出与合并导入（含单测）

mod payload;
mod store;

use payload::{export_payload, import_payload};
use store::{
    dir_url, host_of, http_client, load_store, now_str, open, repo_url, require_master, save_store,
    seal, store_path,
};

use crate::db::{Db, DbError, DbResult};
use reqwest::StatusCode;
use serde::Serialize;

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
        .map(|v| {
            let n = |k: &str| {
                v.get(k)
                    .and_then(|e| e.as_array())
                    .map(|a| a.len())
                    .unwrap_or(0)
            };
            n("entries") + n("bookmarks") + n("sessions") + n("history")
        })
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
    Ok(format!(
        "已加密推送 {} 项（密码库 + 书签 + 会话 + 历史）到云端（服务器仅见密文）",
        count
    ))
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
        return Err(DbError::Other(format!(
            "下载失败：服务器返回 HTTP {}",
            status
        )));
    }

    let sealed = resp
        .text()
        .await
        .map_err(|e| DbError::Other(format!("读取响应失败: {}", e)))?;
    let plain = open(&key, &sealed)?;
    let count = import_payload(&db, &plain).await?;

    store.last_pull_at = Some(now_str(&db).await);
    save_store(&db, &store, &key)?;
    Ok(format!(
        "已从云端合并 {} 项（密码库 + 书签 + 会话 + 历史，本地独有记录未删除）",
        count
    ))
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

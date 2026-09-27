//! 密码管理命令（R1 拆分 · 2026-09-24）
//!
//! 本文件只放 6 个 Tauri 命令与出入参结构；密钥文件/主钥获取/AES-GCM 加解密
//! 等纯逻辑在 [`crate::db::password_crypto`]（含全部单测）。
use ts_rs::TS;

use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

// vault.rs 经 crate::db::passwords::master_key 调用——重导出保持原路径
pub(crate) use super::password_crypto::master_key;
use super::password_crypto::{decrypt, encrypt, get_master_key};

/// 保存密码（加密后存储）
#[tauri::command]
pub async fn save_password(
    db: tauri::State<'_, Db>,
    domain: String,
    username: Option<String>,
    password: String,
    notes: Option<String>,
) -> DbResult<i64> {
    // 脏数据防护：空域名/空密码拒绝入库（空 domain 会污染唯一索引与查询语义）
    if domain.trim().is_empty() {
        return Err(crate::db::DbError::Other("域名不能为空".to_string()));
    }
    if password.is_empty() {
        return Err(crate::db::DbError::Other("密码不能为空".to_string()));
    }
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
    tabs: tauri::State<'_, crate::tabs::TabManager>,
    domain: String,
) -> DbResult<Option<PasswordEntry>> {
    // 安全（#7 · 2026-09-22）：domain 必须匹配"当前活动标签"的 host，
    // 否则拒绝——防止被诱导取到其他站点的密码（不信任前端入参）。
    let host = tabs
        .active_url()
        .and_then(|u| url::Url::parse(&u).ok())
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_default();
    if host.is_empty() || !domain.eq_ignore_ascii_case(&host) {
        log::warn!(
            "[passwords] get_password 拒绝：domain={} 与活动标签 host={} 不符",
            domain,
            host
        );
        return Ok(None);
    }
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

/// 从活动标签取 host（小写）。空 = 无活动页面/内部页，调用方应拒绝明文出口
fn active_host(tabs: &crate::tabs::TabManager) -> String {
    tabs.active_url()
        .and_then(|u| url::Url::parse(&u).ok())
        .and_then(|u| u.host_str().map(|s| s.to_ascii_lowercase()))
        .unwrap_or_default()
}

/// 按 id 获取单条密码（解密返回）。同域名多账号时前端用 id 精确定位
/// 安全：与 get_password 同口径——条目 domain 必须匹配当前活动标签 host，
/// 否则拒绝。防止页面脚本枚举 id（1..N）捞走全表明文。
/// 密码管理器跨站查看会受限（需先到该站再查看），安全优先。
#[tauri::command]
pub async fn get_password_by_id(
    db: tauri::State<'_, Db>,
    tabs: tauri::State<'_, crate::tabs::TabManager>,
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
    let host = active_host(&tabs);
    if host.is_empty() || !domain.eq_ignore_ascii_case(&host) {
        log::warn!(
            "[passwords] get_password_by_id 拒绝：id={} domain={} 与活动标签 host={} 不符",
            id,
            domain,
            host
        );
        return Ok(None);
    }
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
/// 安全：domain 必须非空且匹配当前活动标签 host；LIKE 通配符转义，空串直接拒
/// （旧实现空串变 LIKE '%%' 全表捞明文）。
#[tauri::command]
pub async fn get_passwords_for_domain(
    db: tauri::State<'_, Db>,
    tabs: tauri::State<'_, crate::tabs::TabManager>,
    domain: String,
) -> DbResult<Vec<PasswordEntry>> {
    let domain = domain.trim().to_string();
    if domain.is_empty() {
        log::warn!("[passwords] get_passwords_for_domain 拒绝：空 domain");
        return Ok(vec![]);
    }
    let host = active_host(&tabs);
    if host.is_empty() || !domain.eq_ignore_ascii_case(&host) {
        log::warn!(
            "[passwords] get_passwords_for_domain 拒绝：domain={} 与活动标签 host={} 不符",
            domain,
            host
        );
        return Ok(vec![]);
    }
    // LIKE 转义：\、%、_ 按字面量匹配，加 ESCAPE 子句
    let escaped = domain.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, String, String)>(
        "SELECT id, domain, username, password, notes, created_at, updated_at FROM passwords WHERE domain LIKE ? ESCAPE '\\' ORDER BY id",
    )
    .bind(format!("%{}%", escaped))
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
#[derive(Debug, Serialize, Deserialize, Clone, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct PasswordMeta {
    #[ts(type = "number")]
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

// LINES-EXEMPT: 仅保留 6 个 Tauri 命令与出入参结构；AES-GCM 加解密与密钥派生已全部外移到 password_crypto
//! 密码管理命令（R1 拆分 · 2026-09-24）
//!
//! 本文件只放 6 个 Tauri 命令与出入参结构；密钥文件/主钥获取/AES-GCM 加解密
//! 等纯逻辑在 [`crate::db::password_crypto`]（含全部单测）。
//!
//! 约定：命令层只把 tauri::State 拆成 `&Db` / 活动标签 host 后转调 `*_inner`；
//! 所有 SQL 与安全规则（空域名拒绝、host 匹配闸门、UPSERT 去重）只在 `*_inner`
//! 里写一次，集成测试直接调它们（不再重抄 SQL）。
use ts_rs::TS;

use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

// vault.rs 经 crate::db::passwords::master_key 调用——重导出保持原路径
pub(crate) use super::password_crypto::master_key;
use super::password_crypto::{decrypt, encrypt, get_master_key};

// ===== 核心逻辑（与 Tauri 解耦，便于集成测试）=====

/// 保存密码（加密后存储）。空域名/空密码拒绝入库。
pub(crate) async fn save_password_inner(
    db: &Db,
    domain: &str,
    username: Option<String>,
    password: &str,
    notes: Option<String>,
) -> DbResult<i64> {
    // 脏数据防护：空域名/空密码拒绝入库（空 domain 会污染唯一索引与查询语义）
    if domain.trim().is_empty() {
        return Err(crate::db::DbError::Other("域名不能为空".to_string()));
    }
    if password.is_empty() {
        return Err(crate::db::DbError::Other("密码不能为空".to_string()));
    }
    let key = get_master_key(db).await?;
    let encrypted = encrypt(&key, password)?;

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
    .bind(domain)
    .bind(&username)
    .bind(&encrypted)
    .bind(&notes)
    .execute(&db.pool)
    .await?;

    Ok(0) // UPSERT 不返回可靠 rowid，调用方（前端）以 domain+username 查询为准
}

/// 保存密码（**insert-only**：同 (domain, username) 已存在则原样保留，返回 false）。
/// F-98：浏览器导入专用——不从浏览器静默覆盖用户已有密码（覆盖会毁掉用户在
/// 应用内改过的值）。与 `save_password_inner` 同做空域名/空密码守卫；
/// username=NULL 沿用其语义（SQLite 不触发唯一冲突，照插）。
pub(crate) async fn save_password_new_only_inner(
    db: &Db,
    domain: &str,
    username: Option<String>,
    password: &str,
    notes: Option<String>,
) -> DbResult<bool> {
    if domain.trim().is_empty() {
        return Err(crate::db::DbError::Other("域名不能为空".to_string()));
    }
    if password.is_empty() {
        return Err(crate::db::DbError::Other("密码不能为空".to_string()));
    }
    let key = get_master_key(db).await?;
    let encrypted = encrypt(&key, password)?;

    // insert-only：冲突时 DO NOTHING。rows_affected()==1 → 新插入；
    // ==0 → 已存在被跳过（靠迁移 7 的 (domain, username) 唯一索引原子判定）。
    let res = sqlx::query(
        "INSERT INTO passwords (domain, username, password, notes) VALUES (?, ?, ?, ?)
         ON CONFLICT(domain, username) DO NOTHING",
    )
    .bind(domain)
    .bind(&username)
    .bind(&encrypted)
    .bind(&notes)
    .execute(&db.pool)
    .await?;

    Ok(res.rows_affected() > 0)
}

/// 列出所有密码记录（不含明文，前端展示列表用）
pub(crate) async fn list_passwords_inner(db: &Db) -> DbResult<Vec<PasswordMeta>> {
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
pub(crate) async fn delete_password_inner(db: &Db, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM passwords WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 按 domain 取单条并解密。
/// 安全（#7 · 2026-09-22）：domain 必须匹配当前活动标签 host，否则返回 None——
/// 防止被诱导取到其他站点的密码（不信任前端入参）。
pub(crate) async fn get_password_for_host_inner(
    db: &Db,
    host: &str,
    domain: &str,
) -> DbResult<Option<PasswordEntry>> {
    if host.is_empty() || !domain.eq_ignore_ascii_case(host) {
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
    .bind(domain)
    .fetch_optional(&db.pool)
    .await?;

    let Some((id, domain, username, encrypted, notes, created_at, updated_at)) = row else {
        return Ok(None);
    };
    let key = get_master_key(db).await?;
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

/// 按 id 取单条并解密。同域名多账号时前端用 id 精确定位。
/// 安全：与 get_password 同口径——条目 domain 必须匹配当前活动标签 host，否则拒绝。
/// 防止页面脚本枚举 id（1..N）捞走全表明文。
pub(crate) async fn get_password_by_id_for_host_inner(
    db: &Db,
    host: &str,
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
    if host.is_empty() || !domain.eq_ignore_ascii_case(host) {
        log::warn!(
            "[passwords] get_password_by_id 拒绝：id={} domain={} 与活动标签 host={} 不符",
            id,
            domain,
            host
        );
        return Ok(None);
    }
    let key = get_master_key(db).await?;
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

/// 取某域名下所有密码记录（解密）。
/// 安全：domain 必须非空且匹配当前活动标签 host；LIKE 通配符转义，空串直接拒
/// （旧实现空串变 LIKE '%%' 全表捞明文）。
pub(crate) async fn get_passwords_for_domain_inner(
    db: &Db,
    host: &str,
    domain: &str,
) -> DbResult<Vec<PasswordEntry>> {
    let domain = domain.trim().to_string();
    if domain.is_empty() {
        log::warn!("[passwords] get_passwords_for_domain 拒绝：空 domain");
        return Ok(vec![]);
    }
    if host.is_empty() || !domain.eq_ignore_ascii_case(host) {
        log::warn!(
            "[passwords] get_passwords_for_domain 拒绝：domain={} 与活动标签 host={} 不符",
            domain,
            host
        );
        return Ok(vec![]);
    }
    // LIKE 转义：\、%、_ 按字面量匹配，加 ESCAPE 子句
    let escaped = domain
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, String, String)>(
        "SELECT id, domain, username, password, notes, created_at, updated_at FROM passwords WHERE domain LIKE ? ESCAPE '\\' ORDER BY id",
    )
    .bind(format!("%{}%", escaped))
    .fetch_all(&db.pool)
    .await?;

    let key = get_master_key(db).await?;
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

// ===== Tauri 命令层（薄壳：拆 State / 取活动标签 host → 转调 *_inner）=====

/// 从活动标签取 host（小写）。空 = 无活动页面/内部页，调用方应拒绝明文出口
fn active_host(tabs: &crate::tabs::TabManager) -> String {
    tabs.active_url()
        .and_then(|u| url::Url::parse(&u).ok())
        .and_then(|u| u.host_str().map(|s| s.to_ascii_lowercase()))
        .unwrap_or_default()
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
    save_password_inner(&db, &domain, username, &password, notes).await
}

/// 获取密码（解密返回）
#[tauri::command]
pub async fn get_password(
    db: tauri::State<'_, Db>,
    tabs: tauri::State<'_, crate::tabs::TabManager>,
    domain: String,
) -> DbResult<Option<PasswordEntry>> {
    let host = active_host(&tabs);
    get_password_for_host_inner(&db, &host, &domain).await
}

/// 按 id 获取单条密码（解密返回）。同域名多账号时前端用 id 精确定位
#[tauri::command]
pub async fn get_password_by_id(
    db: tauri::State<'_, Db>,
    tabs: tauri::State<'_, crate::tabs::TabManager>,
    id: i64,
) -> DbResult<Option<PasswordEntry>> {
    let host = active_host(&tabs);
    get_password_by_id_for_host_inner(&db, &host, id).await
}

/// 获取某域名下所有密码记录（解密）
#[tauri::command]
pub async fn get_passwords_for_domain(
    db: tauri::State<'_, Db>,
    tabs: tauri::State<'_, crate::tabs::TabManager>,
    domain: String,
) -> DbResult<Vec<PasswordEntry>> {
    let host = active_host(&tabs);
    get_passwords_for_domain_inner(&db, &host, &domain).await
}

/// 列出所有密码记录（不含明文，前端展示列表用）
#[tauri::command]
pub async fn list_passwords(db: tauri::State<'_, Db>) -> DbResult<Vec<PasswordMeta>> {
    list_passwords_inner(&db).await
}

/// 删除密码记录
#[tauri::command]
pub async fn delete_password(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    delete_password_inner(&db, id).await
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（含 passwords 表与 (domain, username) 唯一索引）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    #[tokio::test]
    async fn save_rejects_empty_domain_or_password() {
        let (_dir, db) = temp_db().await;
        assert!(save_password_inner(&db, "   ", None, "pw", None)
            .await
            .is_err());
        assert!(save_password_inner(&db, "example.com", None, "", None)
            .await
            .is_err());
        // 都被拦下 → 表里一条没有
        assert!(list_passwords_inner(&db).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn save_then_list_never_exposes_plaintext() {
        let (_dir, db) = temp_db().await;
        save_password_inner(
            &db,
            "example.com",
            Some("alice".into()),
            "s3cret",
            Some("备注".into()),
        )
        .await
        .unwrap();

        let metas = list_passwords_inner(&db).await.unwrap();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].domain, "example.com");
        assert_eq!(metas[0].username.as_deref(), Some("alice"));
        assert_eq!(metas[0].notes.as_deref(), Some("备注"));

        // 落库的 password 列必须是密文，不含明文
        let stored: String = sqlx::query_scalar("SELECT password FROM passwords LIMIT 1")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_ne!(stored, "s3cret");
        assert!(!stored.contains("s3cret"));
    }

    #[tokio::test]
    async fn get_password_decrypts_and_enforces_host_gate() {
        let (_dir, db) = temp_db().await;
        save_password_inner(&db, "example.com", Some("alice".into()), "s3cret", None)
            .await
            .unwrap();

        // host 匹配 → 解出明文
        let got = get_password_for_host_inner(&db, "example.com", "example.com")
            .await
            .unwrap()
            .expect("host 匹配应返回条目");
        assert_eq!(got.password, "s3cret");
        assert_eq!(got.username.as_deref(), Some("alice"));

        // host 大小写不敏感
        assert!(
            get_password_for_host_inner(&db, "EXAMPLE.COM", "example.com")
                .await
                .unwrap()
                .is_some()
        );

        // host 不符 → 拒绝
        assert!(get_password_for_host_inner(&db, "evil.com", "example.com")
            .await
            .unwrap()
            .is_none());
        // 无活动标签（空 host）→ 拒绝
        assert!(get_password_for_host_inner(&db, "", "example.com")
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn save_upserts_on_domain_username() {
        let (_dir, db) = temp_db().await;
        save_password_inner(&db, "example.com", Some("alice".into()), "old", None)
            .await
            .unwrap();
        save_password_inner(
            &db,
            "example.com",
            Some("alice".into()),
            "new",
            Some("改过".into()),
        )
        .await
        .unwrap();

        // 同 domain+username 只有一行，且密码被覆盖
        let metas = list_passwords_inner(&db).await.unwrap();
        assert_eq!(metas.len(), 1);
        let got = get_password_for_host_inner(&db, "example.com", "example.com")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.password, "new");
        assert_eq!(got.notes.as_deref(), Some("改过"));

        // 同域名不同用户名 → 两行（唯一索引只约束 (domain, username)）
        save_password_inner(&db, "example.com", Some("bob".into()), "pw2", None)
            .await
            .unwrap();
        assert_eq!(list_passwords_inner(&db).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn save_new_only_skips_existing_without_overwrite() {
        let (_dir, db) = temp_db().await;

        // 首发：新插入返回 true
        assert!(save_password_new_only_inner(
            &db,
            "example.com",
            Some("alice".into()),
            "first",
            None
        )
        .await
        .unwrap());

        // 同 (domain, username) 再来：跳过、返回 false、且**不覆盖**原值
        assert!(!save_password_new_only_inner(
            &db,
            "example.com",
            Some("alice".into()),
            "second",
            None
        )
        .await
        .unwrap());
        let got = get_password_for_host_inner(&db, "example.com", "example.com")
            .await
            .unwrap()
            .expect("应仍存在");
        assert_eq!(got.password, "first", "insert-only 不得覆盖已存在密码");
        assert_eq!(
            list_passwords_inner(&db).await.unwrap().len(),
            1,
            "不得产生重复行"
        );

        // 不同 username：各自独立插入
        assert!(save_password_new_only_inner(
            &db,
            "example.com",
            Some("bob".into()),
            "bobpw",
            None
        )
        .await
        .unwrap());
        assert_eq!(list_passwords_inner(&db).await.unwrap().len(), 2);

        // username=NULL 不触发唯一冲突（与 save_password_inner 同语义，照插）
        assert!(
            save_password_new_only_inner(&db, "null.com", None, "p1", None)
                .await
                .unwrap()
        );
        assert!(
            save_password_new_only_inner(&db, "null.com", None, "p2", None)
                .await
                .unwrap()
        );

        // 空域名/空密码仍拒收（与 save_password_inner 同守卫）
        assert!(save_password_new_only_inner(&db, "  ", None, "pw", None)
            .await
            .is_err());
        assert!(save_password_new_only_inner(&db, "x.com", None, "", None)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn get_by_id_enforces_host_gate() {
        let (_dir, db) = temp_db().await;
        save_password_inner(&db, "example.com", Some("alice".into()), "s3cret", None)
            .await
            .unwrap();
        let id = list_passwords_inner(&db).await.unwrap()[0].id;

        // host 匹配 → 解出
        let got = get_password_by_id_for_host_inner(&db, "example.com", id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(got.password, "s3cret");

        // host 不符 → 拒绝（防页面枚举 id 捞全表）
        assert!(get_password_by_id_for_host_inner(&db, "evil.com", id)
            .await
            .unwrap()
            .is_none());
        // id 不存在 → None
        assert!(get_password_by_id_for_host_inner(&db, "example.com", 99999)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn get_for_domain_rejects_empty_or_mismatched_host() {
        let (_dir, db) = temp_db().await;
        save_password_inner(&db, "example.com", Some("alice".into()), "pw1", None)
            .await
            .unwrap();
        save_password_inner(&db, "example.com", Some("bob".into()), "pw2", None)
            .await
            .unwrap();

        // 空 domain → 空结果（旧实现空串会变 LIKE '%%' 全表捞明文）
        assert!(get_passwords_for_domain_inner(&db, "example.com", "  ")
            .await
            .unwrap()
            .is_empty());
        // host 不符 → 空结果
        assert!(
            get_passwords_for_domain_inner(&db, "evil.com", "example.com")
                .await
                .unwrap()
                .is_empty()
        );

        // 正常：两条都解出（按 id 排序）
        let list = get_passwords_for_domain_inner(&db, "example.com", "example.com")
            .await
            .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].username.as_deref(), Some("alice"));
        assert_eq!(list[1].username.as_deref(), Some("bob"));
    }

    #[tokio::test]
    async fn delete_password_removes_row() {
        let (_dir, db) = temp_db().await;
        save_password_inner(&db, "example.com", Some("alice".into()), "pw", None)
            .await
            .unwrap();
        let id = list_passwords_inner(&db).await.unwrap()[0].id;
        delete_password_inner(&db, id).await.unwrap();
        assert!(list_passwords_inner(&db).await.unwrap().is_empty());
    }
}

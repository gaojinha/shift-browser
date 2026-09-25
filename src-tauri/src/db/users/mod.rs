//! 用户账号系统（个人中心）：注册 / 登录 / 资料 / 偏好 / 备份（R2-1 拆分 · 2026-09-24）
//!
//! 原 db/users.rs 按主题拆为子模块：security（密码/限流/校验）、session（会话与认证）、
//! profile（资料与偏好）、import_export（备份导入导出）。本文件保留数据结构与重导出。

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPrefs {
    pub show_lunar: bool,
    pub show_quote: bool,
    pub show_weather: bool,
    pub font_size: i64,
    pub weather_city: String,
    pub theme_color: String,
    pub wallpaper: String,
}


impl Default for UserPrefs {
    fn default() -> Self {
        Self {
            show_lunar: true,
            show_quote: true,
            show_weather: true,
            font_size: 14,
            weather_city: "运城".into(),
            theme_color: "#4a9eff".into(),
            wallpaper: "".into(),
        }
    }
}


#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub nickname: String,
    pub avatar: String,
    pub signature: String,
    pub created_at: String,
    pub prefs: UserPrefs,
}


/// 统一返回：{ ok, msg, user?, path? }
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserResult {
    pub ok: bool,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}


impl UserResult {
    pub(crate) fn ok(msg: impl Into<String>) -> Self {
        Self {
            ok: true,
            msg: msg.into(),
            user: None,
            path: None,
        }
    }
    pub(crate) fn ok_user(msg: impl Into<String>, user: User) -> Self {
        Self {
            ok: true,
            msg: msg.into(),
            user: Some(user),
            path: None,
        }
    }
    pub(crate) fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            msg: msg.into(),
            user: None,
            path: None,
        }
    }
}



/// sqlx 错误统一转 UserResult::err（UserResult 不是 Result，不能用 ?）
macro_rules! db_try {
    ($e:expr) => {
        match $e.await {
            Ok(v) => v,
            Err(e) => return Ok(crate::db::users::UserResult::err(format!("数据库错误: {}", e))),
        }
    };
}

// 宏经 glob 导入到子模块，clippy 的 unused_imports 检测追踪不到，显式放行
#[allow(unused_imports)]
pub(crate) use db_try;

mod import_export;
mod profile;
mod security;
mod session;

pub use import_export::*;
pub use profile::*;
pub use session::*;


#[cfg(test)]
mod tests {
    use super::*;
    use super::security::*;
    use crate::db::Db;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// 建一个带 users 表的临时库（用生产迁移建表，保证字段一致）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("test.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../../migrations/20260914000000_users.sql"))
            .execute(&pool)
            .await
            .unwrap();
        let key_path = dir.path().join("master.key");
        let vault_path = dir.path().join("vault.key");
        (
            dir,
            Db {
                pool: pool.clone(),
                feed_pool: pool,
                key_path,
                vault_path,
                master_key: tokio::sync::Mutex::new(None),
            },
        )
    }

    /// 按指定算法标记插入一个用户（模拟历史数据）
    async fn insert_user(db: &Db, name: &str, pwd: &str, algo: &str) {
        let salt = generate_salt();
        let iter = iterations_for_algo(algo);
        let hash = hash_password_with_iter(pwd, &salt, iter);
        sqlx::query(
      "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
       VALUES (?, ?, ?, ?, ?, (char(129418)), (char(36825)))",
    )
    .bind(name)
    .bind(&hash)
    .bind(&salt)
    .bind(algo)
    .bind(name)
    .execute(&db.pool)
    .await
    .unwrap();
    }

    async fn algo_of(db: &Db, name: &str) -> String {
        sqlx::query_scalar::<_, String>("SELECT hash_algo FROM users WHERE username = ?")
            .bind(name)
            .fetch_one(&db.pool)
            .await
            .unwrap()
    }

    #[test]
    fn test_iterations_for_algo_parsing() {
        // 当前格式
        assert_eq!(iterations_for_algo(HASH_ALGO_CURRENT), PBKDF2_ITER);
        assert_eq!(iterations_for_algo("pbkdf2-sha512-210000"), 210_000);
        // 历史标记 / 未知值都按旧参数兜底（老用户仍能登录）
        assert_eq!(iterations_for_algo("pbkdf2"), LEGACY_PBKDF2_ITER);
        assert_eq!(iterations_for_algo(""), LEGACY_PBKDF2_ITER);
        assert_eq!(iterations_for_algo("bcrypt"), LEGACY_PBKDF2_ITER);
        assert_eq!(iterations_for_algo("pbkdf2-sha512-abc"), LEGACY_PBKDF2_ITER);
    }

    #[tokio::test]
    async fn test_verify_upgrades_legacy_hash() {
        let (_dir, db) = temp_db().await;
        // 旧版用户：hash_algo = pbkdf2（10 万次）
        insert_user(&db, "testuser", "test-pass-42", "pbkdf2").await;
        assert_eq!(algo_of(&db, "testuser").await, "pbkdf2");

        // 正确密码通过，且哈希被透明升级到当前算法
        assert!(verify_password(&db, "testuser", "test-pass-42")
            .await
            .unwrap());
        assert_eq!(algo_of(&db, "testuser").await, HASH_ALGO_CURRENT);

        // 升级后仍能用同一密码登录（参数一致）
        assert!(verify_password(&db, "testuser", "test-pass-42")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_verify_rejects_wrong_password_without_upgrade() {
        let (_dir, db) = temp_db().await;
        // 用小迭代标记（1000 次）只是为了跑得快：失败路径与迭代次数无关
        insert_user(&db, "testuser", "test-pass-42", "pbkdf2-sha512-1000").await;

        assert!(!verify_password(&db, "testuser", "wrong-pass")
            .await
            .unwrap());
        // 失败不改哈希（否则等于把账号锁死）
        assert_eq!(algo_of(&db, "testuser").await, "pbkdf2-sha512-1000");
        // 空密码/不存在用户也不通过
        assert!(!verify_password(&db, "testuser", "").await.unwrap());
        assert!(!verify_password(&db, "nobody", "test-pass-42")
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_new_password_uses_current_algo() {
        let (_dir, db) = temp_db().await;
        let salt = generate_salt();
        let hash = hash_password("abc12345", &salt);
        sqlx::query(
      "INSERT INTO users (username, password_hash, salt, hash_algo, nickname) VALUES (?, ?, ?, ?, ?)",
    )
    .bind("newbie")
    .bind(&hash)
    .bind(&salt)
    .bind(HASH_ALGO_CURRENT)
    .bind("newbie")
    .execute(&db.pool)
    .await
    .unwrap();
        assert_eq!(algo_of(&db, "newbie").await, HASH_ALGO_CURRENT);
        assert!(verify_password(&db, "newbie", "abc12345").await.unwrap());
    }

    /// P1-4：导入备份字段校验——合法通过、非法拒绝
    #[test]
    fn test_validate_import_account() {
        let ok_hash = "a".repeat(128);
        let ok_salt = "b".repeat(32);
        // 合法：历史标记 / 当前标记 / 自定义合理迭代
        assert!(validate_import_account("张三", &ok_hash, &ok_salt, "pbkdf2").is_ok());
        assert!(
            validate_import_account("zhang3", &ok_hash, &ok_salt, "pbkdf2-sha512-210000").is_ok()
        );
        assert!(
            validate_import_account("zhang3", &ok_hash, &ok_salt, "pbkdf2-sha512-1000").is_ok()
        );
        // 用户名非法
        assert!(validate_import_account("a", &ok_hash, &ok_salt, "pbkdf2").is_err());
        assert!(validate_import_account("a/b", &ok_hash, &ok_salt, "pbkdf2").is_err());
        assert!(validate_import_account("", &ok_hash, &ok_salt, "pbkdf2").is_err());
        // 哈希/盐格式非法（非 hex、长度不对）
        assert!(validate_import_account("zhang3", "zz", &ok_salt, "pbkdf2").is_err());
        assert!(validate_import_account("zhang3", &ok_hash[..64], &ok_salt, "pbkdf2").is_err());
        assert!(validate_import_account("zhang3", &ok_hash, "zz", "pbkdf2").is_err());
        // 算法标记非法：未知算法、迭代超上限（防登录时 DoS）、非数字
        assert!(validate_import_account("zhang3", &ok_hash, &ok_salt, "bcrypt").is_err());
        assert!(
            validate_import_account("zhang3", &ok_hash, &ok_salt, "pbkdf2-sha512-999999999")
                .is_err()
        );
        assert!(validate_import_account("zhang3", &ok_hash, &ok_salt, "pbkdf2-sha512-0").is_err());
        assert!(
            validate_import_account("zhang3", &ok_hash, &ok_salt, "pbkdf2-sha512-abc").is_err()
        );
    }

    /// R1 错误类型化防漂移：会话写入/清除往返 + 未登录态读取
    #[tokio::test]
    async fn test_session_roundtrip() {
        let (_dir, db) = temp_db().await;
        assert_eq!(current_user_id(&db).await, None);
        insert_user(&db, "alice", "test-pass-42", HASH_ALGO_CURRENT).await;
        let uid: i64 = sqlx::query_scalar("SELECT id FROM users WHERE username = 'alice'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        set_session(&db, Some(uid)).await.unwrap();
        assert_eq!(current_user_id(&db).await, Some(uid));
        clear_session(&db).await.unwrap();
        assert_eq!(current_user_id(&db).await, None);
    }

    /// R1 错误类型化防漂移：持久层故障必须归类 E_DB（前端据此提示"稍后重试"，
    /// 而不是误报账号密码错误）
    #[tokio::test]
    async fn test_db_failure_is_e_db() {
        let (_dir, db) = temp_db().await;
        sqlx::query("DROP TABLE users")
            .execute(&db.pool)
            .await
            .unwrap();
        let err = verify_password(&db, "someone", "whatever").await.unwrap_err();
        assert_eq!(err.code, crate::error::E_DB);
    }
}

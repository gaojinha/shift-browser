// 数据库连接与迁移管理
pub mod ai_audit;
pub mod bookmarks;
pub mod certificates;
pub mod downloads;
pub mod extensions;
pub mod history;
// 密码库密钥/加解密原语（passwords 命令的内部实现，不对外）
mod password_crypto;
pub mod passwords;
pub mod sessions;
pub mod settings;
pub mod settings_cache;
pub mod sync;
pub mod tab_groups;
pub mod translate_memory;
pub mod users;
pub mod userscripts;
pub mod vault;
// vault.key 加解密与 v1/v2 文件格式（vault 命令的内部实现，不对外）
mod vault_crypto;
pub mod workspaces;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;

/// 进程级主连接池句柄：Tauri 命令走 `State<Db>`；CEF 外壳的 console 回调拿不到
/// AppHandle，只能经此取池（F-CEF-13 书签管理动作）。`Db::init` 时注册一次。
pub static GLOBAL_POOL: std::sync::OnceLock<SqlitePool> = std::sync::OnceLock::new();

/// 全局数据库连接池（Tauri State 管理）
pub struct Db {
    pub pool: SqlitePool,
    /// 资讯抓取专用连接池（性能 P1 · 2026-09-22）：与 UI 命令的 pool 隔离，
    /// 避免 62 源并发写与 UI 抢连接/写锁（原共用一个 5 连接池 → 启动后卡顿）
    pub feed_pool: SqlitePool,
    /// 密码主密钥文件路径（独立于数据库，权限 600）
    pub key_path: PathBuf,
    /// 密码库主密码包裹文件路径（vault.key，权限 600；文件存在 = 已设主密码）
    pub vault_path: PathBuf,
    /// 翻译记忆 HMAC 密钥文件路径（translate.key，权限 600；C-11 原文不落明文）
    pub translate_key_path: PathBuf,
    /// 密码主密钥内存缓存：进程内串行化首次生成（并发首用不会双写覆盖），
    /// 命中后不再重复读文件。tokio Mutex 保证跨 await 持有。
    pub master_key: tokio::sync::Mutex<Option<[u8; 32]>>,
    /// 翻译记忆 HMAC 密钥内存缓存（同 master_key 的串行化口径）
    pub translate_key: tokio::sync::Mutex<Option<[u8; 32]>>,
}

impl Db {
    /// 初始化数据库：创建目录、连接池、执行迁移
    pub async fn init(app_data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        std::fs::create_dir_all(&app_data_dir)?;
        let db_path = app_data_dir.join("shift-browser.db");

        // 安全：应用数据目录权限收紧到 700（含数据库/密钥/cookies 等敏感文件，
        // 目录不可进则同机其他用户无法读取内部任何文件）
        #[cfg(unix)]
        {
            crate::harden::restrict_mode(&app_data_dir, 0o700);
        }

        // 安全：预创建数据库文件并直接以 600 落盘。sqlx 的 create_if_missing 会按 umask
        // 建文件（常见 644），到下面 chmod 之间存在窗口期；这里先占位即可消除。
        // 失败不致命（后续 connect_with 仍会尝试创建），故忽略错误。
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let _ = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .mode(0o600)
                .open(&db_path);
        }

        // 注意：sqlx 的 create_if_missing 默认是 false，
        // 不加 create_if_missing(true) 时，文件不存在会报 SQLITE_CANTOPEN (code 14)
        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            // 写锁等待 10 秒：下载进度频繁更新 + 多连接并发写时避免 database is locked 立即失败
            .busy_timeout(std::time::Duration::from_secs(
                crate::contracts::timeout::DB_BUSY_SECS,
            ));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        let _ = GLOBAL_POOL.set(pool.clone());

        // 安全：数据库含加密密码/设置，权限收紧到 600（仅当前用户可读写）。
        // 必须在 connect_with（此时才 create_if_missing 建文件）之后执行，
        // 否则首次运行时文件尚未创建，收紧不生效、以 umask 默认权限落盘
        #[cfg(unix)]
        {
            crate::harden::restrict_mode(&db_path, 0o600);
        }

        // 开启 WAL 与外键约束
        sqlx::query("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .execute(&pool)
            .await?;

        // 启动时收缩 WAL：上次运行大量写入（如 feed 刷新）后 -wal 文件可能膨胀，
        // 这里把已提交内容合并回主库并截断 WAL（TRUNCATE），保持磁盘整洁。
        // 运行中 WAL 超 4MB 会由 SQLite 自动 checkpoint，不会无限增长。
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE);")
            .execute(&pool)
            .await?;

        // 执行迁移
        Self::run_migrations(&pool).await?;

        // 性能 P1：资讯抓取独立连接池（2 连接，够 16 路信号量排队用）
        let feed_options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .busy_timeout(std::time::Duration::from_secs(
                crate::contracts::timeout::DB_BUSY_SECS,
            ));
        let feed_pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(feed_options)
            .await?;

        log::info!("[ShiftBrowser] 数据库已初始化: {}", db_path.display());
        Ok(Self {
            pool,
            feed_pool,
            key_path: app_data_dir.join("master.key"),
            vault_path: app_data_dir.join("vault.key"),
            translate_key_path: app_data_dir.join("translate.key"),
            master_key: tokio::sync::Mutex::new(None),
            translate_key: tokio::sync::Mutex::new(None),
        })
    }

    /// 执行 SQL 迁移文件
    async fn run_migrations(pool: &SqlitePool) -> Result<(), Box<dyn std::error::Error>> {
        // 创建迁移记录表
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS _migrations (
        version INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        applied_at TEXT DEFAULT (datetime('now'))
      )",
        )
        .execute(pool)
        .await?;

        // 迁移列表：版本号 -> (名称, SQL)
        let migrations: Vec<(i64, &str, &str)> = vec![
            (
                1,
                "init_core_tables",
                include_str!("../../migrations/20260905000000_init.sql"),
            ),
            (
                2,
                "feed_items",
                include_str!("../../migrations/20260913000000_feeds.sql"),
            ),
            (
                3,
                "history_url_uniq",
                include_str!("../../migrations/20260913000001_history_uniq.sql"),
            ),
            (
                4,
                "drop_dead_feed_index",
                include_str!("../../migrations/20260913000002_drop_dead_index.sql"),
            ),
            (
                5,
                "users_account_system",
                include_str!("../../migrations/20260914000000_users.sql"),
            ),
            (
                6,
                "history_last_visit_index",
                include_str!("../../migrations/20260918000000_history_last_visit_index.sql"),
            ),
            (
                7,
                "passwords_domain_username_uniq",
                include_str!("../../migrations/20260918000001_passwords_uniq.sql"),
            ),
            (
                8,
                "passwords_uniq_null_fix",
                include_str!("../../migrations/20260918000002_passwords_uniq_fix.sql"),
            ),
            (
                9,
                "index_cleanup",
                include_str!("../../migrations/20260918000003_index_cleanup.sql"),
            ),
            (
                10,
                "drop_reading_list_dead_table",
                include_str!("../../migrations/20260920000000_drop_reading_list.sql"),
            ),
            (
                11,
                "mcp_tools",
                include_str!("../../migrations/20260920000001_mcp_tools.sql"),
            ),
            (
                12,
                "space_color",
                include_str!("../../migrations/20260921000000_space_color.sql"),
            ),
            (
                13,
                "userscripts",
                include_str!("../../migrations/20260921000001_userscripts.sql"),
            ),
            (
                14,
                "certificates",
                include_str!("../../migrations/20260922000000_certificates.sql"),
            ),
            (
                15,
                "feed_url_index",
                include_str!("../../migrations/20260922000001_feed_url_index.sql"),
            ),
            (
                16,
                "ai_audit",
                include_str!("../../migrations/20260925000000_ai_audit.sql"),
            ),
            (
                17,
                "translate_memory",
                include_str!("../../migrations/20260927000000_translate_memory.sql"),
            ),
            (
                18,
                "hibernate_defaults",
                include_str!("../../migrations/20260928000000_hibernate_defaults.sql"),
            ),
            (
                19,
                "perf_indexes",
                include_str!("../../migrations/20260928000001_perf_indexes.sql"),
            ),
            (
                20,
                "search_indexes",
                include_str!("../../migrations/20260928000002_search_indexes.sql"),
            ),
            (
                21,
                "translate_memory_hmac",
                include_str!("../../migrations/20261015000000_translate_memory_hmac.sql"),
            ),
        ];

        for (version, name, sql) in migrations {
            let applied: Option<i64> =
                sqlx::query_scalar("SELECT version FROM _migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional(pool)
                    .await?;

            if applied.is_none() {
                Self::apply_migration(pool, version, name, sql).await?;
                log::info!("[ShiftBrowser] 迁移 v{} {} 完成", version, name);
            }
        }

        Ok(())
    }

    async fn apply_migration(
        pool: &SqlitePool,
        version: i64,
        name: &str,
        sql: &str,
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        sqlx::raw_sql(sql).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO _migrations (version, name) VALUES (?, ?)")
            .bind(version)
            .bind(name)
            .execute(&mut *tx)
            .await?;
        tx.commit().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn failed_migration_rolls_back_sql_and_version_record() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE sample (value INTEGER NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sample (value) VALUES (1)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE _migrations (version INTEGER PRIMARY KEY, name TEXT NOT NULL)")
            .execute(&pool)
            .await
            .unwrap();

        let result = Db::apply_migration(
            &pool,
            21,
            "rollback_test",
            "UPDATE sample SET value = 2; INSERT INTO missing_table VALUES (1);",
        )
        .await;
        assert!(result.is_err());

        let value: i64 = sqlx::query_scalar("SELECT value FROM sample")
            .fetch_one(&pool)
            .await
            .unwrap();
        let recorded: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM _migrations WHERE version = 21")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(value, 1);
        assert_eq!(recorded, 0);
    }
}

/// 数据库错误类型
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("数据库错误: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("序列化错误: {0}")]
    Json(#[from] serde_json::Error),
    #[error("其他错误: {0}")]
    Other(String),
}

impl serde::Serialize for DbError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type DbResult<T> = Result<T, DbError>;

// 数据库连接与迁移管理
pub mod bookmarks;
pub mod downloads;
pub mod extensions;
pub mod history;
pub mod passwords;
pub mod sessions;
pub mod settings;
pub mod sync;
pub mod tab_groups;
pub mod users;
pub mod userscripts;
pub mod vault;
pub mod workspaces;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;

/// 全局数据库连接池（Tauri State 管理）
pub struct Db {
    pub pool: SqlitePool,
    /// 密码主密钥文件路径（独立于数据库，权限 600）
    pub key_path: PathBuf,
    /// 密码库主密码包裹文件路径（vault.key，权限 600；文件存在 = 已设主密码）
    pub vault_path: PathBuf,
    /// 密码主密钥内存缓存：进程内串行化首次生成（并发首用不会双写覆盖），
    /// 命中后不再重复读文件。tokio Mutex 保证跨 await 持有。
    pub master_key: tokio::sync::Mutex<Option<[u8; 32]>>,
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
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&app_data_dir, std::fs::Permissions::from_mode(0o700));
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
            .busy_timeout(std::time::Duration::from_secs(10));

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        // 安全：数据库含加密密码/设置，权限收紧到 600（仅当前用户可读写）。
        // 必须在 connect_with（此时才 create_if_missing 建文件）之后执行，
        // 否则首次运行时文件尚未创建，收紧不生效、以 umask 默认权限落盘
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600));
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

        log::info!("[ShiftBrowser] 数据库已初始化: {}", db_path.display());
        Ok(Self {
            pool,
            key_path: app_data_dir.join("master.key"),
            vault_path: app_data_dir.join("vault.key"),
            master_key: tokio::sync::Mutex::new(None),
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
        ];

        for (version, name, sql) in migrations {
            let applied: Option<i64> =
                sqlx::query_scalar("SELECT version FROM _migrations WHERE version = ?")
                    .bind(version)
                    .fetch_optional(pool)
                    .await?;

            if applied.is_none() {
                // 在事务中执行迁移
                let mut tx = pool.begin().await?;
                sqlx::raw_sql(sql).execute(&mut *tx).await?;
                sqlx::query("INSERT INTO _migrations (version, name) VALUES (?, ?)")
                    .bind(version)
                    .bind(name)
                    .execute(&mut *tx)
                    .await?;
                tx.commit().await?;
                log::info!("[ShiftBrowser] 迁移 v{} {} 完成", version, name);
            }
        }

        Ok(())
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

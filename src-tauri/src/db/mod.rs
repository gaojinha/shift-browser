// 数据库连接与迁移管理
pub mod bookmarks;
pub mod downloads;
pub mod extensions;
pub mod history;
pub mod passwords;
pub mod sessions;
pub mod settings;
pub mod tab_groups;
pub mod workspaces;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::PathBuf;

/// 全局数据库连接池（Tauri State 管理）
pub struct Db {
  pub pool: SqlitePool,
}

impl Db {
  /// 初始化数据库：创建目录、连接池、执行迁移
  pub async fn init(app_data_dir: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(&app_data_dir)?;
    let db_path = app_data_dir.join("shift-browser.db");

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

    // 开启 WAL 与外键约束
    sqlx::query("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
      .execute(&pool)
      .await?;

    // 执行迁移
    Self::run_migrations(&pool).await?;

    println!("[ShiftBrowser] 数据库已初始化: {}", db_path.display());
    Ok(Self { pool })
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
    let migrations: Vec<(i64, &str, &str)> = vec![(
      1,
      "init_core_tables",
      include_str!("../../migrations/20260905000000_init.sql"),
    ), (
      2,
      "feed_items",
      include_str!("../../migrations/20260913000000_feeds.sql"),
    )];

    for (version, name, sql) in migrations {
      let applied: Option<i64> = sqlx::query_scalar("SELECT version FROM _migrations WHERE version = ?")
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
        println!("[ShiftBrowser] 迁移 v{} {} 完成", version, name);
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
  #[error("记录未找到")]
  NotFound,
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
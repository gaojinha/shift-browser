// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

// 用户脚本（Tampermonkey 风格 · 阶段3.2 平台化）
// 每个脚本：name 名称、matches 匹配模式（换行分隔，WebKit allow_list 格式）、code JS 源码、enabled 开关。
// 注入：由 extensions.rs 的 inject_extensions 统一搭车（与扩展脚本同一套 UserScript 机制）。
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Userscript {
    pub id: i64,
    pub name: String,
    /// 匹配模式（换行分隔，WebKit allow_list 格式，如 `*://*.example.com/*`）
    pub matches: String,
    pub code: String,
    pub enabled: bool,
    pub created_at: String,
}

/// 列出全部用户脚本
#[tauri::command]
pub async fn list_userscripts(db: tauri::State<'_, Db>) -> DbResult<Vec<Userscript>> {
    let rows = sqlx::query_as::<_, (i64, String, String, String, i64, String)>(
        "SELECT id, name, matches, code, enabled, created_at FROM userscripts ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, name, matches, code, enabled, created_at)| Userscript {
                id,
                name,
                matches,
                code,
                enabled: enabled != 0,
                created_at,
            },
        )
        .collect())
}

/// 新增用户脚本（返回 id）
#[tauri::command]
pub async fn add_userscript(
    db: tauri::State<'_, Db>,
    name: String,
    matches: String,
    code: String,
) -> DbResult<i64> {
    let r =
        sqlx::query("INSERT INTO userscripts (name, matches, code, enabled) VALUES (?, ?, ?, 1)")
            .bind(name.trim())
            .bind(matches.trim())
            .bind(code)
            .execute(&db.pool)
            .await?;
    refresh_cache(&db.pool).await;
    Ok(r.last_insert_rowid())
}

/// 更新用户脚本（部分字段）
#[tauri::command]
pub async fn update_userscript(
    db: tauri::State<'_, Db>,
    id: i64,
    name: Option<String>,
    matches: Option<String>,
    code: Option<String>,
    enabled: Option<bool>,
) -> DbResult<()> {
    let mut tx = db.pool.begin().await?;
    if let Some(v) = name {
        sqlx::query("UPDATE userscripts SET name = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = matches {
        sqlx::query("UPDATE userscripts SET matches = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = code {
        sqlx::query("UPDATE userscripts SET code = ? WHERE id = ?")
            .bind(v)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(v) = enabled {
        sqlx::query("UPDATE userscripts SET enabled = ? WHERE id = ?")
            .bind(if v { 1 } else { 0 })
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    refresh_cache(&db.pool).await;
    Ok(())
}

/// 刷新用户脚本注入缓存（启动时调用；inject 路径不能 await，故缓存）
#[tauri::command]
pub async fn refresh_userscripts_cache(db: tauri::State<'_, Db>) -> DbResult<()> {
    refresh_cache(&db.pool).await;
    Ok(())
}

/// 删除用户脚本
#[tauri::command]
pub async fn delete_userscript(db: tauri::State<'_, Db>, id: i64) -> DbResult<()> {
    sqlx::query("DELETE FROM userscripts WHERE id = ?")
        .bind(id)
        .execute(&db.pool)
        .await?;
    refresh_cache(&db.pool).await;
    Ok(())
}

/// 注入缓存条目：(脚本源码, 注入时机/匹配等参数列表)
type InjectCache = std::sync::Mutex<Vec<(String, Vec<String>)>>;

/// 注入缓存（同步读；inject 在 tokio 上下文里不能 await/block_on，故用缓存）
static CACHE: std::sync::OnceLock<InjectCache> = std::sync::OnceLock::new();

fn cache() -> &'static InjectCache {
    CACHE.get_or_init(|| std::sync::Mutex::new(Vec::new()))
}

/// 刷新注入缓存（用户脚本增删改后 + 启动时调用）
pub async fn refresh_cache(pool: &sqlx::SqlitePool) {
    let scripts = enabled_scripts(pool).await;
    if let Ok(mut g) = cache().lock() {
        *g = scripts;
    }
}

/// 取注入缓存（同步；注入路径专用，不碰 DB/runtime）
pub fn cached_scripts() -> Vec<(String, Vec<String>)> {
    cache().lock().map(|g| g.clone()).unwrap_or_default()
}

/// 取启用的用户脚本（注入用）：返回 (code, 匹配模式列表)
pub async fn enabled_scripts(pool: &sqlx::SqlitePool) -> Vec<(String, Vec<String>)> {
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT code, matches FROM userscripts WHERE enabled = 1 AND code != '' ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    rows.into_iter()
        .map(|(code, matches)| {
            let patterns: Vec<String> = matches
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (code, patterns)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn pool() -> sqlx::SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20260921000001_userscripts.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        pool
    }

    async fn insert(pool: &sqlx::SqlitePool, matches: &str, code: &str, enabled: i64) {
        sqlx::query("INSERT INTO userscripts (name, matches, code, enabled) VALUES ('n', ?, ?, ?)")
            .bind(matches)
            .bind(code)
            .bind(enabled)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn enabled_scripts_filters_disabled_and_empty_code() {
        let pool = pool().await;
        insert(&pool, "*://a.com/*", "A", 1).await;
        insert(&pool, "*://b.com/*", "B", 0).await;
        insert(&pool, "*://c.com/*", "", 1).await;
        insert(&pool, "*://d.com/*", "D", 1).await;
        let got = enabled_scripts(&pool).await;
        let codes: Vec<&str> = got.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(codes, ["A", "D"]);
    }

    #[tokio::test]
    async fn enabled_scripts_parses_matches_lines() {
        let pool = pool().await;
        insert(&pool, " *://a.com/* \n\n\t\n*://b.com/*\r\n", "X", 1).await;
        insert(&pool, "", "Y", 1).await;
        let got = enabled_scripts(&pool).await;
        assert_eq!(got[0].1, ["*://a.com/*", "*://b.com/*"]);
        assert!(got[1].1.is_empty());
    }

    #[tokio::test]
    async fn enabled_scripts_empty_table_and_missing_table() {
        assert!(enabled_scripts(&pool().await).await.is_empty());
        let bare = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        assert!(enabled_scripts(&bare).await.is_empty());
    }

    // 全局 CACHE 只在本用例内读写，避免并行测试互相污染
    #[tokio::test]
    async fn refresh_cache_replaces_cached_scripts() {
        let pool = pool().await;
        insert(&pool, "*://a.com/*", "A", 1).await;
        refresh_cache(&pool).await;
        assert_eq!(
            cached_scripts(),
            vec![("A".to_string(), vec!["*://a.com/*".to_string()])]
        );
        sqlx::query("UPDATE userscripts SET enabled = 0")
            .execute(&pool)
            .await
            .unwrap();
        refresh_cache(&pool).await;
        assert!(cached_scripts().is_empty());
    }
}

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）
#![allow(clippy::unwrap_used, clippy::expect_used)]

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
        .map(|(id, name, matches, code, enabled, created_at)| Userscript {
            id,
            name,
            matches,
            code,
            enabled: enabled != 0,
            created_at,
        })
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

/// 注入缓存（同步读；inject 在 tokio 上下文里不能 await/block_on，故用缓存）
static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<(String, Vec<String>)>>> =
    std::sync::OnceLock::new();

fn cache() -> &'static std::sync::Mutex<Vec<(String, Vec<String>)>> {
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

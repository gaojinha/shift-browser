//! settings 内存缓存（性能 P0 · 2026-09-22）
//!
//! 问题：settings 是"高频读、低频写"数据，但 Rust 侧每次读都直连 SQLite
//! （页面加载查 record_history、休眠查 tab_hibernate_minutes、翻译查 engine/target/timeout…）。
//! 实测 39 处 `SELECT value FROM settings WHERE key`，其中多在热路径 → N+1 查库。
//!
//! 方案：进程内 `RwLock<HashMap>` 缓存
//! - 启动时 `warmup()` 全量预热
//! - `get(key)` 命中即返回；未命中回源并回填
//! - `set/delete` 写库后同步更新缓存（保持一致性）
//!
//! 注意：**敏感键（master_key）不进缓存**（避免明文常驻内存）。

use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// 不缓存的敏感键
const SKIP_KEYS: &[&str] = &["master_key"];

static CACHE: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn cache() -> &'static RwLock<HashMap<String, String>> {
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 启动时全量预热（在 Db::init 之后调用一次）
pub async fn warmup(pool: &SqlitePool) {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings")
        .fetch_all(pool)
        .await
        .unwrap_or_default();
    let mut map = HashMap::with_capacity(rows.len());
    for (k, v) in rows {
        if SKIP_KEYS.contains(&k.as_str()) {
            continue;
        }
        map.insert(k, v);
    }
    let n = map.len();
    if let Ok(mut c) = cache().write() {
        *c = map;
    }
    log::info!("[settings-cache] 预热完成：{} 项", n);
}

/// 读设置（缓存优先，未命中回源并回填）
pub async fn get(pool: &SqlitePool, key: &str) -> Option<String> {
    if SKIP_KEYS.contains(&key) {
        // 敏感键直连库（不缓存）
        return sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    }
    // 命中缓存
    if let Ok(c) = cache().read() {
        if let Some(v) = c.get(key) {
            return Some(v.clone());
        }
    }
    // 回源 + 回填
    let v: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    if let Some(val) = &v {
        if let Ok(mut c) = cache().write() {
            c.insert(key.to_string(), val.clone());
        }
    }
    v
}

/// 同步读（仅用于已在缓存中的热路径；未命中返回 None，调用方自行回源）
/// 注意：不能在 async 上下文里跨 await 持有读锁——本函数不 await，安全。
pub fn get_sync(key: &str) -> Option<String> {
    cache().read().ok().and_then(|c| c.get(key).cloned())
}

/// 写设置后同步更新缓存
pub fn update(key: &str, value: &str) {
    if SKIP_KEYS.contains(&key) {
        return;
    }
    if let Ok(mut c) = cache().write() {
        c.insert(key.to_string(), value.to_string());
    }
}

/// 删除设置后同步移除缓存
pub fn remove(key: &str) {
    if let Ok(mut c) = cache().write() {
        c.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_hit_and_update() {
        // 直接测内存缓存语义（不依赖真实库）
        update("homepage", "https://example.com");
        assert_eq!(get_sync("homepage").as_deref(), Some("https://example.com"));
        update("homepage", "https://bing.com");
        assert_eq!(get_sync("homepage").as_deref(), Some("https://bing.com"));
        remove("homepage");
        assert_eq!(get_sync("homepage"), None);
    }

    #[test]
    fn sensitive_keys_not_cached() {
        // master_key 不应进缓存
        update("master_key", "secret");
        assert_eq!(get_sync("master_key"), None);
    }
}

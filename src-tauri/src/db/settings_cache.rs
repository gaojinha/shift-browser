//! settings 内存缓存（性能 P0 · 2026-09-22；v2 白名单加固 · 2026-09-22）
//!
//! 问题：settings 是"高频读、低频写"数据，但 Rust 侧每次读都直连 SQLite
//! （页面加载查 record_history、休眠查 tab_hibernate_minutes、翻译查 engine/timeout…）。
//! 实测 39 处 `SELECT value FROM settings WHERE key`，其中多在热路径 → N+1 查库。
//!
//! 方案：进程内 `RwLock<HashMap>` 缓存 + **白名单**。
//!
//! v2 关键改动（2026-09-22 隐患收口）：**只缓存白名单键**。
//! 理由：settings 表有 39+ 处直接写（privacy/adblock/perms/useragent…多为裸 SQL 绕过 set_setting），
//! 若缓存"全量预热"，任何一处裸写一旦与读热路径重合，就会读到脏缓存。
//! 改为白名单后——非白名单键**一律直接回源（不缓存）**，从结构上保证"永远不会脏"：
//!   缓存 = 白名单键 ×（写走 set_setting 透传） → 一致性闭环。
//! 白名单选键原则 = **恰好是走 set_setting 写的热路径键**，其余（低频/裸写）不缓存。
//!
//! 白名单键（6 个 · 全部在 settings.ALLOWED_KEYS 内，写必然走 set_setting 透传）：
//!   - record_history          （页面加载 · tabs.rs 热路径）
//!   - tab_hibernate_minutes   （休眠时长 · tabs.rs 热路径）
//!   - tab_hibernate_count     （休眠标签数阈值 · tabs.rs 热路径，0 = 关闭数量阈值）
//!   - translate_engine/target/timeout（翻译 · translate.rs 热路径）
//!
//! 敏感键（master_key / ai_api_key）不在白名单 → 明文永不常驻内存，兼收安全收益。

use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

/// 白名单：**唯一**允许进缓存的热路径键。
/// 约束（硬契约）：白名单键必须都在 db/settings.rs 的 ALLOWED_KEYS 内
/// （保证写路径走 set_setting → 缓存透传 update），否则缓存永不命中/脏读。
const WHITELIST: &[&str] = &[
    // 页面加载 / 历史记录（tabs.rs）
    "record_history",
    // 内存水位休眠（tabs.rs）
    "tab_hibernate_minutes",
    // 内存水位休眠——标签数阈值（tabs.rs，0 = 关闭数量阈值）
    "tab_hibernate_count",
    // 翻译（translate.rs）
    "translate_engine",
    "translate_target",
    "translate_timeout",
];

static CACHE: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();

fn cache() -> &'static RwLock<HashMap<String, String>> {
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 是否允许进缓存（白名单判定，兼做敏感排除）
fn is_cachable(key: &str) -> bool {
    WHITELIST.contains(&key)
}

/// 启动时预热白名单（在 Db::init 之后调用一次）
pub async fn warmup(pool: &SqlitePool) {
    // 只预热白名单键：非白名单键本来就回源，预热只服务热路径。
    // 占位符数量随 WHITELIST 动态生成，不硬编码 bind 数量——
    // 增删白名单键不会越界 panic，也不会漏预热。
    let placeholders = vec!["?"; WHITELIST.len()].join(", ");
    let sql = format!("SELECT key, value FROM settings WHERE key IN ({})", placeholders);
    let mut query = sqlx::query_as::<_, (String, String)>(&sql);
    for k in WHITELIST {
        query = query.bind(*k);
    }
    let rows: Vec<(String, String)> = query.fetch_all(pool).await.unwrap_or_default();
    let mut map = HashMap::with_capacity(rows.len());
    for (k, v) in rows {
        map.insert(k, v);
    }
    let n = map.len();
    if let Ok(mut c) = cache().write() {
        *c = map;
    }
    log::info!("[settings-cache] 白名单预热完成：{} 项（共 {})", n, WHITELIST.len());
}

/// 读设置（白名单 → 缓存优先；非白名单 → 直接回源不缓存）
pub async fn get(pool: &SqlitePool, key: &str) -> Option<String> {
    if !is_cachable(key) {
        // 非白名单（敏感/裸写低频键）：永不进缓存，避免脏读
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
pub fn get_sync(key: &str) -> Option<String> {
    if !is_cachable(key) {
        return None; // 非白名单不缓存，同步读等价于未命中
    }
    cache().read().ok().and_then(|c| c.get(key).cloned())
}

/// 写设置后同步更新缓存（仅白名单键；非白名单不缓存自然无更新）
pub fn update(key: &str, value: &str) {
    if !is_cachable(key) {
        return;
    }
    if let Ok(mut c) = cache().write() {
        c.insert(key.to_string(), value.to_string());
    }
}

/// 删除设置后同步移除缓存（仅白名单键）
pub fn remove(key: &str) {
    if !is_cachable(key) {
        return;
    }
    if let Ok(mut c) = cache().write() {
        c.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_only() {
        // 白名单键允许进缓存
        assert!(is_cachable("record_history"));
        assert!(is_cachable("tab_hibernate_minutes"));
        assert!(is_cachable("tab_hibernate_count"));
        assert!(is_cachable("translate_engine"));
        // 非白名单（裸写/敏感）一律拒绝
        assert!(!is_cachable("master_key"));
        assert!(!is_cachable("ai_api_key"));
        assert!(!is_cachable("privacy_shield_level"));
        assert!(!is_cachable("user_agent"));
    }

    #[test]
    fn non_whitelist_never_stored() {
        // 模拟热路径误写非白名单键：缓存拒收
        update("ai_api_key", "sk-xxx");
        assert_eq!(get_sync("ai_api_key"), None);
        update("user_agent", "Mozilla/5.0");
        assert_eq!(get_sync("user_agent"), None);
        // 白名单正常
        update("record_history", "true");
        assert_eq!(get_sync("record_history").as_deref(), Some("true"));
        remove("record_history");
        assert_eq!(get_sync("record_history"), None);
    }
}

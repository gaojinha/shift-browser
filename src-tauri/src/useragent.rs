//! User-Agent 管理（方案 B · 2026-09-22）
//!
//! 背景：UA 写死 Chrome/120 导致"浏览器不受支持"提示；WebKitGTK 真身 UA 是
//! Safari/605 更老。本模块提供：
//! - 默认 UA 提升到主流版本（Chrome 131），并可**全局自定义**
//! - **站点级覆盖**（某些站需要特定 UA）
//! - Client Hints（userAgentData.brands）与 UA 版本**对齐**，避免"UA 说 Chrome、
//!   brands 里没 Chrome"被特性检测识破
//!
//! 设置项（settings 表）：
//! - `user_agent`       全局 UA（空 = 用默认）
//! - `ua_site_overrides` JSON: {"host": "UA"} 站点级覆盖

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// 默认 UA（Chrome 142 · 2026-09 主流版本；WebKitGTK 2.52 引擎足以支撑现代站点）
pub const DEFAULT_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";

/// 从 UA 里解析 Chrome 主版本号（用于 Client Hints brands 对齐）
pub fn chrome_major(ua: &str) -> u32 {
    ua.split("Chrome/")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(142)
}

/// 生成与 UA 版本对齐的 Client Hints brands JSON
pub fn brands_json(ua: &str) -> String {
    let v = chrome_major(ua);
    format!(
        r#"[{{"brand":"Not_A Brand","version":"24"}},{{"brand":"Chromium","version":"{v}"}},{{"brand":"Google Chrome","version":"{v}"}}]"#
    )
}

/// 全局 UA 计算核心（命令层与测试共用）：trim 后空串回落默认
pub fn normalized_global_ua(ua: &str) -> String {
    let val = ua.trim();
    if val.is_empty() {
        DEFAULT_UA.to_string()
    } else {
        val.to_string()
    }
}

/// 站点覆盖核心（命令层与测试共用）：host 归一化小写、空 host 拒绝、空 ua 删除覆盖
pub fn apply_site_override(
    m: &mut HashMap<String, String>,
    host: &str,
    ua: &str,
) -> Result<(), String> {
    let host = host.trim().to_lowercase();
    if host.is_empty() {
        return Err("host 不能为空".into());
    }
    if ua.trim().is_empty() {
        m.remove(&host);
    } else {
        m.insert(host, ua.trim().to_string());
    }
    Ok(())
}

static GLOBAL_UA: OnceLock<Mutex<String>> = OnceLock::new();
static SITE_OVERRIDES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn global_ua() -> &'static Mutex<String> {
    GLOBAL_UA.get_or_init(|| Mutex::new(DEFAULT_UA.to_string()))
}
fn overrides() -> &'static Mutex<HashMap<String, String>> {
    SITE_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 站点覆盖表 JSON（注入页面供 JS 按 location.host 动态对齐指纹用；
/// 注入是全局的、不分站点，JS 运行时自己查表，见 adblock wl_prefix 与 compat 脚本）
pub fn overrides_json() -> String {
    overrides()
        .lock()
        .map(|m| serde_json::to_string(&*m).unwrap_or_else(|_| "{}".into()))
        .unwrap_or_else(|_| "{}".into())
}

/// 生效 UA：站点覆盖优先，其次全局，最后默认
pub fn ua_for(host: &str) -> String {
    if !host.is_empty() {
        if let Ok(m) = overrides().lock() {
            if let Some(ua) = m.get(host) {
                if !ua.is_empty() {
                    return ua.clone();
                }
            }
        }
    }
    global_ua()
        .lock()
        .map(|s| s.clone())
        .unwrap_or_else(|_| DEFAULT_UA.to_string())
}

/// 启动时从 settings 载入
pub async fn load(pool: &SqlitePool) {
    let g: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'user_agent'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some(v) = g {
        if !v.trim().is_empty() {
            if let Ok(mut s) = global_ua().lock() {
                *s = v;
            }
        }
    }
    let o: Option<String> =
        sqlx::query_scalar("SELECT value FROM settings WHERE key = 'ua_site_overrides'")
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
    if let Some(json) = o {
        if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&json) {
            if let Ok(mut m) = overrides().lock() {
                *m = map;
            }
        }
    }
    log::info!("[ua] 已载入：全局={:?}", ua_for(""));
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UaConfig {
    pub global: String,
    pub default: String,
    pub overrides: HashMap<String, String>,
}

#[tauri::command]
pub async fn get_user_agent_config() -> Result<UaConfig, String> {
    Ok(UaConfig {
        global: ua_for(""),
        default: DEFAULT_UA.to_string(),
        overrides: overrides().lock().map(|m| m.clone()).unwrap_or_default(),
    })
}

/// settings 表 upsert（命令层与测试共用）
async fn persist_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(key)
    .bind(value)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 设置全局 UA（空 = 恢复默认）
#[tauri::command]
pub async fn set_user_agent(db: tauri::State<'_, crate::db::Db>, ua: String) -> Result<(), String> {
    let val = ua.trim().to_string();
    {
        let mut s = global_ua().lock().map_err(|e| e.to_string())?;
        *s = normalized_global_ua(&ua);
    }
    persist_setting(&db.pool, "user_agent", &val).await?;
    log::info!("[ua] 全局 UA 已更新");
    Ok(())
}

/// 设置站点级 UA 覆盖（ua 为空 = 删除该站点覆盖）
#[tauri::command]
pub async fn set_user_agent_override(
    db: tauri::State<'_, crate::db::Db>,
    host: String,
    ua: String,
) -> Result<(), String> {
    let json = {
        let mut m = overrides().lock().map_err(|e| e.to_string())?;
        apply_site_override(&mut m, &host, &ua)?;
        serde_json::to_string(&*m).unwrap_or_else(|_| "{}".into())
    };
    persist_setting(&db.pool, "ua_site_overrides", &json).await?;
    log::info!("[ua] 站点覆盖已更新: {}", host.trim().to_lowercase());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有全局静态可变状态的测试统一串行（GLOBAL_UA / SITE_OVERRIDES 进程级共享）；
    /// 用 tokio 互斥锁：状态类测试均 async，锁跨 await 不中毒
    static STATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// 复位全局静态到默认态（每个状态类测试开头调用，防用例间状态泄漏）
    fn reset_ua_state() {
        overrides().lock().unwrap().clear();
        *global_ua().lock().unwrap() = DEFAULT_UA.to_string();
    }

    /// 建一个跑完整迁移的真实库（settings 表齐全），同 newtab.rs temp_db 模式
    async fn temp_db() -> (tempfile::TempDir, crate::db::Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = crate::db::Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    /// 把进程级静态状态复位到已知基线（改状态的测试必须先调，保证与执行顺序无关）
    fn reset_shared_state() {
        *global_ua().lock().unwrap_or_else(|e| e.into_inner()) = DEFAULT_UA.to_string();
        overrides().lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// 直写一行 settings（测试造数用；绕开命令层）
    async fn seed_setting(pool: &SqlitePool, key: &str, value: &str) {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .unwrap();
    }

    // ── 纯函数：chrome_major ────────────────────────────────────────

    #[test]
    fn chrome_major_parses_version_from_ua() {
        assert_eq!(chrome_major(DEFAULT_UA), 142);
        assert_eq!(
            chrome_major("Mozilla/5.0 (X11; Linux x86_64) Chrome/131.0.0.0 Safari/537.36"),
            131
        );
    }

    #[test]
    fn chrome_major_falls_back_to_142_on_missing_or_malformed() {
        // 缺 Chrome 标记（WebKitGTK 真身）
        assert_eq!(chrome_major("Mozilla/5.0 (X11) AppleWebKit/605.1.15 Safari/605"), 142);
        // 畸变输入：非数字 / 空串
        assert_eq!(chrome_major("Chrome/abc"), 142);
        assert_eq!(chrome_major(""), 142);
    }

    // ── 纯函数：brands_json ─────────────────────────────────────────

    #[test]
    fn brands_json_is_valid_and_aligned_with_chrome_major() {
        let json = brands_json("Chrome/131.0.0.0 Safari/537.36");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("brands_json 必须是合法 JSON");
        let brands = parsed.as_array().unwrap();
        assert_eq!(brands.len(), 3);
        // Chromium 与 Google Chrome 两项版本都要对齐 chrome_major
        for brand in brands {
            let name = brand["brand"].as_str().unwrap();
            if name == "Chromium" || name == "Google Chrome" {
                assert_eq!(brand["version"].as_str().unwrap(), "131");
            }
        }
        // 默认 UA → 142 对齐
        assert!(brands_json(DEFAULT_UA).contains(r#""version":"142""#));
    }

    // ── 状态函数：ua_for / overrides_json（串行） ───────────────────

    #[tokio::test]
    async fn ua_for_prefers_site_override_then_global_and_skips_empty_value() {
        let _g = STATE_LOCK.lock().await;
        reset_ua_state();
        assert_eq!(ua_for(""), DEFAULT_UA); // 空 host 回落全局
        // 站点覆盖优先于全局
        overrides()
            .lock()
            .unwrap()
            .insert("example.com".into(), "TestUA/9.0".into());
        assert_eq!(ua_for("example.com"), "TestUA/9.0");
        assert_eq!(ua_for("other.com"), DEFAULT_UA);
        // 覆盖值为空串 → 视为无覆盖，回落全局
        overrides()
            .lock()
            .unwrap()
            .insert("empty.com".into(), String::new());
        assert_eq!(ua_for("empty.com"), DEFAULT_UA);
    }

    #[tokio::test]
    async fn overrides_json_empty_table_returns_empty_object() {
        let _g = STATE_LOCK.lock().await;
        reset_ua_state();
        let json = overrides_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("overrides_json 必须是合法 JSON");
        assert!(parsed.as_object().unwrap().is_empty());
    }

    // ── 抽出的命令核心：normalized_global_ua / apply_site_override ──

    #[test]
    fn normalized_global_ua_trims_and_falls_back_to_default_on_empty() {
        assert_eq!(normalized_global_ua(""), DEFAULT_UA);
        assert_eq!(normalized_global_ua("   "), DEFAULT_UA);
        assert_eq!(normalized_global_ua("  Chrome/77.0  "), "Chrome/77.0");
    }

    #[test]
    fn apply_site_override_rejects_empty_host() {
        let mut m = HashMap::new();
        assert!(apply_site_override(&mut m, "", "SomeUA/1").is_err());
        assert!(apply_site_override(&mut m, "   ", "SomeUA/1").is_err());
        assert!(m.is_empty());
    }

    #[test]
    fn apply_site_override_lowercases_host_and_empty_ua_removes() {
        let mut m = HashMap::new();
        apply_site_override(&mut m, "  Example.COM ", "SomeUA/1").unwrap();
        assert_eq!(m.get("example.com").map(String::as_str), Some("SomeUA/1"));
        // 空 ua = 删除覆盖
        apply_site_override(&mut m, "example.com", "   ").unwrap();
        assert!(!m.contains_key("example.com"));
    }

    // ── DB 命令层：persist_setting + load（真实库） ─────────────────

    #[tokio::test]
    // 测试专用串行锁（整段测试为单一临界区，#[tokio::test] 单线程运行时，无实际阻塞风险）
    #[allow(clippy::await_holding_lock)]
    async fn persist_setting_upserts_and_load_reads_back() {
        let _g = STATE_LOCK.lock().unwrap();
        reset_shared_state();
        let (_dir, db) = temp_db().await;
        // 首次插入
        persist_setting(&db.pool, "user_agent", "Chrome/100.0.0.0 persist").await.unwrap();
        // 冲突更新
        persist_setting(&db.pool, "user_agent", "Chrome/101.0.0.0 persist").await.unwrap();
        // 命令层语义：全局 UA 空串落库为空串（命令写 trim 后的值）
        persist_setting(&db.pool, "ua_site_overrides", r#"{"db.com":"DbUA/1"}"#).await.unwrap();
        // load 读回并生效到内存状态
        load(&db.pool).await;
        assert_eq!(ua_for(""), "Chrome/101.0.0.0 persist");
        assert_eq!(ua_for("db.com"), "DbUA/1");
        assert!(overrides_json().contains("db.com"));
    }

    #[tokio::test]
    // 同上：测试专用串行锁
    #[allow(clippy::await_holding_lock)]
    async fn load_keeps_default_when_user_agent_blank_or_absent() {
        let _g = STATE_LOCK.lock().unwrap();
        reset_shared_state();
        let (_dir, db) = temp_db().await;
        // 不写任何 settings → 全部缺省
        load(&db.pool).await;
        assert_eq!(ua_for(""), DEFAULT_UA);
        // 空串 / 纯空白 → 不采纳，保持默认
        seed_setting(&db.pool, "user_agent", "   ").await;
        load(&db.pool).await;
        assert_eq!(ua_for(""), DEFAULT_UA);
        // 覆盖表畸变 JSON → 静默忽略，不炸
        seed_setting(&db.pool, "ua_site_overrides", "not-json").await;
        load(&db.pool).await;
        assert!(overrides_json().starts_with('{'));
    }
}

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

static GLOBAL_UA: OnceLock<Mutex<String>> = OnceLock::new();
static SITE_OVERRIDES: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

fn global_ua() -> &'static Mutex<String> {
    GLOBAL_UA.get_or_init(|| Mutex::new(DEFAULT_UA.to_string()))
}
fn overrides() -> &'static Mutex<HashMap<String, String>> {
    SITE_OVERRIDES.get_or_init(|| Mutex::new(HashMap::new()))
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

/// 设置全局 UA（空 = 恢复默认）
#[tauri::command]
pub async fn set_user_agent(db: tauri::State<'_, crate::db::Db>, ua: String) -> Result<(), String> {
    let val = ua.trim().to_string();
    {
        let mut s = global_ua().lock().map_err(|e| e.to_string())?;
        *s = if val.is_empty() {
            DEFAULT_UA.to_string()
        } else {
            val.clone()
        };
    }
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('user_agent', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(&val)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
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
    let host = host.trim().to_lowercase();
    if host.is_empty() {
        return Err("host 不能为空".into());
    }
    let json = {
        let mut m = overrides().lock().map_err(|e| e.to_string())?;
        if ua.trim().is_empty() {
            m.remove(&host);
        } else {
            m.insert(host.clone(), ua.trim().to_string());
        }
        serde_json::to_string(&*m).unwrap_or_else(|_| "{}".into())
    };
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES ('ua_site_overrides', ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    log::info!("[ua] 站点覆盖已更新: {}", host);
    Ok(())
}

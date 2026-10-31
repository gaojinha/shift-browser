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

/// CEF 构建的内核原生 UA（Chromium 154，对应 cef crate `154.5.0+154.0.34`）。
///
/// 为什么要单独一份：CEF 的 `User-Agent` 请求头由内核自己发（`Chrome/154.0.0.0`），
/// 应用层不覆盖它；品牌表（`navigator.userAgentData.brands`）必须与这个版本对齐。
/// 若沿用 WebKit 那套 `DEFAULT_UA`（142），就变成「UA 154 / brands 142」的新矛盾
/// ——正是 F-CEF-08 要修的那条硬伤（见 `.ai/evidence/20261026-0138-*.md` §4）。
///
/// **跟版：升级 cef crate 时同步改这里**，与 `DEFAULT_UA` 同一节奏。
#[cfg(feature = "cef-engine")]
pub const CEF_KERNEL_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36";

/// 品牌表派生：按引擎选 UA 源。
///
/// - CEF 构建：用内核原生 UA（154），保证 `sec-ch-ua` 与 `user-agent` 自洽
/// - WebKit 构建：沿用应用层 UA（142，伪装机制不变）
pub fn brands_json_for_engine() -> String {
    #[cfg(feature = "cef-engine")]
    {
        brands_json(CEF_KERNEL_UA)
    }
    #[cfg(not(feature = "cef-engine"))]
    {
        brands_json(&ua_for(""))
    }
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
#[path = "useragent_tests.rs"]
mod tests;

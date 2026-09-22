//! 隐私/广告设置命令（P1-3 补完 · 2026-09-22）
//!
//! 隐私白名单 / 防护档位 / 反指纹模式 / GPC / 广告拦截 的设置命令。原在 extensions.rs。

use super::*;

#[tauri::command]
pub async fn set_privacy_whitelist(
    db: State<'_, Db>,
    app: AppHandle,
    domains: Vec<String>,
) -> Result<(), String> {
    // 清洗：去空、去协议/路径、去重、限长
    let mut cleaned: Vec<String> = Vec::new();
    for d in domains {
        let s = d
            .trim()
            .to_lowercase()
            .replace("https://", "")
            .replace("http://", "");
        let s = s.split('/').next().unwrap_or("").trim().to_string();
        if s.is_empty() {
            continue;
        }
        // 简单域名格式校验（含点且无空格）
        if !s.contains('.') || s.contains(' ') {
            continue;
        }
        if !cleaned.iter().any(|x| x == &s) {
            cleaned.push(s);
        }
    }
    log::info!(
        "[extensions] set_privacy_whitelist: {} 个域名",
        cleaned.len()
    );
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_whitelist', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 读取 WebMCP polyfill 试验开关（阶段 1.5，默认关）
#[tauri::command]
pub async fn get_webmcp_polyfill(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'webmcp_polyfill'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|(v,)| v == "true").unwrap_or(false))
}

/// 设置 WebMCP polyfill 试验开关（存设置 + 立即重新注入）
#[tauri::command]
pub async fn set_webmcp_polyfill(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_webmcp_polyfill: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('webmcp_polyfill', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if enabled { "true" } else { "false" })
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 读取加噪白名单
#[tauri::command]
pub async fn get_privacy_whitelist(db: State<'_, Db>) -> Result<Vec<String>, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_whitelist'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row
        .and_then(|(v,)| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default())
}

/// 隐私防护档位设置（off / standard / strict），存设置 + 立即重新注入
#[tauri::command]
pub async fn set_privacy_shield_level(
    db: State<'_, Db>,
    app: AppHandle,
    level: String,
) -> Result<(), String> {
    let level = match level.as_str() {
        "off" | "standard" | "strict" => level,
        other => return Err(format!("未知的隐私防护档位: {}", other)),
    };
    log::info!("[extensions] set_privacy_shield_level: {}", level);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield_level', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&level)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    // 同步旧布尔键，避免两处状态不一致
    let bool_val = if level == "off" { "false" } else { "true" };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(bool_val)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 隐私防护档位查询（默认 standard）
#[tauri::command]
pub async fn get_privacy_shield_level(db: State<'_, Db>) -> Result<String, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield_level'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some((l,)) = row {
        return Ok(l);
    }
    let srow: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(match srow.map(|r| r.0).as_deref() {
        Some("false") => "off".to_string(),
        _ => "standard".to_string(),
    })
}

/// 归一化反指纹模式（阶段 2）：只接受 noise / uniform，其余（含 None）回落 noise。
pub fn normalize_fp_mode(v: Option<&str>) -> &'static str {
    match v {
        Some("uniform") => "uniform",
        _ => "noise",
    }
}

/// 反指纹模式设置（noise 加噪 / uniform 统一化），存设置 + 立即重新注入
#[tauri::command]
pub async fn set_privacy_fp_mode(
    db: State<'_, Db>,
    app: AppHandle,
    mode: String,
) -> Result<(), String> {
    let mode = match mode.as_str() {
        "noise" | "uniform" => mode,
        other => return Err(format!("未知的反指纹模式: {}", other)),
    };
    log::info!("[extensions] set_privacy_fp_mode: {}", mode);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_fp_mode', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&mode)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 反指纹模式查询（默认 noise）
#[tauri::command]
pub async fn get_privacy_fp_mode(db: State<'_, Db>) -> Result<String, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_fp_mode'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(normalize_fp_mode(row.map(|r| r.0).as_deref()).to_string())
}

/// 隐私防护开关（存设置 + 立即重新注入）
#[tauri::command]
pub async fn set_privacy_shield_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_privacy_shield_enabled: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 隐私防护当前状态（前端初始化用；默认开启）
#[tauri::command]
pub async fn get_privacy_shield_enabled(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0 != "false").unwrap_or(true))
}

/// 广告拦截开关（存设置 + 重新注入）
#[tauri::command]
pub async fn set_adblock_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_adblock_enabled: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_enabled', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 广告拦截当前状态（给前端初始化用）
#[tauri::command]
pub async fn get_adblock_enabled(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_enabled'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0 == "true").unwrap_or(true))
}

/// 读取用户自定义广告关键词（不含内置）
#[tauri::command]
pub async fn get_adblock_keywords(db: State<'_, Db>) -> Result<Vec<String>, String> {
    Ok(adblock::load_custom_keywords(&db).await)
}

/// 保存用户自定义广告关键词并重新注入（立刻对已开标签生效，刷新页面后看到效果）
#[tauri::command]
pub async fn set_adblock_keywords(
    db: State<'_, Db>,
    app: AppHandle,
    keywords: Vec<String>,
) -> Result<(), String> {
    let cleaned: Vec<String> = keywords
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] set_adblock_keywords: {} 个自定义词",
        cleaned.len()
    );
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_keywords', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}
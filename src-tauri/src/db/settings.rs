// 设置管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

/// 敏感设置键（禁止返回/写入前端）
const SENSITIVE_KEYS: &[&str] = &["master_key"];

/// 前端允许读写的设置键白名单（防止非法键写入垃圾数据）
const ALLOWED_KEYS: &[&str] = &[
    "homepage",
    "search_engine",
    "theme",
    "show_bookmarks_bar",
    "record_history",
    "ai_model",
    "ai_base_url",
    "ai_api_key",
    "adblock_enabled",
    "adblock_keywords",
    "show_lunar",
    "show_quote",
    "show_weather",
    "font_size",
    "weather_city",
    "theme_color",
    "wallpaper",
    "show_bg",
    "lock_enabled",
    // 隐私防护（2026-09-18 补入：此前缺失导致"改变隐私设置不能保存"）
    "privacy_shield",
    "privacy_shield_level",
    "privacy_whitelist",
    // 性能：空闲标签休眠超时（分钟，0=关闭）
    "tab_hibernate_minutes",
];

/// 是否为敏感设置键
fn is_sensitive(key: &str) -> bool {
    SENSITIVE_KEYS.contains(&key)
}

/// 是否为白名单允许的键
fn is_allowed(key: &str) -> bool {
    ALLOWED_KEYS.contains(&key)
}

/// API Key 掩码（保留前 4 后 4，中间 ****；太短则全掩）
/// 按字符操作而非字节切片——多字节字符（中文等）在字节边界切片会 panic
fn mask_api_key(v: &str) -> String {
    let v = v.trim();
    let cs: Vec<char> = v.chars().collect();
    if cs.len() <= 8 {
        "****".to_string()
    } else {
        let head: String = cs[..4].iter().collect();
        let tail: String = cs[cs.len() - 4..].iter().collect();
        format!("{}****{}", head, tail)
    }
}

/// 获取单个设置
#[tauri::command]
pub async fn get_setting(db: tauri::State<'_, Db>, key: String) -> DbResult<Option<String>> {
    // 主密钥等敏感设置不允许前端读取
    if is_sensitive(&key) {
        return Ok(None);
    }
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(&key)
        .fetch_optional(&db.pool)
        .await?;
    // 与 get_all_settings 的脱敏契约一致：ai_api_key 返回掩码，明文 key 不出库
    Ok(row.map(|r| {
        if key == "ai_api_key" && !r.0.is_empty() {
            mask_api_key(&r.0)
        } else {
            r.0
        }
    }))
}

/// 获取所有设置（敏感项脱敏：master_key 不返回，ai_api_key 返回掩码）
#[tauri::command]
pub async fn get_all_settings(db: tauri::State<'_, Db>) -> DbResult<Vec<Setting>> {
    log::info!("[settings] get_all_settings 被调用");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT key, value FROM settings ORDER BY key")
            .fetch_all(&db.pool)
            .await?;
    let keys: Vec<String> = rows.iter().map(|(k, _)| k.clone()).collect();
    log::info!(
        "[settings] get_all_settings 返回 {} 项: {:?}",
        rows.len(),
        keys
    );
    let has_lock = rows.iter().find(|(k, _)| k == "lock_enabled");
    log::info!(
        "[settings] lock_enabled 原始值: {:?}",
        has_lock.map(|(_, v)| v.as_str())
    );
    Ok(rows
        .into_iter()
        .filter(|(key, _)| !is_sensitive(key))
        .map(|(key, value)| Setting {
            key: key.clone(),
            value: if key == "ai_api_key" && !value.is_empty() {
                mask_api_key(&value)
            } else {
                value
            },
        })
        .collect())
}

/// 设置单个值
#[tauri::command]
pub async fn set_setting(db: tauri::State<'_, Db>, key: String, value: String) -> DbResult<()> {
    // 主密钥等敏感设置不允许前端写入
    if is_sensitive(&key) {
        return Err(crate::db::DbError::Other(format!(
            "设置 {} 受保护，不允许通过前端修改",
            key
        )));
    }
    // 白名单校验：未知键拒绝写入，防止垃圾数据/错别字键
    if !is_allowed(&key) {
        return Err(crate::db::DbError::Other(format!(
            "设置 {} 不在白名单内，拒绝写入",
            key
        )));
    }
    // 掩码防御：前端把"****xxxx"这类掩码回写会盖掉真实密钥/令牌。
    // 前端读到的敏感值是掩码（get_all_settings 里已 mask），回写黑洞会毁掉真实值。
    // master_key 走上面的 is_sensitive 早拒；ai_api_key 是"前端可写+读回掩码"，
    // 必须在入库前拦掉掩码——否则前端整表回写 settings 就让真实 key 变成 "sk-****"。
    if (is_sensitive(&key) || key == "ai_api_key") && value.contains("****") {
        return Err(crate::db::DbError::Other(format!(
            "设置 {} 收到掩码回写（含 ****），已拒绝。真实值不会写库，请重新输入完整值",
            key
        )));
    }
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(&key)
    .bind(&value)
    .execute(&db.pool)
    .await?;
    Ok(())
}

/// 校验键是否允许前端删除（敏感键 + 白名单双重把关）
fn check_deletable(key: &str) -> Result<(), String> {
    if is_sensitive(key) {
        return Err(format!("设置 {} 受保护，不允许通过前端删除", key));
    }
    if !is_allowed(key) {
        return Err(format!("设置 {} 不在白名单内，拒绝删除", key));
    }
    Ok(())
}

/// 删除设置
///
/// 安全：与 set_setting 同样做白名单校验。原先无任何检查，前端（或注入的脚本）
/// 可删除任意键——包括 master_key（会切断已存密码的解密链）和未知系统键。
#[tauri::command]
pub async fn delete_setting(db: tauri::State<'_, Db>, key: String) -> DbResult<()> {
    check_deletable(&key).map_err(crate::db::DbError::Other)?;
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(&key)
        .execute(&db.pool)
        .await?;
    Ok(())
}

/// 批量设置（用于导入配置）
#[tauri::command]
pub async fn set_settings_bulk(db: tauri::State<'_, Db>, settings: Vec<Setting>) -> DbResult<()> {
    set_settings_bulk_inner(&db, settings).await
}

/// 批量设置内部实现（抽出来便于测试）
async fn set_settings_bulk_inner(db: &Db, settings: Vec<Setting>) -> DbResult<()> {
    let mut tx = db.pool.begin().await?;
    for s in settings {
        // 敏感设置不允许批量写入
        if is_sensitive(&s.key) {
            continue;
        }
        // 白名单校验：未知键跳过（批量导入容忍脏数据，单个 set 才报错）
        if !is_allowed(&s.key) {
            continue;
        }
        // 掩码回写防护（与 set_setting 一致）：ai_api_key 收到掩码形态（读回值）
        // 时跳过——防止"读全部→改→整表存"把真实 key 覆盖成 "sk-****"。
        // 批量场景静默跳过（不整批失败），单键 set 才报错。
        if s.key == "ai_api_key" && s.value.contains("****") {
            continue;
        }
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        )
        .bind(&s.key)
        .bind(&s.value)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;

    /// P3-3：删除设置的白名单/敏感键校验
    #[test]
    fn test_check_deletable() {
        // 白名单内的普通键：可删
        assert!(check_deletable("homepage").is_ok());
        assert!(check_deletable("theme_color").is_ok());
        assert!(check_deletable("adblock_keywords").is_ok());

        // 敏感键：即使将来误加进 ALLOWED_KEYS 也必须拒绝
        assert!(check_deletable("master_key").is_err());

        // 未知键 / 空键 / 疑似注入：拒绝
        assert!(check_deletable("__evil__").is_err());
        assert!(check_deletable("").is_err());
        assert!(check_deletable("homepage; DROP TABLE settings").is_err());
    }

    /// API Key 掩码：多字节字符不应 panic，且不泄露中段
    #[test]
    fn test_mask_api_key() {
        assert_eq!(mask_api_key(""), "****");
        assert_eq!(mask_api_key("short"), "****");
        assert_eq!(mask_api_key("12345678"), "****");
        assert_eq!(mask_api_key("sk-1234567890abcd"), "sk-1****abcd");
        // 中文/多字节：不应 panic
        assert_eq!(mask_api_key("密钥密钥密钥密钥密钥"), "密钥密钥****密钥密钥");
    }

    /// P1-2：批量写入遇 ai_api_key 掩码形态必须跳过（读回→整表存会覆盖真实 key）
    #[tokio::test]
    async fn test_set_settings_bulk_skips_masked_api_key() {
        use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("test.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let db = Db {
            pool,
            key_path: dir.path().join("master.key"),
            master_key: tokio::sync::Mutex::new(None),
        };

        // 先写入真实 key
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES ('ai_api_key', 'sk-real-1234567890')",
        )
        .execute(&db.pool)
        .await
        .unwrap();

        // 批量写入携带掩码形态 + 正常键
        set_settings_bulk_inner(
            &db,
            vec![
                Setting {
                    key: "ai_api_key".into(),
                    value: "sk-re****7890".into(),
                },
                Setting {
                    key: "homepage".into(),
                    value: "https://example.com".into(),
                },
            ],
        )
        .await
        .unwrap();

        // 真实 key 必须保留，homepage 正常写入
        let real: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'ai_api_key'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(real, "sk-real-1234567890", "掩码回写不得覆盖真实 key");
        let hp: String = sqlx::query_scalar("SELECT value FROM settings WHERE key = 'homepage'")
            .fetch_one(&db.pool)
            .await
            .unwrap();
        assert_eq!(hp, "https://example.com");
    }
}

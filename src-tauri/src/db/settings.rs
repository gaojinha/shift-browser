// 设置管理
use crate::db::{Db, DbResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Setting {
    pub key: String,
    pub value: String,
}

// 设置键白名单与敏感键来自 contracts.rs（单一来源）——后端校验与前端默认值
// 由 build.rs 从同一份定义生成，杜绝白名单漂移与"前端缺默认值 → get() 空串失真"。

/// 是否为敏感设置键
fn is_sensitive(key: &str) -> bool {
    crate::contracts::settings::is_sensitive(key)
}

/// 是否为凭据键（读只给掩码，写拦掩码回写）
fn is_masked(key: &str) -> bool {
    crate::contracts::settings::is_masked(key)
}

/// 是否为白名单允许的键
fn is_allowed(key: &str) -> bool {
    crate::contracts::settings::is_allowed(key)
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
    // 未知键直接拒（读侧白名单：库里若有来路不明的键也不外露，fail-closed）
    if !is_allowed(&key) {
        return Ok(None);
    }
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(&key)
        .fetch_optional(&db.pool)
        .await?;
    // 脱敏契约：凭据键返回掩码，明文不出库（ai_api_key / opencode_password）
    Ok(row.map(|r| {
        if is_masked(&key) && !r.0.is_empty() {
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
        // 敏感键不出库；未知键（白名单外）也不出库，防来路不明的键外泄
        .filter(|(key, _)| !is_sensitive(key) && is_allowed(key))
        .map(|(key, value)| Setting {
            key: key.clone(),
            value: if is_masked(&key) && !value.is_empty() {
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
    // 内部键仅专用命令读写，通用入口拒绝直写（P1-1 暗键收编：白名单是"存在的键"，内部键是"存在的键"但不可通用写）
    if crate::contracts::settings::is_internal(&key) {
        return Err(crate::db::DbError::Other(format!(
            "设置 {} 为内部键，仅专用命令可写",
            key
        )));
    }
    // 掩码防御：前端把"****xxxx"这类掩码回写会盖掉真实密钥/令牌。
    // 前端读到的凭据值是掩码（get_all_settings 里已 mask），回写黑洞会毁掉真实值。
    // master_key 走上面的 is_sensitive 早拒；凭据键（ai_api_key / opencode_password）
    // 必须在入库前拦掉掩码——否则前端整表回写 settings 就让真实值变成掩码串。
    if (is_sensitive(&key) || is_masked(&key)) && value.contains("****") {
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
    // 性能 P0：同步更新缓存，保持一致性
    crate::db::settings_cache::update(&key, &value);
    // 开发者工具开关即时生效（免重启）：新开 webview 创建时读静态值，已开标签关掉重开即可
    sync_devtools_static(&key, &value);
    Ok(())
}

/// 开关类运行期静态同步：目前仅开发者工具（enable_devtools）需要。
/// 落库成功后调用——失败/回滚路径不调，静态与库永不分叉。
fn sync_devtools_static(key: &str, value: &str) {
    if key == "enable_devtools" {
        crate::engine::webview::DEVTOOLS_ENABLED
            .store(value == "true", std::sync::atomic::Ordering::Relaxed);
    }
}

/// 校验键是否允许前端删除（敏感键 + 白名单双重把关）
fn check_deletable(key: &str) -> Result<(), String> {
    if is_sensitive(key) {
        return Err(format!("设置 {} 受保护，不允许通过前端删除", key));
    }
    if !is_allowed(key) {
        return Err(format!("设置 {} 不在白名单内，拒绝删除", key));
    }
    if crate::contracts::settings::is_internal(key) {
        return Err(format!("设置 {} 为内部键，不允许通过前端删除", key));
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
    // 性能 P0：同步移除缓存
    crate::db::settings_cache::remove(&key);
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
    // 先收集待写缓存的键值对；commit 成功后才统一回填，避免事务中途失败/回滚时留下脏缓存
    let mut cache_updates: Vec<(String, String)> = Vec::new();
    for s in settings {
        // 敏感设置不允许批量写入
        if is_sensitive(&s.key) {
            continue;
        }
        // 白名单校验：未知键跳过（批量导入容忍脏数据，单个 set 才报错）
        if !is_allowed(&s.key) {
            continue;
        }
        // 掩码回写防护（与 set_setting 一致）：凭据键收到掩码形态（读回值）
        // 时跳过——防止"读全部→改→整表存"把真实值覆盖成掩码串。
        // 批量场景静默跳过（不整批失败），单键 set 才报错。
        if is_masked(&s.key) && s.value.contains("****") {
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
        cache_updates.push((s.key, s.value));
    }
    tx.commit().await?;
    // 性能 P0：commit 成功后统一同步缓存（避免回滚时缓存与库不一致）
    for (key, value) in cache_updates {
        crate::db::settings_cache::update(&key, &value);
        sync_devtools_static(&key, &value);
    }
    Ok(())
}

/// 把「网络层拦截规则」设置写入 Web 进程扩展的规则文件（阶段 3.2）。
/// 扩展有后台守护线程每 2 秒查文件 mtime，改完约 2 秒自动生效（热更新，免重启）。
/// 文件路径与应用数据目录一致（与扩展读取路径对齐）。
#[tauri::command]
pub async fn sync_ext_rules(db: tauri::State<'_, Db>) -> DbResult<()> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'net_block_rules'")
            .fetch_optional(&db.pool)
            .await?;
    let rules = row.map(|(v,)| v).unwrap_or_default();
    let path = db.vault_path.with_file_name("shift-ext-rules.txt");
    let mut content = String::from(
        "# ShiftBrowser 网络层拦截规则（由主程序写入，勿手改）\n# 每行一个 URL 子串，命中即取消该请求；# 开头为注释\n",
    );
    for line in rules.lines() {
        let l = line.trim();
        if !l.is_empty() {
            content.push_str(l);
            content.push('\n');
        }
    }
    std::fs::write(&path, content)
        .map_err(|e| crate::db::DbError::Other(format!("写扩展规则文件失败: {}", e)))?;
    log::info!("[settings] 已写网络层拦截规则文件: {}", path.display());

    // 自定义请求头文件（每行 `Name: Value`）
    let hrow: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'net_headers'")
            .fetch_optional(&db.pool)
            .await?;
    let hdrs = hrow.map(|(v,)| v).unwrap_or_default();
    let hpath = db.vault_path.with_file_name("shift-ext-headers.txt");
    let mut hcontent = String::from(
        "# ShiftBrowser 自定义请求头（由主程序写入，勿手改）\n# 每行 `Name: Value`，注入到每个出网请求；# 开头为注释\n",
    );
    // 网络层 GPC（开关化）：gpc_header != 'false' 时注入 Sec-GPC: 1
    let gpc_on =
        sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'gpc_header'")
            .fetch_optional(&db.pool)
            .await?
            .map(|(v,)| v)
            .unwrap_or_else(|| "true".to_string())
            != "false";
    if gpc_on {
        hcontent.push_str("Sec-GPC: 1\n");
    }
    for line in hdrs.lines() {
        let l = line.trim();
        if !l.is_empty() {
            hcontent.push_str(l);
            hcontent.push('\n');
        }
    }
    std::fs::write(&hpath, hcontent)
        .map_err(|e| crate::db::DbError::Other(format!("写请求头文件失败: {}", e)))?;
    log::info!("[settings] 已写自定义请求头文件: {}", hpath.display());
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

    /// 凭据键分类：master_key 敏感不可读，ai_api_key/opencode_password 读掩码，
    /// opencode_username 普通可读，未知键不在白名单
    #[test]
    fn test_credential_key_classes() {
        assert!(is_sensitive("master_key"));
        assert!(!is_sensitive("opencode_password"));
        assert!(is_masked("ai_api_key"));
        assert!(is_masked("opencode_password"));
        assert!(!is_masked("opencode_username"));
        assert!(!is_masked("homepage"));
        assert!(is_allowed("opencode_username"));
        assert!(is_allowed("opencode_password"));
        assert!(!is_allowed("__evil__"));
        assert!(!is_allowed("master_key"));
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
            pool: pool.clone(),
            feed_pool: pool,
            key_path: dir.path().join("master.key"),
            vault_path: dir.path().join("vault.key"),
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

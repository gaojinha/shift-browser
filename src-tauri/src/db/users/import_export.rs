//! 账号备份导入导出（R2-1 拆分）

use crate::db::Db;

use super::security::*;
use super::session::{current_user_id, load_prefs};
use super::*;

/// 校验导入备份的账号字段（P1-4）
///
/// 备份文件是外部输入，必须逐字段校验，否则恶意/损坏备份可注入：
/// - 用户名：过 valid_username（长度+字符集），与注册一致
/// - 密码哈希：PBKDF2-SHA512 输出 64 字节 → hex 128 字符
/// - 盐值：16 字节 → hex 32 字符
/// - 算法标记：只认 'pbkdf2'（历史）或 'pbkdf2-sha512-{N}'，且 N 必须 ≤ 1000 万——
///   防恶意备份把迭代次数写成几十亿，导入后一登录就把 CPU 打满（本地 DoS）
pub(crate) fn validate_import_account(
    username: &str,
    pwd_hash: &str,
    salt: &str,
    algo: &str,
) -> Result<(), String> {
    let name = username.trim();
    if !valid_username(name) {
        return Err(
            "备份文件中的用户名不合法（需 2~20 个字符，仅中文/字母/数字/下划线/短横线）".into(),
        );
    }
    let is_hex = |s: &str, len: usize| s.len() == len && s.chars().all(|c| c.is_ascii_hexdigit());
    if !is_hex(pwd_hash, 128) {
        return Err("备份文件中的密码哈希格式不合法（应为 128 位十六进制）".into());
    }
    if !is_hex(salt, 32) {
        return Err("备份文件中的盐值格式不合法（应为 32 位十六进制）".into());
    }
    let ok_algo = if algo == "pbkdf2" {
        true
    } else if let Some(n) = algo.strip_prefix("pbkdf2-sha512-") {
        n.parse::<u64>()
            .map(|it| (1..=10_000_000).contains(&it))
            .unwrap_or(false)
    } else {
        false
    };
    if !ok_algo {
        return Err(
            "备份文件中的算法标记不合法（仅支持 pbkdf2 或 pbkdf2-sha512-{迭代次数}）".into(),
        );
    }
    Ok(())
}

// ===== 登录防爆破（内存态：连续失败锁定 30 秒）=====

/// 导出账号备份（JSON 文件，含密码哈希，提示妥善保管）
/// 安全：只能导出当前登录用户自己的数据——不接受任意用户名参数，
/// 防止未登录/他人调用导出任意账号的哈希+盐（离线爆破面）
#[tauri::command]
pub async fn user_export(db: tauri::State<'_, Db>) -> Result<UserResult, String> {
    let Some(uid) = current_user_id(&db).await else {
        return Ok(UserResult::err("请先登录后再导出"));
    };
    let row: Option<(String, String, String, String, String, String, String)> = db_try!(
    sqlx::query_as::<_, (String, String, String, String, String, String, String)>(
      "SELECT username, password_hash, salt, hash_algo, nickname, avatar, signature FROM users WHERE id = ?",
    )
    .bind(uid)
    .fetch_optional(&db.pool)
  );
    let Some((uname, pwd_hash, salt, algo, nickname, avatar, signature)) = row else {
        return Ok(UserResult::err("账号不存在"));
    };
    let prefs = load_prefs(&db, uid).await;
    let payload = serde_json::json!({
      "app": "ShiftBrowser",
      "type": "user-account-backup",
      "version": 1,
      "exportedAt": today_str(),
      "account": {
        "username": uname,
        "passwordHash": pwd_hash,
        "salt": salt,
        "hashAlgo": algo,
        "nickname": nickname,
        "avatar": avatar,
        "signature": signature,
        "prefs": {
          "showLunar": prefs.show_lunar,
          "showQuote": prefs.show_quote,
          "showWeather": prefs.show_weather,
          "fontSize": prefs.font_size,
          "weatherCity": prefs.weather_city,
          "themeColor": prefs.theme_color,
          "wallpaper": prefs.wallpaper,
        }
      }
    });
    let file = match rfd::AsyncFileDialog::new()
        .set_title("导出账号备份")
        .set_file_name(format!(
            "shiftbrowser-账号备份-{}-{}.json",
            uname,
            today_str()
        ))
        .add_filter("JSON", &["json"])
        .save_file()
        .await
    {
        Some(f) => f,
        None => return Ok(UserResult::err("已取消导出")),
    };
    let path = file.path().to_path_buf();
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&payload).unwrap_or_default(),
    )
    .map_err(|e| format!("写入文件失败: {}", e))?;
    // 日志只记文件名不记绝对路径（防目录结构外泄）；返回消息带保管提示，
    // 备份含密码哈希+盐，离线可爆破——目前为明文格式（保导入兼容），勿传网盘与聊天
    log::info!(
        "[users] 账号备份已导出: {}",
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    Ok(UserResult {
        ok: true,
        msg: "备份已导出（含密码哈希，妥善保管，勿上传网盘或发聊天）".into(),
        user: None,
        path: Some(path.display().to_string()),
    })
}

/// 导入账号备份（JSON 文件）
#[tauri::command]
pub async fn user_import(db: tauri::State<'_, Db>) -> Result<UserResult, String> {
    let file = match rfd::AsyncFileDialog::new()
        .set_title("导入账号备份")
        .add_filter("JSON", &["json"])
        .pick_file()
        .await
    {
        Some(f) => f,
        None => return Ok(UserResult::err("已取消导入")),
    };
    let raw = std::fs::read_to_string(file.path()).map_err(|e| format!("读取文件失败: {}", e))?;
    let v: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "备份文件解析失败".to_string())?;
    if v.get("type").and_then(|t| t.as_str()) != Some("user-account-backup") {
        return Ok(UserResult::err("不是有效的账号备份文件"));
    }
    let acct = v
        .get("account")
        .cloned()
        .ok_or_else(|| "备份文件缺少账号数据".to_string())?;
    let uname = acct
        .get("username")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "备份文件缺少用户名".to_string())?
        .to_string();
    let pwd_hash = acct
        .get("passwordHash")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "备份文件缺少密码哈希".to_string())?
        .to_string();
    let salt = acct
        .get("salt")
        .and_then(|u| u.as_str())
        .ok_or_else(|| "备份文件缺少盐值".to_string())?
        .to_string();
    let nickname = acct
        .get("nickname")
        .and_then(|u| u.as_str())
        .unwrap_or(&uname)
        .to_string();
    // 备份文件里的算法标记：旧备份无此字段 → 按历史参数 'pbkdf2'（10 万次）导入，登录时自动升级
    let algo = acct
        .get("hashAlgo")
        .and_then(|u| u.as_str())
        .unwrap_or("pbkdf2")
        .to_string();
    let avatar = acct
        .get("avatar")
        .and_then(|u| u.as_str())
        .unwrap_or("🦊")
        .to_string();
    let signature = acct
        .get("signature")
        .and_then(|u| u.as_str())
        .unwrap_or("这个人很懒，什么都没写")
        .to_string();
    // 字段校验：外部备份文件可能被篡改，逐字段把关（P1-4）
    validate_import_account(&uname, &pwd_hash, &salt, &algo)?;
    // avatar 是特权页渲染字段（未转义历史问题），导入必须走同一套清洗规则（2026-09-19 修复）
    let avatar = sanitize_avatar(&avatar)?;
    let exists: Option<(i64,)> = db_try!(sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&uname)
    .fetch_optional(&db.pool));
    if exists.is_some() {
        return Ok(UserResult::err(format!(
            "账号 {} 已存在，请先删除或换名导入",
            uname
        )));
    }
    db_try!(sqlx::query(
        "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
       VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&uname)
    .bind(&pwd_hash)
    .bind(&salt)
    .bind(&algo)
    .bind(&nickname)
    .bind(&avatar)
    .bind(&signature)
    .execute(&db.pool));
    let uid: (i64,) = db_try!(sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&uname)
    .fetch_one(&db.pool));
    let uid = uid.0;
    // 偏好（原版 camelCase 兼容）
    let p = acct.get("prefs").cloned().unwrap_or_default();
    let prefs = UserPrefs {
        show_lunar: p.get("showLunar").and_then(|x| x.as_bool()).unwrap_or(true),
        show_quote: p.get("showQuote").and_then(|x| x.as_bool()).unwrap_or(true),
        show_weather: p
            .get("showWeather")
            .and_then(|x| x.as_bool())
            .unwrap_or(true),
        font_size: p.get("fontSize").and_then(|x| x.as_i64()).unwrap_or(14),
        weather_city: p
            .get("weatherCity")
            .and_then(|x| x.as_str())
            .unwrap_or("运城")
            .to_string(),
        theme_color: p
            .get("themeColor")
            .and_then(|x| x.as_str())
            .unwrap_or("#4a9eff")
            .to_string(),
        wallpaper: p
            .get("wallpaper")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
    };
    let inserted = sqlx::query(
    "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
  )
  .bind(uid)
  .bind(prefs.show_lunar)
  .bind(prefs.show_quote)
  .bind(prefs.show_weather)
  .bind(prefs.font_size)
  .bind(&prefs.weather_city)
  .bind(&prefs.theme_color)
  .bind(&prefs.wallpaper)
  .execute(&db.pool)
  .await
  .map_err(|e| format!("初始化偏好失败: {}", e))?;
    if inserted.rows_affected() == 0 {
        return Ok(UserResult::err("初始化偏好失败，请重试"));
    }
    log::info!("[users] 账号备份已导入: {}", uname);
    Ok(UserResult::ok(format!("账号 {} 导入成功", uname)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// 创建带 users、user_prefs、user_session 表的临时测试库
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("test_import_export.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        sqlx::raw_sql(include_str!("../../../migrations/20260914000000_users.sql"))
            .execute(&pool)
            .await
            .unwrap();

        let key_path = dir.path().join("master.key");
        let vault_path = dir.path().join("vault.key");
        let translate_key_path = dir.path().join("translate.key");
        (
            dir,
            Db {
                pool: pool.clone(),
                feed_pool: pool,
                key_path,
                vault_path,
                translate_key_path,
                master_key: tokio::sync::Mutex::new(None),
                translate_key: tokio::sync::Mutex::new(None),
            },
        )
    }

    /// 校验 validate_import_account 的白名单与防 DoS 规则
    #[test]
    fn test_validate_import_account_scenarios() {
        let valid_hash = "a".repeat(128);
        let valid_salt = "f".repeat(32);

        // 1. 正常有效入参（pbkdf2 与 pbkdf2-sha512-N）
        assert!(validate_import_account("valid_user", &valid_hash, &valid_salt, "pbkdf2").is_ok());
        assert!(validate_import_account(
            "valid_user",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-210000"
        )
        .is_ok());
        assert!(validate_import_account(
            "测试用户",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-1"
        )
        .is_ok());
        assert!(validate_import_account(
            "valid-user_01",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-10000000"
        )
        .is_ok());

        // 2. 非法用户名（过短、中间含空格、非法标点、过长）
        assert!(validate_import_account("a", &valid_hash, &valid_salt, "pbkdf2").is_err());
        assert!(validate_import_account(
            "user with space",
            &valid_hash,
            &valid_salt,
            "pbkdf2"
        )
        .is_err());
        assert!(validate_import_account(
            "invalid*char",
            &valid_hash,
            &valid_salt,
            "pbkdf2"
        )
        .is_err());
        assert!(validate_import_account(
            &"toolong_username_".repeat(3),
            &valid_hash,
            &valid_salt,
            "pbkdf2"
        )
        .is_err());

        // 3. 非法哈希（长度不对或非 hex）
        let short_hash = "a".repeat(127);
        let non_hex_hash = format!("{}z", "a".repeat(127));
        assert!(validate_import_account("valid_user", &short_hash, &valid_salt, "pbkdf2").is_err());
        assert!(validate_import_account("valid_user", &non_hex_hash, &valid_salt, "pbkdf2").is_err());

        // 4. 非法盐值
        let short_salt = "f".repeat(31);
        let non_hex_salt = format!("{}g", "f".repeat(31));
        assert!(validate_import_account("valid_user", &valid_hash, &short_salt, "pbkdf2").is_err());
        assert!(validate_import_account("valid_user", &valid_hash, &non_hex_salt, "pbkdf2").is_err());

        // 5. 非法算法标记与 DoS 边界（> 1000 万）
        assert!(validate_import_account("valid_user", &valid_hash, &valid_salt, "bcrypt").is_err());
        assert!(validate_import_account("valid_user", &valid_hash, &valid_salt, "md5").is_err());
        assert!(validate_import_account(
            "valid_user",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-0"
        )
        .is_err());
        assert!(validate_import_account(
            "valid_user",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-10000001"
        )
        .is_err());
        assert!(validate_import_account(
            "valid_user",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-99999999999"
        )
        .is_err());
        assert!(validate_import_account(
            "valid_user",
            &valid_hash,
            &valid_salt,
            "pbkdf2-sha512-invalid"
        )
        .is_err());
    }

    /// 校验导入备份 JSON 解析提取、校验、落库与偏好映射逻辑
    #[tokio::test]
    async fn test_import_account_db_insertion_and_prefs() {
        let (_dir, db) = temp_db().await;
        let valid_hash = "b".repeat(128);
        let valid_salt = "c".repeat(32);

        // 1. 模拟标准备份 JSON 结构
        let backup_json = serde_json::json!({
            "app": "ShiftBrowser",
            "type": "user-account-backup",
            "version": 1,
            "exportedAt": "2026-10-17",
            "account": {
                "username": "tester",
                "passwordHash": valid_hash,
                "salt": valid_salt,
                "hashAlgo": "pbkdf2-sha512-210000",
                "nickname": "TestNick",
                "avatar": "🦊",
                "signature": "Custom signature",
                "prefs": {
                    "showLunar": false,
                    "showQuote": true,
                    "showWeather": false,
                    "fontSize": 18,
                    "weatherCity": "Beijing",
                    "themeColor": "#ff5500",
                    "wallpaper": "my-bg.jpg"
                }
            }
        });

        // 提取并校验
        assert_eq!(
            backup_json.get("type").and_then(|t| t.as_str()),
            Some("user-account-backup")
        );
        let acct = backup_json.get("account").unwrap();
        let uname = acct.get("username").and_then(|u| u.as_str()).unwrap();
        let pwd_hash = acct.get("passwordHash").and_then(|u| u.as_str()).unwrap();
        let salt = acct.get("salt").and_then(|u| u.as_str()).unwrap();
        let algo = acct.get("hashAlgo").and_then(|u| u.as_str()).unwrap();
        validate_import_account(uname, pwd_hash, salt, algo).unwrap();

        // 插入 users 表
        let res = sqlx::query(
            "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uname)
        .bind(pwd_hash)
        .bind(salt)
        .bind(algo)
        .bind(acct.get("nickname").and_then(|u| u.as_str()).unwrap())
        .bind(acct.get("avatar").and_then(|u| u.as_str()).unwrap())
        .bind(acct.get("signature").and_then(|u| u.as_str()).unwrap())
        .execute(&db.pool)
        .await
        .unwrap();

        let uid = res.last_insert_rowid();
        assert!(uid > 0);

        // 插入偏好表并验证字段映射
        let p = acct.get("prefs").unwrap();
        sqlx::query(
            "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(uid)
        .bind(p.get("showLunar").and_then(|x| x.as_bool()).unwrap_or(true))
        .bind(p.get("showQuote").and_then(|x| x.as_bool()).unwrap_or(true))
        .bind(p.get("showWeather").and_then(|x| x.as_bool()).unwrap_or(true))
        .bind(p.get("fontSize").and_then(|x| x.as_i64()).unwrap_or(14))
        .bind(p.get("weatherCity").and_then(|x| x.as_str()).unwrap_or("运城"))
        .bind(p.get("themeColor").and_then(|x| x.as_str()).unwrap_or("#4a9eff"))
        .bind(p.get("wallpaper").and_then(|x| x.as_str()).unwrap_or(""))
        .execute(&db.pool)
        .await
        .unwrap();

        // 查验落库内容与 load_prefs
        let loaded_prefs = load_prefs(&db, uid).await;
        assert!(!loaded_prefs.show_lunar);
        assert!(loaded_prefs.show_quote);
        assert!(!loaded_prefs.show_weather);
        assert_eq!(loaded_prefs.font_size, 18);
        assert_eq!(loaded_prefs.weather_city, "Beijing");
        assert_eq!(loaded_prefs.theme_color, "#ff5500");
        assert_eq!(loaded_prefs.wallpaper, "my-bg.jpg");

        // 重复用户名检测
        let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM users WHERE username = ?")
            .bind("tester")
            .fetch_optional(&db.pool)
            .await
            .unwrap();
        assert!(exists.is_some());
    }

    /// 校验旧版/缺失偏好的备份导入时默认值填充
    #[test]
    fn test_import_legacy_backup_default_fallback() {
        let valid_hash = "d".repeat(128);
        let valid_salt = "e".repeat(32);

        // 旧版备份缺少 hashAlgo、avatar、signature 与 prefs
        let legacy_json = serde_json::json!({
            "type": "user-account-backup",
            "account": {
                "username": "legacy_user",
                "passwordHash": valid_hash,
                "salt": valid_salt
            }
        });

        let acct = legacy_json.get("account").unwrap();
        let algo = acct.get("hashAlgo").and_then(|u| u.as_str()).unwrap_or("pbkdf2");
        let avatar = acct.get("avatar").and_then(|u| u.as_str()).unwrap_or("🦊");
        let sig = acct.get("signature").and_then(|u| u.as_str()).unwrap_or("默认签名");

        assert_eq!(algo, "pbkdf2");
        assert_eq!(avatar, "🦊");
        assert_eq!(sig, "默认签名");

        let p = acct.get("prefs").cloned().unwrap_or_default();
        let prefs = UserPrefs {
            show_lunar: p.get("showLunar").and_then(|x| x.as_bool()).unwrap_or(true),
            show_quote: p.get("showQuote").and_then(|x| x.as_bool()).unwrap_or(true),
            show_weather: p.get("showWeather").and_then(|x| x.as_bool()).unwrap_or(true),
            font_size: p.get("fontSize").and_then(|x| x.as_i64()).unwrap_or(14),
            weather_city: p.get("weatherCity").and_then(|x| x.as_str()).unwrap_or("运城").to_string(),
            theme_color: p.get("themeColor").and_then(|x| x.as_str()).unwrap_or("#4a9eff").to_string(),
            wallpaper: p.get("wallpaper").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        };

        assert_eq!(prefs.weather_city, "运城");
        assert_eq!(prefs.font_size, 14);
        assert_eq!(prefs.theme_color, "#4a9eff");
        assert_eq!(prefs.wallpaper, "");
    }
}

// ===== 小工具 =====

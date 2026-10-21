//! 用户资料与偏好管理（R2-1 拆分）

use crate::db::Db;

use super::security::*;
use super::session::load_user;
use super::*;

/// 当前登录用户（未登录返回 ok=false，msg=未登录）
#[tauri::command]
pub async fn user_get_current(db: tauri::State<'_, Db>) -> Result<UserResult, String> {
    log::info!("[users] user_get_current 被调用");
    let Some(uid) = current_user_id(&db).await else {
        return Ok(UserResult::err("未登录"));
    };
    match load_user(&db, uid).await {
        Some(u) => Ok(UserResult::ok_user("已登录", u)),
        None => Ok(UserResult::err("未登录")),
    }
}

/// 校验/清洗头像字段（防 HTML 注入，2026-09-19 安全修复）
///
/// 背景：avatar 会未经转义渲染进特权新标签页（newtab.rs renderProfileLoggedIn），
/// 恶意 `<svg onload=...>` 可拿到 invoke key 接管全部 IPC。双管齐下：
/// - 任何形态都禁止 `<` `>`（杜绝 HTML 注入）
/// - data:image/ 开头仅允许 png/jpeg/gif/webp（排除 svg+xml 等可执行类型），限 300KB
/// - 非 data: 开头按 emoji/文本处理，限 20 字符
pub(crate) fn sanitize_avatar(raw: &str) -> Result<String, String> {
    let a = raw.trim();
    if a.contains('<') || a.contains('>') {
        return Err("头像含有非法字符".to_string());
    }
    if a.starts_with("data:image/") {
        if a.len() > 300_000 {
            return Err("头像图片过大（限 300KB）".to_string());
        }
        let mime_ok = a.starts_with("data:image/png;")
            || a.starts_with("data:image/jpeg;")
            || a.starts_with("data:image/jpg;")
            || a.starts_with("data:image/gif;")
            || a.starts_with("data:image/webp;");
        if !mime_ok {
            return Err("头像图片格式仅支持 png/jpeg/gif/webp".to_string());
        }
        Ok(a.to_string())
    } else {
        Ok(a.chars().take(20).collect())
    }
}

/// 更新个人资料（昵称/头像/签名）
#[tauri::command]
pub async fn user_update_profile(
    db: tauri::State<'_, Db>,
    nickname: Option<String>,
    avatar: Option<String>,
    signature: Option<String>,
) -> Result<UserResult, String> {
    let Some(uid) = current_user_id(&db).await else {
        return Ok(UserResult::err("未登录"));
    };
    let mut sets: Vec<String> = Vec::new();
    let mut binds: Vec<String> = Vec::new();
    if let Some(n) = nickname {
        let n: String = n.chars().take(30).collect();
        sets.push("nickname = ?".into());
        binds.push(n);
    }
    if let Some(a) = avatar {
        let a = sanitize_avatar(&a)?;
        sets.push("avatar = ?".into());
        binds.push(a);
    }
    if let Some(s) = signature {
        let s: String = s.chars().take(100).collect();
        sets.push("signature = ?".into());
        binds.push(s);
    }
    if sets.is_empty() {
        return Ok(UserResult::err("没有需要更新的字段"));
    }
    let mut sql = String::from("UPDATE users SET ");
    sql.push_str(&sets.join(", "));
    sql.push_str(" WHERE id = ?");
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    q = q.bind(uid);
    db_try!(q.execute(&db.pool));
    log::info!("[users] 用户资料已更新");
    match load_user(&db, uid).await {
        Some(u) => Ok(UserResult::ok_user("资料已更新", u)),
        None => Ok(UserResult::err("更新失败")),
    }
}

/// 修改密码
#[tauri::command]
pub async fn user_change_password(
    db: tauri::State<'_, Db>,
    old_password: String,
    new_password: String,
) -> Result<UserResult, String> {
    let Some(uid) = current_user_id(&db).await else {
        return Ok(UserResult::err("未登录"));
    };
    if old_password.is_empty() || new_password.is_empty() {
        return Ok(UserResult::err("旧密码和新密码不能为空"));
    }
    if new_password.chars().count() < 6 {
        return Ok(UserResult::err("新密码至少 6 位"));
    }
    let row: Option<(String, String, String)> =
        db_try!(sqlx::query_as::<_, (String, String, String)>(
            "SELECT password_hash, salt, hash_algo FROM users WHERE id = ?",
        )
        .bind(uid)
        .fetch_optional(&db.pool));
    let Some((stored_hash, salt, algo)) = row else {
        return Ok(UserResult::err("账号不存在"));
    };
    if hash_password_blocking(
        old_password.clone(),
        salt.clone(),
        iterations_for_algo(&algo),
    )
    .await
        != stored_hash
    {
        return Ok(UserResult::err("旧密码错误"));
    }
    let new_salt = generate_salt();
    let new_hash =
        hash_password_blocking(new_password.clone(), new_salt.clone(), PBKDF2_ITER).await;
    db_try!(sqlx::query(
        "UPDATE users SET password_hash = ?, salt = ?, hash_algo = ? WHERE id = ?"
    )
    .bind(&new_hash)
    .bind(&new_salt)
    .bind(HASH_ALGO_CURRENT)
    .bind(uid)
    .execute(&db.pool));
    log::info!("[users] 用户密码已修改");
    Ok(UserResult::ok("密码修改成功"))
}

/// 删除账号（需密码确认）
#[tauri::command]
pub async fn user_delete(
    db: tauri::State<'_, Db>,
    username: String,
    password: String,
) -> Result<UserResult, String> {
    let name = username.trim().to_string();
    if name.is_empty() || password.is_empty() {
        return Ok(UserResult::err("用户名和密码不能为空"));
    }
    let row: Option<(i64, String, String, String)> =
        db_try!(sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, password_hash, salt, hash_algo FROM users WHERE username = ?",
        )
        .bind(&name)
        .fetch_optional(&db.pool));
    let Some((uid, stored_hash, salt, algo)) = row else {
        return Ok(UserResult::err("账号不存在"));
    };
    if hash_password_blocking(password.clone(), salt, iterations_for_algo(&algo)).await
        != stored_hash
    {
        return Ok(UserResult::err("密码错误，无法删除"));
    }
    let was_current = current_user_id(&db).await == Some(uid);
    db_try!(sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(uid)
        .execute(&db.pool));
    if was_current {
        set_session(&db, None)
            .await
            .map_err(|e| format!("账号已删除但会话清理失败: {}", e))?;
        apply_prefs_to_settings(&db, &UserPrefs::default())
            .await
            .map_err(|e| format!("账号已删除但偏好恢复失败: {}", e))?;
    }
    log::info!("[users] 账号已删除: {}", name);
    Ok(UserResult::ok(if was_current {
        "账号已删除并退出登录"
    } else {
        "账号已删除"
    }))
}

/// 更新偏好（跟账号走，同时写入 settings 生效）
#[tauri::command]
pub async fn user_update_prefs(
    db: tauri::State<'_, Db>,
    prefs: UserPrefs,
) -> Result<UserResult, String> {
    let Some(uid) = current_user_id(&db).await else {
        return Ok(UserResult::err("未登录"));
    };
    let _ = sqlx::query(
    "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(user_id) DO UPDATE SET
       show_lunar = excluded.show_lunar,
       show_quote = excluded.show_quote,
       show_weather = excluded.show_weather,
       font_size = excluded.font_size,
       weather_city = excluded.weather_city,
       theme_color = excluded.theme_color,
       wallpaper = excluded.wallpaper",
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
  .map_err(|e| format!("偏好保存失败: {}", e))?;
    apply_prefs_to_settings(&db, &prefs)
        .await
        .map_err(|e| format!("偏好已保存但应用失败: {}", e))?;
    log::info!("[users] 账号偏好已更新");
    match load_user(&db, uid).await {
        Some(u) => Ok(UserResult::ok_user("偏好已保存", u)),
        None => Ok(UserResult::err("保存失败")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// 创建带 users、user_prefs、user_session 表的临时测试库
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("test_profile.db"))
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

    #[test]
    fn test_sanitize_avatar_clean_emoji_and_text() {
        // 普通文本与 emoji：trim 且截断至 20 字符
        assert_eq!(sanitize_avatar(" 🚀 ").unwrap(), "🚀");
        let normal = sanitize_avatar("my_avatar_123").unwrap();
        assert_eq!(normal, "my_avatar_123");

        let long_text = "abcdefghijklmnopqrstuvwxyz1234567890";
        let truncated = sanitize_avatar(long_text).unwrap();
        assert_eq!(truncated.chars().count(), 20);
        assert_eq!(truncated, &long_text[..20]);
    }

    #[test]
    fn test_sanitize_avatar_rejects_html_injection() {
        // 禁止包含 < 或 >，防 SVG onload / HTML 注入
        assert!(sanitize_avatar("<svg onload=alert(1)>").is_err());
        assert!(sanitize_avatar("<img src=x onerror=alert(1)>").is_err());
        assert!(sanitize_avatar("user>name").is_err());
        assert!(sanitize_avatar("<").is_err());
    }

    #[test]
    fn test_sanitize_avatar_data_url_types_and_size() {
        // 支持合法 MIME 类型
        assert!(sanitize_avatar("data:image/png;base64,iVBORw0KGgoAAA").is_ok());
        assert!(sanitize_avatar("data:image/jpeg;base64,/9j/4AAQSkZJRg").is_ok());
        assert!(sanitize_avatar("data:image/jpg;base64,/9j/4AAQSkZJRg").is_ok());
        assert!(sanitize_avatar("data:image/gif;base64,R0lGODlh").is_ok());
        assert!(sanitize_avatar("data:image/webp;base64,UklGR").is_ok());

        // 拒绝非法类型，例如 svg+xml 或其它文件
        let svg_res = sanitize_avatar("data:image/svg+xml;base64,PHN2Zz4=");
        assert!(svg_res.is_err());

        // 超过 300KB（300_000 字节）拒绝
        let huge_png = format!("data:image/png;base64,{}", "A".repeat(300_001));
        assert!(sanitize_avatar(&huge_png).is_err());
    }

    #[tokio::test]
    async fn test_user_prefs_db_persistence() {
        let (_dir, db) = temp_db().await;

        // 插入测试用户
        let uid: i64 = sqlx::query_scalar(
            "INSERT INTO users (username, password_hash, salt, hash_algo, nickname) VALUES (?, ?, ?, ?, ?) RETURNING id"
        )
        .bind("alice")
        .bind("dummy_hash")
        .bind("dummy_salt")
        .bind("pbkdf2_sha256")
        .bind("Alice")
        .fetch_one(&db.pool)
        .await
        .unwrap();

        // 插入偏好记录
        let initial_prefs = UserPrefs {
            show_lunar: false,
            show_quote: true,
            show_weather: false,
            font_size: 14,
            weather_city: "Beijing".into(),
            theme_color: "#123456".into(),
            wallpaper: "https://example.com/bg.png".into(),
        };

        sqlx::query(
            "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(uid)
        .bind(initial_prefs.show_lunar)
        .bind(initial_prefs.show_quote)
        .bind(initial_prefs.show_weather)
        .bind(initial_prefs.font_size)
        .bind(&initial_prefs.weather_city)
        .bind(&initial_prefs.theme_color)
        .bind(&initial_prefs.wallpaper)
        .execute(&db.pool)
        .await
        .unwrap();

        // 校验读取
        let loaded = load_user(&db, uid).await.expect("user should load");
        assert_eq!(loaded.prefs.weather_city, "Beijing");
        assert_eq!(loaded.prefs.theme_color, "#123456");
        assert_eq!(loaded.prefs.font_size, 14);

        // 冲突更新（ON CONFLICT update）
        sqlx::query(
            "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(user_id) DO UPDATE SET
               weather_city = excluded.weather_city,
               font_size = excluded.font_size"
        )
        .bind(uid)
        .bind(true)
        .bind(true)
        .bind(true)
        .bind(18)
        .bind("Shanghai")
        .bind("#654321")
        .bind("")
        .execute(&db.pool)
        .await
        .unwrap();

        let updated = load_user(&db, uid).await.expect("user should load");
        assert_eq!(updated.prefs.weather_city, "Shanghai");
        assert_eq!(updated.prefs.font_size, 18);
    }
}

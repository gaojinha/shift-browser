//! 会话管理与账号认证（R2-1 拆分）

use crate::db::Db;
use crate::error::{AppError, CommandResult};

use super::security::*;
use super::*;

/// 当前登录用户 id（未登录返回 None）
pub(crate) async fn current_user_id(db: &Db) -> Option<i64> {
    let row: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(
        "SELECT user_id FROM user_session WHERE id = 1 AND logged_in = 1",
    )
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten();
    row.map(|r| r.0)
}

pub(crate) async fn load_prefs(db: &Db, user_id: i64) -> UserPrefs {
    let row: Option<(i64, i64, i64, i64, String, String, String)> = sqlx::query_as::<_, (i64, i64, i64, i64, String, String, String)>(
    "SELECT show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper
     FROM user_prefs WHERE user_id = ?",
  )
  .bind(user_id)
  .fetch_optional(&db.pool)
  .await
  .ok()
  .flatten();
    match row {
        Some((lunar, quote, weather, font, city, color, wall)) => UserPrefs {
            show_lunar: lunar != 0,
            show_quote: quote != 0,
            show_weather: weather != 0,
            font_size: font,
            weather_city: city,
            theme_color: color,
            wallpaper: wall,
        },
        None => UserPrefs::default(),
    }
}

pub(crate) async fn load_user(db: &Db, user_id: i64) -> Option<User> {
    let row: Option<(i64, String, String, String, String, String)> =
        sqlx::query_as::<_, (i64, String, String, String, String, String)>(
            "SELECT id, username, nickname, avatar, signature, created_at FROM users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten();
    let (id, username, nickname, avatar, signature, created_at) = row?;
    let prefs = load_prefs(db, user_id).await;
    Some(User {
        id,
        username,
        nickname,
        avatar,
        signature,
        created_at,
        prefs,
    })
}

/// 偏好写入 settings 表（登录时应用 / 更新时同步，newtab 等读取生效）
/// 返回 Result：写入失败如实上报，避免"前端显示成功、实际没生效"的假成功
pub(crate) async fn apply_prefs_to_settings(db: &Db, prefs: &UserPrefs) -> CommandResult<()> {
    let entries: Vec<(&str, String)> = vec![
        (
            "show_lunar",
            if prefs.show_lunar { "1" } else { "0" }.to_string(),
        ),
        (
            "show_quote",
            if prefs.show_quote { "1" } else { "0" }.to_string(),
        ),
        (
            "show_weather",
            if prefs.show_weather { "1" } else { "0" }.to_string(),
        ),
        ("font_size", prefs.font_size.to_string()),
        ("weather_city", prefs.weather_city.clone()),
        ("theme_color", prefs.theme_color.clone()),
        ("wallpaper", prefs.wallpaper.clone()),
    ];
    for (k, v) in entries {
        sqlx::query(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        )
        .bind(k)
        .bind(v)
        .execute(&db.pool)
        .await
        .map_err(|e| AppError::db(format!("写入设置 {} 失败: {}", k, e)))?;
    }
    Ok(())
}

/// 清除当前会话（启动锁屏验证身份后调用，确保个人中心未登录）
pub async fn clear_session(db: &Db) -> CommandResult<()> {
    sqlx::query(
        "INSERT INTO user_session (id, user_id, logged_in) VALUES (1, NULL, 0)
     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, logged_in = excluded.logged_in",
    )
    .execute(&db.pool)
    .await
    .map_err(|e| AppError::db(format!("清除会话失败: {}", e)))?;
    Ok(())
}

pub(crate) async fn set_session(db: &Db, user_id: Option<i64>) -> CommandResult<()> {
    let logged_in = if user_id.is_some() { 1 } else { 0 };
    sqlx::query(
        "INSERT INTO user_session (id, user_id, logged_in) VALUES (1, ?, ?)
     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, logged_in = excluded.logged_in",
    )
    .bind(user_id)
    .bind(logged_in)
    .execute(&db.pool)
    .await
    .map_err(|e| AppError::db(format!("写入会话失败: {}", e)))?;
    Ok(())
}

// ===== IPC 命令 =====

/// 注册账号（成功后自动登录）
#[tauri::command]
pub async fn user_register(
    db: tauri::State<'_, Db>,
    username: String,
    password: String,
) -> CommandResult<UserResult> {
    let name = username.trim().to_string();
    if name.is_empty() || password.is_empty() {
        return Ok(UserResult::err("用户名和密码不能为空"));
    }
    if !valid_username(&name) {
        return Ok(UserResult::err(
            "用户名长度需 2~20 个字符，只能包含中文、字母、数字、下划线、短横线",
        ));
    }
    if password.chars().count() < 6 {
        return Ok(UserResult::err("密码至少 6 位"));
    }
    let exists: Option<(i64,)> = db_try!(sqlx::query_as::<_, (i64,)>(
        "SELECT id FROM users WHERE username = ?"
    )
    .bind(&name)
    .fetch_optional(&db.pool));
    if exists.is_some() {
        return Ok(UserResult::err("用户名已存在，请换一个"));
    }
    let salt = generate_salt();
    let hash = hash_password(&password, &salt);
    let res = db_try!(sqlx::query(
        "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
       VALUES (?, ?, ?, ?, ?, '🦊', '这个人很懒，什么都没写')",
    )
    .bind(&name)
    .bind(&hash)
    .bind(&salt)
    .bind(HASH_ALGO_CURRENT)
    .bind(&name)
    .execute(&db.pool));
    let uid = res.last_insert_rowid();
    let prefs = UserPrefs::default();
    // 偏好行必须落库成功，否则后续登录读取不到偏好（假成功）
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
  .map_err(|e| AppError::db(format!("初始化偏好失败: {}", e)))?;
    if inserted.rows_affected() == 0 {
        return Ok(UserResult::err("初始化偏好失败，请重试"));
    }
    set_session(&db, Some(uid))
        .await
        .map_err(|e| AppError::db(format!("注册成功但登录会话写入失败: {}", e)))?;
    apply_prefs_to_settings(&db, &prefs)
        .await
        .map_err(|e| AppError::db(format!("注册成功但偏好应用失败: {}", e)))?;
    log::info!("[users] 注册新账号: {}", name);
    match load_user(&db, uid).await {
        Some(u) => Ok(UserResult::ok_user("注册成功", u)),
        None => Ok(UserResult::ok("注册成功")),
    }
}

/// 纯密码验证（供启动锁屏等非 IPC 场景使用，不写会话、不触发防爆破）
/// 按存储的 hash_algo 选迭代次数校验；成功后若是旧参数则透明升级到当前参数
pub async fn verify_password(db: &Db, username: &str, password: &str) -> CommandResult<bool> {
    let name = username.trim().to_string();
    if name.is_empty() || password.is_empty() {
        return Ok(false);
    }
    let row: Option<(String, String, String)> = sqlx::query_as::<_, (String, String, String)>(
        "SELECT password_hash, salt, hash_algo FROM users WHERE username = ?",
    )
    .bind(&name)
    .fetch_optional(&db.pool)
    .await
    .map_err(|e| AppError::db(e.to_string()))?;
    let Some((stored_hash, salt, algo)) = row else {
        return Ok(false);
    };
    let iter = iterations_for_algo(&algo);
    if hash_password_with_iter(password, &salt, iter) != stored_hash {
        return Ok(false);
    }
    // 透明升级：旧参数（如 'pbkdf2' 10 万次）→ 当前参数（60 万次）
    if algo != HASH_ALGO_CURRENT {
        let new_hash = hash_password(password, &salt);
        let _ = sqlx::query("UPDATE users SET password_hash = ?, hash_algo = ? WHERE username = ?")
            .bind(&new_hash)
            .bind(HASH_ALGO_CURRENT)
            .bind(&name)
            .execute(&db.pool)
            .await
            .map(|_| {
                log::info!("[users] 密码哈希已升级: {} → {}", algo, HASH_ALGO_CURRENT);
            });
    }
    Ok(true)
}

/// 登录
#[tauri::command]
pub async fn user_login(
    db: tauri::State<'_, Db>,
    username: String,
    password: String,
) -> CommandResult<UserResult> {
    let name = username.trim().to_string();
    if name.is_empty() || password.is_empty() {
        return Ok(UserResult::err("用户名和密码不能为空"));
    }
    let (_, locked_until) = guard_get(&name);
    if locked_until > now_ms() {
        let remain = (locked_until - now_ms()) / 1000 + 1;
        return Ok(UserResult::err(format!(
            "尝试次数过多，请 {} 秒后再试",
            remain
        )));
    }
    let row: Option<(i64, String, String, String)> =
        db_try!(sqlx::query_as::<_, (i64, String, String, String)>(
            "SELECT id, password_hash, salt, hash_algo FROM users WHERE username = ?",
        )
        .bind(&name)
        .fetch_optional(&db.pool));
    let Some((uid, stored_hash, salt, algo)) = row else {
        return Ok(UserResult::err("账号不存在，请先注册"));
    };
    if hash_password_with_iter(&password, &salt, iterations_for_algo(&algo)) != stored_hash {
        guard_record_failure(&name);
        let (f, _) = guard_get(&name);
        if f == 0 {
            return Ok(UserResult::err("密码错误次数过多，已锁定 30 秒"));
        }
        return Ok(UserResult::err(format!(
            "密码错误（还剩 {} 次机会）",
            MAX_LOGIN_FAILURES - f
        )));
    }
    // 透明升级旧迭代参数
    if algo != HASH_ALGO_CURRENT {
        let new_hash = hash_password(&password, &salt);
        let _ = sqlx::query("UPDATE users SET password_hash = ?, hash_algo = ? WHERE id = ?")
            .bind(&new_hash)
            .bind(HASH_ALGO_CURRENT)
            .bind(uid)
            .execute(&db.pool)
            .await
            .map(|_| log::info!("[users] 密码哈希已升级: {} → {}", algo, HASH_ALGO_CURRENT));
    }
    guard_clear(&name);
    set_session(&db, Some(uid))
        .await
        .map_err(|e| AppError::db(format!("登录成功但会话写入失败: {}", e)))?;
    let prefs = load_prefs(&db, uid).await;
    apply_prefs_to_settings(&db, &prefs)
        .await
        .map_err(|e| AppError::db(format!("登录成功但偏好应用失败: {}", e)))?;
    log::info!("[users] 用户登录: {}", name);
    match load_user(&db, uid).await {
        Some(u) => Ok(UserResult::ok_user("登录成功", u)),
        None => Ok(UserResult::err("登录失败，请重试")),
    }
}

/// 退出登录（偏好恢复默认）
#[tauri::command]
pub async fn user_logout(db: tauri::State<'_, Db>) -> CommandResult<UserResult> {
    set_session(&db, None)
        .await
        .map_err(|e| AppError::db(format!("退出登录失败: {}", e)))?;
    apply_prefs_to_settings(&db, &UserPrefs::default())
        .await
        .map_err(|e| AppError::db(format!("退出登录但偏好恢复失败: {}", e)))?;
    log::info!("[users] 用户退出登录");
    Ok(UserResult::ok("已退出登录"))
}

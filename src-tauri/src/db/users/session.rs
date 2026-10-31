// LINES-EXEMPT: 会话状态机（登录/续期/注销）的整体性要求，分散后难以核对状态迁移完备
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
    let hash = hash_password_blocking(password.clone(), salt.clone(), PBKDF2_ITER).await;
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
    if hash_password_blocking(password.to_string(), salt.clone(), iter).await != stored_hash {
        return Ok(false);
    }
    // 透明升级：旧参数（如 'pbkdf2' 10 万次）→ 当前参数（60 万次）
    if algo != HASH_ALGO_CURRENT {
        let new_hash = hash_password_blocking(password.to_string(), salt, PBKDF2_ITER).await;
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
    if hash_password_blocking(password.clone(), salt.clone(), iterations_for_algo(&algo)).await
        != stored_hash
    {
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
        let new_hash = hash_password_blocking(password.clone(), salt, PBKDF2_ITER).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    /// 创建带 users、user_session、user_prefs、settings 表的独立测试库
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let options = SqliteConnectOptions::new()
            .filename(dir.path().join("test_session.db"))
            .create_if_missing(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .unwrap();
        // 执行 users 迁移建表（包含 users, user_session, user_prefs 等）
        sqlx::raw_sql(include_str!("../../../migrations/20260914000000_users.sql"))
            .execute(&pool)
            .await
            .unwrap();
        // 补充 settings 表结构以支撑偏好同步
        sqlx::raw_sql(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )
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

    /// 辅助插入用户
    async fn insert_test_user(db: &Db, name: &str, pwd: &str, algo: &str) -> i64 {
        let salt = generate_salt();
        let iter = iterations_for_algo(algo);
        let hash = hash_password_with_iter(pwd, &salt, iter);
        let res = sqlx::query(
            "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
             VALUES (?, ?, ?, ?, ?, '🦊', 'bio')",
        )
        .bind(name)
        .bind(&hash)
        .bind(&salt)
        .bind(algo)
        .bind(name)
        .execute(&db.pool)
        .await
        .unwrap();
        res.last_insert_rowid()
    }

    /// 校验会话状态机：初始未登录、set_session 写入与切换、clear_session 覆盖与注销
    #[tokio::test]
    async fn test_session_state_machine() {
        let (_dir, db) = temp_db().await;

        // 插入有效用户以满足 user_session.user_id 外键约束
        let u1 = insert_test_user(&db, "user_one", "pass123", HASH_ALGO_CURRENT).await;
        let u2 = insert_test_user(&db, "user_two", "pass123", HASH_ALGO_CURRENT).await;

        // 1. 初始库中 user_session 无记录，current_user_id 为 None
        assert_eq!(current_user_id(&db).await, None);

        // 2. 写入用户 u1 登录态
        set_session(&db, Some(u1)).await.unwrap();
        assert_eq!(current_user_id(&db).await, Some(u1));

        // 3. 覆盖写入用户 u2
        set_session(&db, Some(u2)).await.unwrap();
        assert_eq!(current_user_id(&db).await, Some(u2));

        // 4. set_session(None) 标记 logged_in = 0
        set_session(&db, None).await.unwrap();
        assert_eq!(current_user_id(&db).await, None);

        // 5. 再次登录后调用 clear_session，必须重置为 NULL / 0
        set_session(&db, Some(u1)).await.unwrap();
        assert_eq!(current_user_id(&db).await, Some(u1));
        clear_session(&db).await.unwrap();
        assert_eq!(current_user_id(&db).await, None);
    }

    /// 校验偏好读写与 settings 表同步
    #[tokio::test]
    async fn test_load_prefs_and_apply_to_settings() {
        let (_dir, db) = temp_db().await;

        // 插入有效用户作为偏好绑定的外键宿主
        let uid = insert_test_user(&db, "pref_user", "pass123", HASH_ALGO_CURRENT).await;

        // 1. 未配置偏好的用户回退为 UserPrefs::default()
        let default_prefs = load_prefs(&db, 999).await;
        assert_eq!(
            default_prefs.weather_city,
            UserPrefs::default().weather_city
        );

        // 2. 插入自定义偏好行
        sqlx::query(
            "INSERT INTO user_prefs (user_id, show_lunar, show_quote, show_weather, font_size, weather_city, theme_color, wallpaper)
             VALUES (?, 0, 1, 0, 18, 'Shanghai', '#123456', 'custom.png')",
        )
        .bind(uid)
        .execute(&db.pool)
        .await
        .unwrap();

        let loaded = load_prefs(&db, uid).await;
        assert!(!loaded.show_lunar);
        assert!(loaded.show_quote);
        assert!(!loaded.show_weather);
        assert_eq!(loaded.font_size, 18);
        assert_eq!(loaded.weather_city, "Shanghai");
        assert_eq!(loaded.theme_color, "#123456");
        assert_eq!(loaded.wallpaper, "custom.png");

        // 3. 应用偏好到 settings 表
        apply_prefs_to_settings(&db, &loaded).await.unwrap();

        let val_city: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'weather_city'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        let val_quote: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'show_quote'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        let val_lunar: String =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'show_lunar'")
                .fetch_one(&db.pool)
                .await
                .unwrap();

        assert_eq!(val_city, "Shanghai");
        assert_eq!(val_quote, "1");
        assert_eq!(val_lunar, "0");
    }

    /// 校验 verify_password 纯密码验证：匹配、错误密码、空用户名/密码、以及历史算法透明升级
    #[tokio::test]
    async fn test_verify_password_and_transparent_upgrade() {
        let (_dir, db) = temp_db().await;

        // 空入参拦截
        assert!(!verify_password(&db, "", "pwd123").await.unwrap());
        assert!(!verify_password(&db, "user", "").await.unwrap());
        assert!(!verify_password(&db, "nonexistent", "pwd123").await.unwrap());

        // 插入当前算法用户验证
        let uid = insert_test_user(&db, "alice", "Secret123", HASH_ALGO_CURRENT).await;
        assert!(uid > 0);
        assert!(verify_password(&db, "alice", "Secret123").await.unwrap());
        assert!(!verify_password(&db, "alice", "WrongPwd").await.unwrap());

        // 插入历史算法（10 万次）用户验证
        insert_test_user(&db, "bob", "OldPass456", "pbkdf2").await;
        let algo_before: String =
            sqlx::query_scalar("SELECT hash_algo FROM users WHERE username = 'bob'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(algo_before, "pbkdf2");

        // 历史算法密码校验成功后，触发透明升级落库
        let ok = verify_password(&db, "bob", "OldPass456").await.unwrap();
        assert!(ok);

        let algo_after: String =
            sqlx::query_scalar("SELECT hash_algo FROM users WHERE username = 'bob'")
                .fetch_one(&db.pool)
                .await
                .unwrap();
        assert_eq!(algo_after, HASH_ALGO_CURRENT);

        // 升级后再次校验依然通过
        assert!(verify_password(&db, "bob", "OldPass456").await.unwrap());
    }

    /// 校验 load_user 加载基础信息与关联偏好
    #[tokio::test]
    async fn test_load_user_with_prefs() {
        let (_dir, db) = temp_db().await;
        assert!(load_user(&db, 999).await.is_none());

        let uid = insert_test_user(&db, "carol", "Pass789", HASH_ALGO_CURRENT).await;
        let user = load_user(&db, uid).await.expect("user should exist");
        assert_eq!(user.id, uid);
        assert_eq!(user.username, "carol");
        assert_eq!(user.nickname, "carol");
        assert_eq!(user.avatar, "🦊");
    }
}

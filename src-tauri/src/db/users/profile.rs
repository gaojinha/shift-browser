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
    if hash_password_with_iter(&old_password, &salt, iterations_for_algo(&algo)) != stored_hash {
        return Ok(UserResult::err("旧密码错误"));
    }
    let new_salt = generate_salt();
    let new_hash = hash_password(&new_password, &new_salt);
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
    if hash_password_with_iter(&password, &salt, iterations_for_algo(&algo)) != stored_hash {
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

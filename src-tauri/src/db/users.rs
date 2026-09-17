// 用户账号系统（个人中心）：注册 / 登录 / 资料 / 偏好 / 备份
// 密码哈希与原版 Electron 一致：PBKDF2（10 万次迭代，SHA-512）
// 偏好跟账号走：登录时写入 settings 表（newtab 等读取生效），退出恢复默认
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

// ===== 数据结构 =====

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserPrefs {
  pub show_lunar: bool,
  pub show_quote: bool,
  pub show_weather: bool,
  pub font_size: i64,
  pub weather_city: String,
  pub theme_color: String,
  pub wallpaper: String,
}

impl Default for UserPrefs {
  fn default() -> Self {
    Self {
      show_lunar: true,
      show_quote: true,
      show_weather: true,
      font_size: 14,
      weather_city: "运城".into(),
      theme_color: "#4a9eff".into(),
      wallpaper: "".into(),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct User {
  pub id: i64,
  pub username: String,
  pub nickname: String,
  pub avatar: String,
  pub signature: String,
  pub created_at: String,
  pub prefs: UserPrefs,
}

/// 统一返回：{ ok, msg, user?, path? }
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserResult {
  pub ok: bool,
  pub msg: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub user: Option<User>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub path: Option<String>,
}

impl UserResult {
  fn ok(msg: impl Into<String>) -> Self {
    Self { ok: true, msg: msg.into(), user: None, path: None }
  }
  fn ok_user(msg: impl Into<String>, user: User) -> Self {
    Self { ok: true, msg: msg.into(), user: Some(user), path: None }
  }
  fn err(msg: impl Into<String>) -> Self {
    Self { ok: false, msg: msg.into(), user: None, path: None }
  }
}

/// sqlx 错误统一转 UserResult::err（UserResult 不是 Result，不能用 ?）
macro_rules! db_try {
  ($e:expr) => {
    match $e.await {
      Ok(v) => v,
      Err(e) => return Ok(UserResult::err(format!("数据库错误: {}", e))),
    }
  };
}

// ===== 密码哈希（PBKDF2，10 万次迭代 SHA-512）=====

fn hash_password(password: &str, salt: &str) -> String {
  use pbkdf2::pbkdf2_hmac;
  use sha2::Sha512;
  let mut out = [0u8; 64];
  pbkdf2_hmac::<Sha512>(password.as_bytes(), salt.as_bytes(), 100_000, &mut out);
  hex::encode(out)
}

fn generate_salt() -> String {
  use rand::RngCore;
  let mut salt = [0u8; 16];
  rand::thread_rng().fill_bytes(&mut salt);
  hex::encode(salt)
}

fn valid_username(name: &str) -> bool {
  let len = name.chars().count();
  (2..=20).contains(&len)
    && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

// ===== 登录防爆破（内存态：连续失败锁定 30 秒）=====

static LOGIN_GUARD: Mutex<Option<HashMap<String, (u32, u64)>>> = Mutex::new(None);

const MAX_LOGIN_FAILURES: u32 = 5;
const LOCK_MS: u64 = 30_000;

fn now_ms() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map(|d| d.as_millis() as u64)
    .unwrap_or(0)
}

fn guard_get(name: &str) -> (u32, u64) {
  let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
  let map = g.get_or_insert_with(HashMap::new);
  map.get(name).copied().unwrap_or((0, 0))
}

fn guard_record_failure(name: &str) {
  let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
  let map = g.get_or_insert_with(HashMap::new);
  let (f, _) = map.get(name).copied().unwrap_or((0, 0));
  let f = f + 1;
  if f >= MAX_LOGIN_FAILURES {
    map.insert(name.to_string(), (0, now_ms() + LOCK_MS));
  } else {
    map.insert(name.to_string(), (f, 0));
  }
}

fn guard_clear(name: &str) {
  let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
  if let Some(map) = g.as_mut() {
    map.remove(name);
  }
}

// ===== 数据库辅助 =====

/// 当前登录用户 id（未登录返回 None）
async fn current_user_id(db: &Db) -> Option<i64> {
  let row: Option<(i64,)> = sqlx::query_as::<_, (i64,)>(
    "SELECT user_id FROM user_session WHERE id = 1 AND logged_in = 1",
  )
  .fetch_optional(&db.pool)
  .await
  .ok()
  .flatten();
  row.map(|r| r.0)
}

async fn load_prefs(db: &Db, user_id: i64) -> UserPrefs {
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

async fn load_user(db: &Db, user_id: i64) -> Option<User> {
  let row: Option<(i64, String, String, String, String, String)> = sqlx::query_as::<_, (i64, String, String, String, String, String)>(
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
async fn apply_prefs_to_settings(db: &Db, prefs: &UserPrefs) {
  let entries: Vec<(&str, String)> = vec![
    ("show_lunar", if prefs.show_lunar { "1" } else { "0" }.to_string()),
    ("show_quote", if prefs.show_quote { "1" } else { "0" }.to_string()),
    ("show_weather", if prefs.show_weather { "1" } else { "0" }.to_string()),
    ("font_size", prefs.font_size.to_string()),
    ("weather_city", prefs.weather_city.clone()),
    ("theme_color", prefs.theme_color.clone()),
    ("wallpaper", prefs.wallpaper.clone()),
  ];
  for (k, v) in entries {
    let _ = sqlx::query(
      "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, datetime('now'))
       ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
    )
    .bind(k)
    .bind(v)
    .execute(&db.pool)
    .await;
  }
}

/// 清除当前会话（启动锁屏验证身份后调用，确保个人中心未登录）
pub async fn clear_session(db: &Db) {
  let _ = sqlx::query(
    "INSERT INTO user_session (id, user_id, logged_in) VALUES (1, NULL, 0)
     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, logged_in = excluded.logged_in",
  )
  .execute(&db.pool)
  .await;
}

async fn set_session(db: &Db, user_id: Option<i64>) {
  let logged_in = if user_id.is_some() { 1 } else { 0 };
  let _ = sqlx::query(
    "INSERT INTO user_session (id, user_id, logged_in) VALUES (1, ?, ?)
     ON CONFLICT(id) DO UPDATE SET user_id = excluded.user_id, logged_in = excluded.logged_in",
  )
  .bind(user_id)
  .bind(logged_in)
  .execute(&db.pool)
  .await;
}

// ===== IPC 命令 =====

/// 注册账号（成功后自动登录）
#[tauri::command]
pub async fn user_register(
  db: tauri::State<'_, Db>,
  username: String,
  password: String,
) -> Result<UserResult, String> {
  let name = username.trim().to_string();
  if name.is_empty() || password.is_empty() {
    return Ok(UserResult::err("用户名和密码不能为空"));
  }
  if !valid_username(&name) {
    return Ok(UserResult::err("用户名长度需 2~20 个字符，只能包含中文、字母、数字、下划线、短横线"));
  }
  if password.chars().count() < 6 {
    return Ok(UserResult::err("密码至少 6 位"));
  }
  let exists: Option<(i64,)> = db_try!(
    sqlx::query_as::<_, (i64,)>("SELECT id FROM users WHERE username = ?")
      .bind(&name)
      .fetch_optional(&db.pool)
  );
  if exists.is_some() {
    return Ok(UserResult::err("用户名已存在，请换一个"));
  }
  let salt = generate_salt();
  let hash = hash_password(&password, &salt);
  let res = db_try!(
    sqlx::query(
      "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
       VALUES (?, ?, ?, 'pbkdf2', ?, '🦊', '这个人很懒，什么都没写')",
    )
    .bind(&name)
    .bind(&hash)
    .bind(&salt)
    .bind(&name)
    .execute(&db.pool)
  );
  let uid = res.last_insert_rowid();
  let prefs = UserPrefs::default();
  let _ = sqlx::query(
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
  .await;
  set_session(&db, Some(uid)).await;
  apply_prefs_to_settings(&db, &prefs).await;
  log::info!("[users] 注册新账号: {}", name);
  match load_user(&db, uid).await {
    Some(u) => Ok(UserResult::ok_user("注册成功", u)),
    None => Ok(UserResult::ok("注册成功")),
  }
}

/// 纯密码验证（供启动锁屏等非 IPC 场景使用，不写会话、不触发防爆破）
pub async fn verify_password(db: &Db, username: &str, password: &str) -> Result<bool, String> {
  let name = username.trim().to_string();
  if name.is_empty() || password.is_empty() {
    return Ok(false);
  }
  let row: Option<(String, String)> = sqlx::query_as::<_, (String, String)>(
    "SELECT password_hash, salt FROM users WHERE username = ?",
  )
  .bind(&name)
  .fetch_optional(&db.pool)
  .await
  .map_err(|e| e.to_string())?;
  let Some((stored_hash, salt)) = row else {
    return Ok(false);
  };
  Ok(hash_password(password, &salt) == stored_hash)
}

/// 登录
#[tauri::command]
pub async fn user_login(
  db: tauri::State<'_, Db>,
  username: String,
  password: String,
) -> Result<UserResult, String> {
  let name = username.trim().to_string();
  if name.is_empty() || password.is_empty() {
    return Ok(UserResult::err("用户名和密码不能为空"));
  }
  let (_, locked_until) = guard_get(&name);
  if locked_until > now_ms() {
    let remain = (locked_until - now_ms()) / 1000 + 1;
    return Ok(UserResult::err(format!("尝试次数过多，请 {} 秒后再试", remain)));
  }
  let row: Option<(i64, String, String)> = db_try!(
    sqlx::query_as::<_, (i64, String, String)>(
      "SELECT id, password_hash, salt FROM users WHERE username = ?",
    )
    .bind(&name)
    .fetch_optional(&db.pool)
  );
  let Some((uid, stored_hash, salt)) = row else {
    return Ok(UserResult::err("账号不存在，请先注册"));
  };
  if hash_password(&password, &salt) != stored_hash {
    guard_record_failure(&name);
    let (f, _) = guard_get(&name);
    if f == 0 {
      return Ok(UserResult::err("密码错误次数过多，已锁定 30 秒"));
    }
    return Ok(UserResult::err(format!("密码错误（还剩 {} 次机会）", MAX_LOGIN_FAILURES - f)));
  }
  guard_clear(&name);
  set_session(&db, Some(uid)).await;
  let prefs = load_prefs(&db, uid).await;
  apply_prefs_to_settings(&db, &prefs).await;
  log::info!("[users] 用户登录: {}", name);
  match load_user(&db, uid).await {
    Some(u) => Ok(UserResult::ok_user("登录成功", u)),
    None => Ok(UserResult::err("登录失败，请重试")),
  }
}

/// 退出登录（偏好恢复默认）
#[tauri::command]
pub async fn user_logout(db: tauri::State<'_, Db>) -> Result<UserResult, String> {
  set_session(&db, None).await;
  apply_prefs_to_settings(&db, &UserPrefs::default()).await;
  log::info!("[users] 用户退出登录");
  Ok(UserResult::ok("已退出登录"))
}

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
    let a = a.trim().to_string();
    // 图片头像（base64 data URL）限制 300KB；emoji/文本头像限 20 字符
    let a = if a.starts_with("data:image/") {
      if a.len() > 300_000 {
        return Ok(UserResult::err("头像图片过大（限 300KB）"));
      }
      a
    } else {
      a.chars().take(20).collect()
    };
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
  let row: Option<(String, String)> = db_try!(
    sqlx::query_as::<_, (String, String)>(
      "SELECT password_hash, salt FROM users WHERE id = ?",
    )
    .bind(uid)
    .fetch_optional(&db.pool)
  );
  let Some((stored_hash, salt)) = row else {
    return Ok(UserResult::err("账号不存在"));
  };
  if hash_password(&old_password, &salt) != stored_hash {
    return Ok(UserResult::err("旧密码错误"));
  }
  let new_salt = generate_salt();
  let new_hash = hash_password(&new_password, &new_salt);
  db_try!(
    sqlx::query("UPDATE users SET password_hash = ?, salt = ?, hash_algo = 'pbkdf2' WHERE id = ?")
      .bind(&new_hash)
      .bind(&new_salt)
      .bind(uid)
      .execute(&db.pool)
  );
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
  let row: Option<(i64, String, String)> = db_try!(
    sqlx::query_as::<_, (i64, String, String)>(
      "SELECT id, password_hash, salt FROM users WHERE username = ?",
    )
    .bind(&name)
    .fetch_optional(&db.pool)
  );
  let Some((uid, stored_hash, salt)) = row else {
    return Ok(UserResult::err("账号不存在"));
  };
  if hash_password(&password, &salt) != stored_hash {
    return Ok(UserResult::err("密码错误，无法删除"));
  }
  let was_current = current_user_id(&db).await == Some(uid);
  db_try!(sqlx::query("DELETE FROM users WHERE id = ?").bind(uid).execute(&db.pool));
  if was_current {
    set_session(&db, None).await;
    apply_prefs_to_settings(&db, &UserPrefs::default()).await;
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
  .await;
  apply_prefs_to_settings(&db, &prefs).await;
  log::info!("[users] 账号偏好已更新");
  match load_user(&db, uid).await {
    Some(u) => Ok(UserResult::ok_user("偏好已保存", u)),
    None => Ok(UserResult::err("保存失败")),
  }
}

/// 导出账号备份（JSON 文件，含密码哈希，提示妥善保管）
#[tauri::command]
pub async fn user_export(
  db: tauri::State<'_, Db>,
  username: Option<String>,
) -> Result<UserResult, String> {
  let name = username.unwrap_or_default().trim().to_string();
  let uid = if name.is_empty() {
    current_user_id(&db).await
  } else {
    let row: Option<(i64,)> = sqlx::query_as::<_, (i64,)>("SELECT id FROM users WHERE username = ?")
      .bind(&name)
      .fetch_optional(&db.pool)
      .await
      .ok()
      .flatten();
    row.map(|r| r.0)
  };
  let Some(uid) = uid else {
    return Ok(UserResult::err("账号不存在"));
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
    .set_file_name(format!("shiftbrowser-账号备份-{}-{}.json", uname, today_str()))
    .add_filter("JSON", &["json"])
    .save_file()
    .await
  {
    Some(f) => f,
    None => return Ok(UserResult::err("已取消导出")),
  };
  let path = file.path().to_path_buf();
  std::fs::write(&path, serde_json::to_string_pretty(&payload).unwrap_or_default())
    .map_err(|e| format!("写入文件失败: {}", e))?;
  log::info!("[users] 账号备份已导出: {}", path.display());
  Ok(UserResult {
    ok: true,
    msg: "备份已导出".into(),
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
  let acct = v.get("account").cloned().ok_or_else(|| "备份文件缺少账号数据".to_string())?;
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
  let nickname = acct.get("nickname").and_then(|u| u.as_str()).unwrap_or(&uname).to_string();
  let avatar = acct.get("avatar").and_then(|u| u.as_str()).unwrap_or("🦊").to_string();
  let signature = acct
    .get("signature")
    .and_then(|u| u.as_str())
    .unwrap_or("这个人很懒，什么都没写")
    .to_string();
  let exists: Option<(i64,)> = db_try!(
    sqlx::query_as::<_, (i64,)>("SELECT id FROM users WHERE username = ?")
      .bind(&uname)
      .fetch_optional(&db.pool)
  );
  if exists.is_some() {
    return Ok(UserResult::err(format!("账号 {} 已存在，请先删除或换名导入", uname)));
  }
  db_try!(
    sqlx::query(
      "INSERT INTO users (username, password_hash, salt, hash_algo, nickname, avatar, signature)
       VALUES (?, ?, ?, 'pbkdf2', ?, ?, ?)",
    )
    .bind(&uname)
    .bind(&pwd_hash)
    .bind(&salt)
    .bind(&nickname)
    .bind(&avatar)
    .bind(&signature)
    .execute(&db.pool)
  );
  let uid: (i64,) = db_try!(
    sqlx::query_as::<_, (i64,)>("SELECT id FROM users WHERE username = ?")
      .bind(&uname)
      .fetch_one(&db.pool)
  );
  let uid = uid.0;
  // 偏好（原版 camelCase 兼容）
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
  let _ = sqlx::query(
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
  .await;
  log::info!("[users] 账号备份已导入: {}", uname);
  Ok(UserResult::ok(format!("账号 {} 导入成功", uname)))
}

// ===== 小工具 =====

fn today_str() -> String {
  // 简单 YYYY-MM-DD（UTC 近似，够文件名用）
  let secs = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs();
  let days = secs / 86400;
  let (y, m, d) = civil_from_days(days as i64);
  format!("{:04}-{:02}-{:02}", y, m, d)
}

/// 天数 → 公历日期（Howard Hinnant 算法）
fn civil_from_days(z: i64) -> (i64, i64, i64) {
  let z = z + 719_468;
  let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
  let doe = (z - era * 146_097) as u64;
  let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
  let y = yoe as i64 + era * 400;
  let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
  let mp = (5 * doy + 2) / 153;
  let d = doy - (153 * mp + 2) / 5 + 1;
  let m = if mp < 10 { mp + 3 } else { mp - 9 };
  (if m <= 2 { y + 1 } else { y }, (m as i64), (d as i64))
}
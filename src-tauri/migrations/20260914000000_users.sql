-- 用户账号系统（个人中心迁移：账号 / 资料 / 偏好 / 会话）
-- 日期：2026-09-14

-- 用户账号
CREATE TABLE IF NOT EXISTS users (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  username TEXT NOT NULL UNIQUE,
  password_hash TEXT NOT NULL,
  salt TEXT NOT NULL,
  hash_algo TEXT NOT NULL DEFAULT 'pbkdf2',
  nickname TEXT NOT NULL,
  avatar TEXT NOT NULL DEFAULT '🦊',
  signature TEXT NOT NULL DEFAULT '这个人很懒，什么都没写',
  created_at TEXT DEFAULT (datetime('now'))
);

-- 用户偏好（跟账号走：登录加载、退出恢复默认）
CREATE TABLE IF NOT EXISTS user_prefs (
  user_id INTEGER PRIMARY KEY,
  show_lunar INTEGER DEFAULT 1,
  show_quote INTEGER DEFAULT 1,
  show_weather INTEGER DEFAULT 1,
  font_size INTEGER DEFAULT 14,
  weather_city TEXT DEFAULT '运城',
  theme_color TEXT DEFAULT '#4a9eff',
  wallpaper TEXT DEFAULT '',
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);

-- 登录会话（单例，id 恒为 1）
CREATE TABLE IF NOT EXISTS user_session (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  user_id INTEGER,
  logged_in INTEGER DEFAULT 0,
  FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
);
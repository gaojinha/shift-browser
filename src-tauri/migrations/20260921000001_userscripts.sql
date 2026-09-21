-- 阶段 3.2 用户脚本（Tampermonkey 风格）
-- 每个脚本：name 名称、matches 匹配模式（换行分隔，WebKit allow_list 格式）、
--           code JS 源码、enabled 开关。
CREATE TABLE IF NOT EXISTS userscripts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  matches TEXT NOT NULL DEFAULT '*://*/*',
  code TEXT NOT NULL DEFAULT '',
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

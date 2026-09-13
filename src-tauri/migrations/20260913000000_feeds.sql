-- Shift Browser 资讯订阅缓存 v2
-- 日期：2026-09-13

-- 资讯条目（内置源定时抓取缓存，每源保留最新 50 条）
CREATE TABLE IF NOT EXISTS feed_items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  source TEXT NOT NULL,          -- 源名称（如 IT之家）
  category TEXT NOT NULL,        -- 分类（如 科技互联网）
  title TEXT NOT NULL,
  url TEXT NOT NULL,
  published_at TEXT DEFAULT '',  -- 源里的原始时间文本（前端 new Date() 尽力解析）
  fetched_at TEXT DEFAULT (datetime('now')),
  UNIQUE(url)
);
CREATE INDEX IF NOT EXISTS idx_feed_items_fetched ON feed_items(fetched_at DESC);
CREATE INDEX IF NOT EXISTS idx_feed_items_source ON feed_items(source);

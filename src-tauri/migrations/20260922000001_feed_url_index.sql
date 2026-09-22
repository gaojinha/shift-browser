-- 性能 P1（2026-09-22）：feed_items 补 url 索引
-- 背景：latest_items 用 `WHERE id IN (SELECT MAX(id) FROM feed_items GROUP BY url)` 去重，
-- 原先只有 fetched_at/source 索引，GROUP BY url 会全表扫 + 临时表。
-- 注：表已有 UNIQUE(url) 约束（自带唯一索引），但显式命名索引便于查询计划利用。
CREATE INDEX IF NOT EXISTS idx_feed_items_url ON feed_items(url);

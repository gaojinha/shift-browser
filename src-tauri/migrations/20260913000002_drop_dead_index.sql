-- 删除死索引 idx_feed_items_fetched：
-- latest_items 按 id DESC + GROUP BY url（走隐含 UNIQUE(url) 索引），每源裁剪按 id，
-- fetched_at DESC 索引无任何查询使用，只在每次 INSERT 时增加写开销
DROP INDEX IF EXISTS idx_feed_items_fetched;
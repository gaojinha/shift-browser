-- v9 索引整理（P2）：
--
-- 1) 删除冗余 idx_history_url：迁移 v3 已建唯一索引 idx_history_url_uniq，
--    同一列重复维护两个索引，非唯一那个对任何查询无用，却让每次 UPSERT
--    双倍索引写开销（与 v4 删死索引 idx_feed_items_fetched 同思路）。
-- 2) 补 idx_history_visit_count：get_most_visited 按
--    `ORDER BY visit_count DESC, last_visit_at DESC` 全表排序，记录接近上限
--    （1 万条）时每次新标签页渲染都有可感开销。
-- 3) 补 idx_sessions_workspace：load_last_session 每次启动/切工作区按
--    workspace_id + ORDER BY id DESC 全表扫描。

DROP INDEX IF EXISTS idx_history_url;

CREATE INDEX IF NOT EXISTS idx_history_visit_count
  ON history(visit_count DESC, last_visit_at DESC);

CREATE INDEX IF NOT EXISTS idx_sessions_workspace
  ON sessions(workspace_id, id DESC);
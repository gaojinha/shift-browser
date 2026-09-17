-- 浏览历史：last_visit_at 索引
--
-- 背景：history 表只有 url 唯一索引；get_recent_history 的
-- `ORDER BY last_visit_at DESC` 与旧记录清理子查询都要全表排序，
-- 记录数接近上限（1 万条）时每次导航都产生明显开销。
-- 加索引后排序/取最近 N 条走索引扫描。
CREATE INDEX IF NOT EXISTS idx_history_last_visit ON history(last_visit_at DESC);

-- 历史表 URL 唯一化：
-- 1) 合并既有重复行：同一 url 只保留 id 最大的一行，visit_count 累加（保留最新 last_visit_at）
UPDATE history SET visit_count = (
  SELECT SUM(h2.visit_count) FROM history h2 WHERE h2.url = history.url
) WHERE id IN (SELECT MAX(id) FROM history GROUP BY url);

DELETE FROM history WHERE id NOT IN (
  SELECT MAX(id) FROM history GROUP BY url
);

-- 2) 唯一索引：防并发"先查后插"产生重复行（record_history 已改为 UPSERT）
CREATE UNIQUE INDEX IF NOT EXISTS idx_history_url_uniq ON history(url);
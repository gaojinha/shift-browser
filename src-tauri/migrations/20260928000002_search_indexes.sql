-- 数据层优化 · 搜索排序索引（2026-09-28）
--
-- idx_history_visit_count：最常访问按 visit_count 全排序，无索引即全表扫。
-- idx_bookmarks_title：书签搜索按 title 排序，无索引即全表扫。
-- 说明：前后通配 LIKE 保持原语义（地址栏联想靠它），索引覆盖排序路径；
-- 历史 10000 上限封住扫描规模。
CREATE INDEX IF NOT EXISTS idx_history_visit_count ON history(visit_count DESC);
CREATE INDEX IF NOT EXISTS idx_bookmarks_title ON bookmarks(title);

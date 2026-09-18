-- save_password 竞态修复：password 表加 (domain, username) 唯一索引。
--
-- 背景：save_password 原来是"先 SELECT 判重 → 再 INSERT/UPDATE"，两步非事务。
-- 并发保存同一 domain+username 时（比如前端快速连点保存、或两个窗口同时存），
-- 两个事务都先读到"不存在"，然后各自 INSERT → 产生重复行。之后查询按更新时间取最新，
-- 但重复行会让"按 username 取密码"这类逻辑产生二义性。
--
-- 本迁移：
--  1. 清理历史重复行（保留每个 (domain, username) 里 id 最小的一条，即最早插入的）
--  2. 建唯一索引，让 save_password 可以用 UPSERT（ON CONFLICT DO UPDATE）原子化
--
-- 唯一索引只对非空 domain 生效；password 表不允许 NULL domain（NOT NULL 约束），
-- 所以这里对 (domain, username) 整体唯一，username 可为空（NULL 不参与唯一性，
-- SQLite 里 NULL != NULL，两条 NULL username 不会互相冲突——这正是我们想要的：
-- 允许 NULL username 的重复，但显式同名必须唯一）。

-- 1. 清理重复：保留每组最早一条
DELETE FROM passwords WHERE id NOT IN (
  SELECT MIN(id) FROM passwords GROUP BY domain, username
);

-- 2. 唯一索引
CREATE UNIQUE INDEX IF NOT EXISTS idx_passwords_domain_username
  ON passwords(domain, username);

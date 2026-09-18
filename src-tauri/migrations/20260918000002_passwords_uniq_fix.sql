-- v8 修复：v7（20260918000001_passwords_uniq.sql）在已应用的生产库上
-- 曾用 `GROUP BY domain, username` 清理重复，SQLite 会把 NULL username 归为一组，
-- 导致同域名下所有 NULL username 密码（注释明确允许的重复）被误删。
--
-- 本迁移说明：
--  1. 已应用 v7 的库：NULL 重复行已被删除（不可恢复，需从备份还原）；
--     唯一索引允许 NULL 重复，后续再存 NULL username 密码不受影响。
--  2. 未应用 v7 的新库：v7 已改为 `WHERE username IS NOT NULL`，不会再误删。
--  3. 本迁移只保证索引存在（幂等），并作为修复记录进入 _migrations。

CREATE UNIQUE INDEX IF NOT EXISTS idx_passwords_domain_username
  ON passwords(domain, username);
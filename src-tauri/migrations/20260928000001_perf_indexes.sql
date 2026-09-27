-- 第二期止血 · 索引补齐（2026-09-28）
--
-- downloads(state)：启动扫尾 UPDATE ... WHERE state='in_progress' 与每次插入的
--   清理子查询都要按状态过滤，零索引即全表扫。
-- sessions(workspace_id)：切换工作区 load_last_session 按工作区查最近一条。
-- sessions(name, id)：自动保存清理按 name 过滤按 id 排序。
CREATE INDEX IF NOT EXISTS idx_downloads_state ON downloads(state);
CREATE INDEX IF NOT EXISTS idx_sessions_workspace ON sessions(workspace_id);
CREATE INDEX IF NOT EXISTS idx_sessions_name_id ON sessions(name, id DESC);

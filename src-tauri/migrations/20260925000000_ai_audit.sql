-- AI 读取审计（K 类上下文与审计 · O3 审计持久化 · 2026-09-25）
--
-- 目的：记录开放 API 的 AI 操作（读页面 / 开标签 / 交互 / 清理），使"AI 看了什么"
--       可追溯；写入 db 表，重启不丢（安全底线，不可关闭）。
-- 隐私（N5）：只记 命令 + url + 结果码，**绝不记页面内容 / 表单值**。

CREATE TABLE IF NOT EXISTS ai_audit (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  ts          TEXT    NOT NULL DEFAULT (datetime('now')),
  cmd         TEXT    NOT NULL,                      -- 命令名（open_and_read / read_page / …）
  url         TEXT    NOT NULL DEFAULT '',           -- 目标 URL（不含内容）
  tab_id      TEXT    NOT NULL DEFAULT '',           -- 关联标签 id（可空）
  ok          INTEGER NOT NULL DEFAULT 1,            -- 1 成功 / 0 失败
  error_code  TEXT    NOT NULL DEFAULT '',           -- 失败分类（AppError::code()）
  origin      TEXT    NOT NULL DEFAULT 'ai'          -- 来源（预留，当前恒 ai）
);

-- 按时间倒序查询（最近 N 条），id 作同秒并列的决胜键
CREATE INDEX IF NOT EXISTS idx_ai_audit_id_desc ON ai_audit(id DESC);

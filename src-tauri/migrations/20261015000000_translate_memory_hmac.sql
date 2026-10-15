-- 2026-10-15: C-11 翻译记忆原文不落明文——source 明文列换 HMAC 查询键
--
-- 依据 C-10 评估结论（docs/C-10-translate_memory加密评估_2026-10-15.md）：
-- translate_memory 是纯缓存（无任何 UI 读取原文），旧数据清表零价值损失；
-- source 列换成 source_hash = HMAC-SHA256(translate.key, 归一化原文) 的 hex，
-- 等值查询/裁剪/主键语义不变，但库里不再有可读的用户浏览原文。
-- 密钥在独立文件 translate.key（权限 600，与 DB 分离，与密码库解锁状态解耦）。

DROP TABLE IF EXISTS translate_memory;

CREATE TABLE IF NOT EXISTS translate_memory (
  model        TEXT    NOT NULL,                   -- 模型标识（AI 模型名或 engine:<engine>:<target> 命名空间）
  source_hash  TEXT    NOT NULL,                   -- HMAC-SHA256(translate.key, 归一化原文) hex（原文不落明文）
  target       TEXT    NOT NULL,                   -- 模型译文
  hits         INTEGER NOT NULL DEFAULT 1,         -- 命中次数（查价值用，后续可按它清理）
  created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
  updated_at   TEXT    NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (model, source_hash)
);

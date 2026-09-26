-- 翻译记忆（整页汉化提速 · 2026-09-27）
--
-- 目的：把模型翻过的「模型 + 归一化原文 → 译文」持久存下来。重访同一页面、
--       常见站点 UI 文案（Sign in / Settings …）、以及重新汉化时直接命中缓存，
--       一次 6 秒固定开销的 opencode 调用都不用发。
--
-- 为什么按模型隔离：换了模型后旧译文风格/质量可能不一致，模型变了缓存自动作废，
--       不做"聪明"的跨模型复用。
--
-- 隐私：内容仍然只在本机库里（app 数据目录 700/数据库文件 600），与既有策略一致。

CREATE TABLE IF NOT EXISTS translate_memory (
  model       TEXT    NOT NULL,                   -- 模型标识（opencode /config 的 model，如 opencode/big-pickle）
  source      TEXT    NOT NULL,                   -- 归一化原文（压缩空白后的待译文本，可能含 ⟦n⟧ 结构标记）
  target      TEXT    NOT NULL,                   -- 模型译文
  hits        INTEGER NOT NULL DEFAULT 1,         -- 命中次数（查价值用，后续可按它清理）
  created_at  TEXT    NOT NULL DEFAULT (datetime('now')),
  updated_at  TEXT    NOT NULL DEFAULT (datetime('now')),
  PRIMARY KEY (model, source)
);

-- 阶段 3.2 空间（Spaces）：工作区升级为"上下文隔离容器"
-- 每个空间带一个主题色（accent），用于切换器标识与界面强调色。
-- 空字符串 = 用默认强调色（跟随主题）。
ALTER TABLE workspaces ADD COLUMN color TEXT NOT NULL DEFAULT '';

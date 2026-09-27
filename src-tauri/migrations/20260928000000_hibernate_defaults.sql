-- 休眠策略收紧（第一期止血 · 内存）：闲置 30 分钟→10 分钟，数量阈值关闭→8 个。
-- 只动旧默认值（用户手工改过的不碰）；新装无键时由 contracts 默认值兜底。
UPDATE settings SET value = '10' WHERE key = 'tab_hibernate_minutes' AND value = '30';
UPDATE settings SET value = '8' WHERE key = 'tab_hibernate_count' AND value = '0';

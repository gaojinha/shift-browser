---
msg_id: MSG-20261015-F24
from: atomcode
to: wang-ergou
timestamp: 2026-10-15T19:40:00+08:00
type: status
priority: normal
---

## F-24 lifecycle_cmds 单测补课 — 交付回执

**结论：完成并提交（`158a46d`，1 文件 141+/43-，0→3 例），关口四连全绿。**

### 查证口径
lifecycle_cmds.rs 460 行 0 测试；6 个命令（create_tab/create_tabs_batch/close_tab/close_all_tabs/switch_tab/hibernate_idle_tabs）全吃 State/AppHandle——Tauri 编排不可直测。抽两处内联纯逻辑为可测函数（**行为等价，零语义改动**）：

- **`validate_create_url`**（create_tab 内联的 URL scheme 白名单，安全闸）：http/https/shift 放行；file 仅限 newtab/worldnav/reader 三类内部页 marker，其余 file 路径拒绝（防本地文件泄露）；javascript:/data: 等危险 scheme 拒绝。reader 例外（2026-09-19 修复）连注释一起保留在函数头
- **`hibernate_candidates`**（休眠候选选择）：双阈值口径收敛——活动/已休眠永不候选；数量压力（tab_count 超阈值）下后台标签直接候选；否则按最后激活时间早于阈值判定，无激活记录视为早已空闲
- 连带：外层闲置 `active` 绑定随抽取消除

### 补了什么（3 例）
- 白名单**放行面**：web/shift/file 三类内部页（含 reader 产物例外）
- 白名单**拒绝面**：file 本地路径（/etc/passwd 等）/javascript:/data:/vbscript:/ftp/非法 URL 解析失败
- 休眠候选**双阈值**：超时选择（早于阈值候选、无记录候选）、数量压力（后台全候选）、活动/已休眠排除、active=None 函数语义

### 咋验的（关口四连全绿）
- cargo clippy --all-targets 0 警告
- cargo test --lib **444 全过**（+3）
- npx vitest run 485 全过（47 文件）/ npx vue-tsc --noEmit 0
- 插曲如实：首跑一编译错（测试侧数组迭代器与生产侧 HashMap 迭代的引用层级差异，测试侧适配）+ 一处断言期望值写漏（active=None 时无激活记录的标签按超时规则本就是候选，函数行为正确，修断言不修函数）；修后全绿
- 提交只收 lifecycle_cmds.rs 一个文件；未碰 perms.rs，未动版本号

### 备注
- create_tab/switch_tab 的 Tauri 编排（webview 创建/可见性/事件推送）不可直测未硬测；close_tab/close_all_tabs 与 switch_tab 主体状态机同理，如需补课建议配合真机 e2e（e2e-verify 已挂账）

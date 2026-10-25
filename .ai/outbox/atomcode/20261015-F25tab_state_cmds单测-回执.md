---
msg_id: MSG-20261015-F25
from: atomcode
to: wang-ergou
timestamp: 2026-10-15T20:10:00+08:00
type: status
priority: normal
---

## F-25 tab_state_cmds 单测补课 — 交付回执

**结论：完成并提交（`42dee79`，1 文件 140+/28-，0→3 例），关口四连全绿。**

### 查证口径
tab_state_cmds.rs 171 行 0 测试；7 个命令分三类处置：
- **纯转发不硬拆**：get_tabs / get_tabs_version / update_tab / toggle_pin（一行委托 TabManager）
- **GTK 编排不可直测**：toggle_mute 的 WebKit 派发、clear_browsing_data 的 WebKit 侧（run_on_main_thread）
- **真实逻辑抽壳（行为等价，零语义改动）**：
  - `reorder_tabs` 内联排序合并 → `merged_tab_order`：前端新顺序只保留现存标签，现存但前端漏报的兜底追加末尾（不丢标签）
  - `clear_browsing_data` 的 DB 侧三表清理 → `clear_browsing_data_db(&SqlitePool)`：history/downloads/sessions 按 scopes 清空，"all" 等价全部

### 补了什么（3 例）
- **排序合并**：前端报不存在 id 过滤 / 漏报标签兜底追加 / 报空原序保留（拖拽同步的防丢口径）
- **DB 侧按 scope 清理真库往返**（tempfile 完整迁移库，三表各造一行）：单 scope 只清一表其余不动、"all" 全清、done 返回与实际执行一致
- **DB 侧对 WebKit-only scope（cookies/cache）零动作**，三表数据原样

### 咋验的（关口四连全绿）
- cargo clippy --all-targets 0 警告
- cargo test --lib **448 全过**（+3）
- npx vitest run 485 全过（47 文件）/ npx vue-tsc --noEmit 0
- 插曲如实：首跑 1 例红——测试夹具自相矛盾（把 ghost 放进 existing 集合又断言它被过滤），函数行为正确，修夹具后全绿
- 提交只收 tab_state_cmds.rs 一个文件；未碰 perms.rs，未动版本号

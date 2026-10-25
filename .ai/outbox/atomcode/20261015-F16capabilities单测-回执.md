---
msg_id: MSG-20261015-F16
from: atomcode
to: wang-ergou
timestamp: 2026-10-15T18:05:00+08:00
type: status
priority: normal
---

## F-16 capabilities.rs 单测补课 — 交付回执

**结论：完成并提交（`be116d7`，3 文件 53+/0-，9→12 例），关口四连全绿。卡片描述已过时，按代码原文核实后补真实缺口。**

### 查证修正（重要）
F-16 卡称「capabilities 模块缺单测」，**实际四个子模块已有 9 例**（registry_tests 5 / plugins 2 / agent_actions 1 / skills 1；registry 防漂移另有 api/handlers 的外部消费测试）。按铁律「数字必须来自代码原文」，核实后只补三处**真实缺口**，纯测试不改生产代码：

### 干了啥（三处缺口）
- **skills**：锁 `skills_list_json()` 输出形状——模块唯一公开函数此前零覆盖（现有测试只测 contracts 数据侧），MCP 消费方按此响应取数，漂移即红
- **plugins**：锁 `entry()` 全字段透传（kind/kindLabel/id/name/detail）+ 四形态 `as_str`/`togglable` 语义（仅 WebExtension 编译期加载不可启停）
- **agent_actions**：锁 `required` 参数（Agent 校验/提示词生成依赖）+ 动作 id 唯一性（enum 生成/白名单查重依赖）

### 咋验的（关口四连全绿）
- cargo clippy --all-targets 0 警告
- cargo test --lib **420 全过**（+3）
- npx vitest run 485 全过（47 文件）
- npx vue-tsc --noEmit 0 错

### 备注
- registry/table_read/table_write/core 的防漂移测试已在位（registry_tests.rs，含计数断言），未重复补
- agent_actions.rs 的 `#![allow(dead_code)]` 带豁免注释（接线前允许死代码），合规保留
- 插曲如实：新测试首跑 1 例红——我把入参 `enabled=false` 误断言成形态属性 `togglable=false`（McpServer 支持启停应为 true），生产代码无错，修断言后全绿
- 提交只收 capabilities 三文件，未碰 perms.rs，未动版本号

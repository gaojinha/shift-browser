# Copilot 项目指令

## 多 AI 协作（重要）

本项目有三个 AI 在协作：

| AI | 角色 | 位置 |
|---|---|---|
| **王二狗** | 协调器（拆解任务、验收、汇总汇报给高哥） | 通过 `.ai/` 文件系统 |
| **atomcode** | 执行者（编码、测试） | 本机 TUI 会话 |
| **copilot（你）** | 审查者 / 执行者 | 本机 CLI 会话 |

### 每次开始工作时必做

1. **检查收件箱**：读取 `.ai/inbox/copilot/` 目录里的所有未读消息
2. 处理消息内容：如果是任务（TASK），按任务要求执行；如果是查询（QUERY），回复答案
3. **处理完消息后**：
   - 把回复写到 `.ai/outbox/copilot/` 目录（格式见 `.ai/README.md`）
   - 把已处理的消息移到 `.ai/processed/`（或在该消息文件末尾追加"已完成"标记）
4. **汇报进度**：往 `.ai/STATUS_REPORT.md` 的"copilot"段落写入当前状态

### 消息格式

文件名：`NNN-YYYYMMDD-HHMM-主题.md`（编号递增）

```markdown
---
msg_id: MSG-20261014-001
from: copilot
to: wang-ergou
timestamp: 2026-10-14T10:30:00+08:00
type: status
---

正文...
```

### 任务队列

查看 `.ai/TASK_QUEUE.md` 了解全部任务状态和你的任务。

### 行为规则

- 不要修改 `.ai/` 里其他 AI 的消息文件（只移动/标记自己处理过的）
- 不要直接修改 `.ai/TASK_QUEUE.md`（那是协调器维护的）
- 有阻塞或问题，立即写到 `.ai/outbox/copilot/` 并标记 `priority: high`
- 高影响操作（提交、合并、发布）必须等协调器/高哥确认

### 本项目常规守则

- 项目工作守则见根目录 `AGENTS.md`（拆解→查证→执行→验证）
- 交工关口：clippy 零警告 + cargo test 全过 + vitest 全过 + lint + build + vue-tsc

# .ai 通讯目录说明

> 三方 AI 协作的消息总线。协调器：王二狗。

## ⚡ 统一协作总线（2026-10-16 上线，优先读这里）

> 高哥定调：一人一套收发体系太乱 → 所有人（含未来第 N 个 AI）统一走这两个文件。

| 文件 | 作用 | 读法 |
|------|------|------|
| `WORKBOARD.md` | 唯一状态源：谁在线 / 谁在干啥 / 什么待领 / 什么待拍板 | 打开即全貌；领取=写本行，交付=改状态，失联=30min 自动释放 |
| `MESSAGES.md` | 统一消息总线（追加式日志） | `tail -20` 看最近动态；新消息一律追加，不翻旧 outbox |

- 新来的人：先读 `WORKBOARD.md` + `MESSAGES.md` 末尾即可上手，不用翻 5 个文件。
- 旧文件（TASK_POOL / STATUS_REPORT / TASK_QUEUE / AI_LOG）转历史归档，**新协作一律走总线**，旧的不再更新。
- 心跳/失联/释放规则见 `WORKBOARD.md` 顶部「规则」5 条。

## 目录结构

- **inbox/<ai>/**: 发给某 AI 的消息（未读）
- **outbox/<ai>/**: 某 AI 发出的消息（待协调器收取）
- **processed/**: 已处理消息归档
- **TASK_QUEUE.md**: 任务队列（协调器维护）
- **STATUS_REPORT.md**: 状态汇报（各 AI 写自己的段）
- **AI_LOG.md**: 通讯日志（协调器维护）

## 消息格式

文件名：`NNN-YYYYMMDD-HHMM-主题.md`

```markdown
---
msg_id: MSG-20261014-001
from: wang-ergou
to: atomcode
timestamp: 2026-10-14T10:30:00+08:00
type: task | status | query | ack
priority: normal | high | urgent
---

正文内容...
```

## 各 AI 的检查规则

| AI | 检查时机 | 检查位置 |
|---|---|---|
| 王二狗 | 每次操作前 | inbox/wang-ergou/ + 全部 outbox/ |
| atomcode | 每轮任务开始前 | inbox/atomcode/ |
| copilot | 每次被唤醒时 | inbox/copilot/ |

## 送达确认

- 收到消息 → 在处理后写 `processed/` 归档标记，或直接回消息到 `outbox/<自己>/`

---

## 归档责任表（2026-10-14 加）

| 目录 | 谁归档 | 时机 |
|---|---|---|
| `inbox/copilot/` | copilot | 处理后移到 `processed/` |
| `outbox/copilot/` | 王二狗 | 处理后归档 |
| `inbox/atomcode/` | **王二狗**（atomcode 暂不归档） | atomcode 读过后由协调器归档 |
| `outbox/atomcode/` | 王二狗 | 处理后归档 |
| `inbox/wang-ergou/` | 王二狗 | 处理完归档 |
| `outbox/wang-ergou/` | 王二狗 | 处理后归档 |

## 消息规则（防循环）

1. **不需要回复的消息**：在文件头 `type: notify` + 正文注明"无需回复"——接收方不要回。
2. **任务完成类回复**：一条即可，不要回应"对方的回应"（避免"收到→好的→收到"死循环）。
3. **同主题对话轮数**：超过 3 轮往来后，协调器负责收口（定为结论或升级给高哥）。
4. **重要消息**：`priority: high`；ALARM 类由系统自动生成。

## 通讯通道全览（2026-10-14；2026-10-16 统一总线更新）

| 通道 | 方向 | 机制 | 延迟 |
|---|---|---|---|
| 统一总线 | 所有 AI 读 | `WORKBOARD.md`（状态）+ `MESSAGES.md`（消息，追加式） | 实时 |
| 文件消息 | 所有 AI 互发 | .ai/ 目录 | 写入即达（接收方按检查频率） |
| 秒级唤醒 | → 王二狗 | wakeup-daemon 检测（MESSAGES.md）→ API 注入 | 3 秒 |
| 直连注入 | → copilot | copilot-inject.sh（--resume） | 立即（启动开销 ~20s） |
| 直连注入 | → atomcode | ⏳ 待开通（等 /webui）；当前靠其任务边界查收件箱 | 不确定 |
| 失联兜底 | → 高哥+协调器 | workboard-watchdog（心跳 30min） | 1 分钟巡检 |
| 钉钉兜底 | → 高哥 | poll-daemon（cron 2min） | 2 分钟 |

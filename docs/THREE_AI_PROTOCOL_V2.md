# 三方 AI 协作通讯协议 v2.0

> 版本：v2.0（带确认机制）  
> 日期：2026-10-14  
> 核心改进：解决"异步无确认"问题

---

## 一、问题诊断

### 文件通讯的缺陷
| 问题 | 后果 |
|---|---|
| 写后不知对方是否读到 | 任务可能没人执行 |
| 无法确认理解是否一致 | 各干各的，方向跑偏 |
| 紧急消息无法及时触达 | 阻塞时无法快速上报 |
| 状态变更无通知 | 协调器不知道进度 |

---

## 二、解决方案：三层通讯架构

```
┌─────────────────────────────────────────┐
│         第三层：实时通知                │
│   钉钉消息（高优先级/阻塞/完成）         │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│         第二层：确认机制                │
│   message_id + ack_file + 心跳检测       │
└─────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────┐
│         第一层：异步通讯                │
│   .ai/ 目录共享文件                     │
└─────────────────────────────────────────┘
```

---

## 三、确认机制设计

### 3.1 消息格式

每条消息带唯一 ID：

```json
{
  "msg_id": "MSG-20261014-001",
  "timestamp": "2026-10-14T09:30:00+08:00",
  "from": "wang-ergou",
  "to": "atomcode",
  "type": "task_assign",
  "priority": "normal",
  "content": { ... }
}
```

### 3.2 ACK 机制

**发送方**写消息时，同步创建等待 ACK 文件：
```
.ai/pending/MSG-20261014-001.json
```

**接收方**读到消息后，写 ACK 文件：
```
.ai/acked/MSG-20261014-001.json
{
  "msg_id": "MSG-20261014-001",
  "acked_by": "atomcode",
  "acked_at": "2026-10-14T09:31:00+08:00",
  "action": "accept"  // accept / reject / need_clarify
}
```

**发送方**定期检查 pending 目录：
- 5 分钟内收到 ACK → 通讯成功
- 10 分钟无 ACK → 发钉钉提醒
- 30 分钟无 ACK → 标记为"未送达"，重新派发

### 3.3 心跳检测

每个 AI 每 5 分钟更新心跳文件：
```
.ai/heartbeat/wang-ergou.json
.ai/heartbeat/atomcode.json
.ai/heartbeat/copilot.json
```

内容：
```json
{
  "ai_id": "atomcode",
  "last_heartbeat": "2026-10-14T09:35:00+08:00",
  "status": "working",  // working / idle / blocked
  "current_task": "T-003"
}
```

协调器每 10 分钟检查：
- 超过 15 分钟无心跳 → 发钉钉询问状态
- 心跳恢复 → 记录恢复时间

---

## 四、文件结构

```
.ai/
├── messages/              # 消息队列
│   ├── incoming/          # 待接收
│   ├── pending/           # 等待 ACK
│   └── acked/             # 已确认
├── heartbeat/             # 心跳文件
├── TASK_QUEUE.md          # 任务队列
├── STATUS_REPORT.md       # 状态汇报
├── AI_LOG.md              # 通讯日志
└── lock/                  # 锁文件
```

---

## 五、通讯流程

### 5.1 任务派发

```
王二狗:
  1. 写消息到 messages/incoming/atomcode/
  2. 创建 pending 文件
  3. 启动定时器检查 ACK
  
atomcode:
  4. 每 2 分钟轮询 incoming 目录
  5. 发现新消息 → 读 + 写 ACK
  6. 开始执行任务
  
王二狗:
  7. 检测到 ACK → 任务生效
  8. 10 分钟无 ACK → 钉钉提醒 atomcode
```

### 5.2 进度汇报

```
atomcode:
  1. 每 15 分钟写 STATUS_REPORT.md
  2. 写心跳文件
  
王二狗:
  3. 每 10 分钟检查心跳
  4. 汇总进度写 PROGRESS.md
  5. 有阻塞或完成 → 钉钉通知高哥
```

### 5.3 阻塞上报

```
atomcode:
  1. 发现阻塞 → 立即写 messages/incoming/wang-ergou/
  2. 写阻塞报告到 STATUS_REPORT.md
  3. 心跳状态改为 blocked
  
王二狗:
  4. 检测到阻塞 → 立即钉钉高哥
  5. 协调资源解决问题
```

---

## 六、实现方案

### 6.1 轮询脚本（各 AI 运行）

```bash
# 每 2 分钟检查新消息
while true; do
  for msg in .ai/messages/incoming/*/; do
    if [ -f "$msg" ] && [ $(stat -c%Y "$msg") -gt $last_check ]; then
      # 读取消息
      content=$(cat "$msg")
      # 写 ACK
      msg_id=$(echo "$content" | jq -r '.msg_id')
      echo "{\"msg_id\":\"$msg_id\",\"acked_by\":\"$(whoami)\",...}" > .ai/acked/$msg_id.json
      # 移动文件
      mv "$msg" ".ai/messages/processed/"
    fi
  done
  sleep 120
done
```

### 6.2 心跳脚本

```bash
# 每 5 分钟更新心跳
while true; do
  cat > .ai/heartbeat/$(ai_id).json << EOF
{
  "ai_id": "$(ai_id)",
  "last_heartbeat": "$(date -Iseconds)",
  "status": "$status",
  "current_task": "$current_task"
}
EOF
  sleep 300
done
```

### 6.3 协调器监控脚本

```bash
# 每 10 分钟检查
while true; do
  # 检查心跳
  for ai in atomcode copilot; do
    last_hb=$(jq -r '.last_heartbeat' .ai/heartbeat/$ai.json)
    if [ $((now - last_hb)) -gt 900 ]; then
      # 发送钉钉提醒
      send_dingtalk "$ai 心跳超时，请检查状态"
    fi
  done
  
  # 检查 pending ACK
  for msg in .ai/pending/*; do
    age=$(($now - $(stat -c%Y $msg)))
    if [ $age -gt 600 ]; then
      # 10 分钟无 ACK，钉钉提醒
      send_dingtalk "消息 $msg 未确认，已 10 分钟"
    fi
  done
  
  sleep 600
done
```

---

## 七、钉钉通知规则

| 场景 | 通知对象 | 内容 |
|---|---|---|
| 任务完成 | 高哥 | T-XXX 已完成，提交 XXX |
| 阻塞上报 | 高哥 | T-XXX 阻塞，原因：XXX |
| 心跳超时 | AI | XXX 心跳超时，请检查 |
| ACK 超时 | AI | 消息 XXX 未确认，已 10 分钟 |
| 协议变更 | 三方 | 新协议草案，请阅读确认 |

---

## 八、启动检查清单

- [ ] 各 AI 运行轮询脚本（检查 incoming 消息）
- [ ] 各 AI 运行心跳脚本
- [ ] 协调器运行监控脚本
- [ ] 钉钉 webhook 配置完成
- [ ] 三方阅读本协议并确认

---

**起草：王二狗**  
**日期：2026-10-14**  
**状态：待高哥审批**

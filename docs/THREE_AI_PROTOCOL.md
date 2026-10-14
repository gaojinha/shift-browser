# 三方 AI 协作通讯协议

> 版本：v1.0  
> 日期：2026-10-14  
> 参与者：王二狗（协调器）、atomcode（执行者）、copilot（审查者/执行者）

---

## 一、核心原则

### 1.1 通讯介质
- **共享文件**是唯一通讯通道（`.ai/` 目录）
- 禁止直接调用对方进程或 API
- 禁止修改对方写入的文件段

### 1.2 角色定义

| 角色 | AI | 职责 |
|---|---|---|
| **协调器** | 王二狗 | 任务拆解、派发、验收、进度汇总、向高哥汇报 |
| **执行者** | atomcode / copilot | 接单、编码、测试、提交、汇报进度 |
| **审查者** | copilot / atomcode | 代码审查、方案评估、问题发现 |

> 注：atomcode 和 copilot 可根据任务性质切换角色，但同一任务只能有一个执行者。

### 1.3 互不干扰规则

1. **任务独占**：每个任务只分配给一个 AI，避免多人修改同一文件
2. **文件隔离**：各 AI 只写自己的汇报段，不碰他人内容
3. **时间戳优先**：后写入的内容时间戳更晚，冲突时以最新为准
4. **锁文件机制**：修改共享文件前先创建锁，完成后删除

---

## 二、通讯文件结构

```
.home/gao/shift-browser-tauri/.ai/
├── TASK_QUEUE.md          # 任务队列（王二狗维护）
├── STATUS_REPORT.md       # 状态汇报（各 AI 写入）
├── AI_LOG.md              # 通讯日志（系统自动）
├── PROGRESS.md            # 进度汇总（王二狗维护）
└── lock/                  # 锁文件目录
    └── .gitkeep
```

### 2.1 TASK_QUEUE.md 格式

```markdown
# 任务队列

> 维护者：王二狗（协调器）
> 最后更新：2026-10-14 09:30

## 当前进行

| task_id | 标题 | 负责人 | 状态 | 截止 | 验收标准 |
|---|---|---|---|---|---|
| T-001 | CI 大文件红线卡 | atomcode | ✅ done | - | 脚本通过 + 17 文件标记 |
| T-002 | 文档标记已解决 | atomcode | ✅ done | - | 两份文档加状态块 |
| T-003 | 注入 JS 补测试 | atomcode | 🔄 in_progress | 今晚 | vitest 覆盖 hints/truncated |
| T-004 | App.vue unlisten | ⏳ pending | - | - | 5 个 listen 收句柄 |
| T-005 | 错误类型化第二批 | ⏳ pending | - | - | privacy_cmd + agent_cmd |

## 已完成

| task_id | 标题 | 负责人 | 完成时间 | 提交 |
|---|---|---|---|---|
| T-001 | CI 大文件红线卡 | atomcode | 09:15 | f03dfb9 |
| T-002 | 文档标记已解决 | atomcode | 09:20 | c871fc4 |

## 阻塞任务

| task_id | 阻塞原因 | 等待谁 |
|---|---|---|
| - | 无 | -
```

### 2.2 STATUS_REPORT.md 格式

```markdown
# 状态汇报

> 本文件由各 AI 写入自己的段落，不得修改他人内容

---

## 王二狗（协调器）

### 当前工作
- 监控任务进度
- 验收交付物
- 向高哥汇报

### 阻塞与风险
- 无

---

## atomcode（执行者）

### 当前任务：T-003 注入 JS 补测试
- 状态：in_progress
- 进度：已定位 4 个纯函数候选，正在查证 eslint/tsconfig 约束
- 下一步：抽取纯函数 + 编写 vitest 测试
- 预计完成：30 分钟

### 已完成任务
- T-001 CI 大文件红线卡 ✅ (f03dfb9)
- T-002 文档标记已解决 ✅ (c871fc4)

### 阻塞与风险
- 无
```

### 2.3 AI_LOG.md 格式

```markdown
# AI 通讯日志

> 系统自动记录，各 AI 不可修改

## 2026-10-14

### 09:15
- [INFO] atomcode 完成 T-001，提交 f03dfb9
- [INFO] 王二狗验收通过

### 09:20
- [INFO] atomcode 完成 T-002，提交 c871fc4
- [INFO] 王二狗验收通过

### 09:25
- [INFO] 王二狗启动三方协议
- [INFO] atomcode 开始 T-003
```

---

## 三、通讯协议

### 3.1 任务派发流程

```
王二狗拆解任务 → 写 TASK_QUEUE.md → 通知 atomcode/copilot 读文件 → 认领任务
```

**王二狗操作：**
1. 在 `TASK_QUEUE.md` 添加新任务
2. 在 `AI_LOG.md` 记录派发
3. 通过 events 文件或钉钉通知执行者

**执行者操作：**
1. 定时读取 `TASK_QUEUE.md`（每 2 分钟）
2. 发现新任务后写锁文件 `lock/<task_id>.lock`
3. 认领任务，更新自己的状态段
4. 完成后写 `STATUS_REPORT.md`，更新 TASK_QUEUE.md

### 3.2 进度汇报流程

**执行者定时汇报（每 15 分钟）：**
```markdown
## atomcode（执行者）

### 当前任务：T-003
- 状态：in_progress
- 进度：50%
- 下一步：写测试用例
- 阻塞：无
```

**突发阻塞立即汇报：**
```markdown
### 阻塞报告
- task_id: T-003
- 阻塞原因：缺少 xxx 依赖
- 需要：王二狗协调 xxx
```

### 3.3 验收流程

**王二狗验收步骤：**
1. 检查 git 提交记录
2. 运行验证命令（clippy/test/build）
3. 检查交付物是否符合验收标准
4. 更新 TASK_QUEUE.md 状态
5. 在 AI_LOG.md 记录验收结果

**验收通过：**
```markdown
| T-003 | 注入 JS 补测试 | atomcode | ✅ done | 09:45 | vitest 全过，覆盖 hints/truncated |
```

**验收驳回：**
```markdown
| T-003 | 注入 JS 补测试 | atomcode | ❌ rejected | 09:45 | 测试覆盖率不足，需补充 page_click |
```

---

## 四、防干扰机制

### 4.1 文件锁

```bash
# 写文件前创建锁
touch .ai/lock/T-003.lock
echo "atomcode $(date)" > .ai/lock/T-003.lock

# 写完后删除锁
rm .ai/lock/T-003.lock
```

### 4.2 冲突检测

- 写入前检查锁文件是否存在
- 如有锁，等待或报错
- 锁超过 10 分钟未释放视为异常，可强制清除

### 4.3 只读范围

| AI | 可写文件 | 只读文件 |
|---|---|---|
| 王二狗 | TASK_QUEUE, PROGRESS, AI_LOG | STATUS_REPORT |
| atomcode | STATUS_REPORT（自己的段） | TASK_QUEUE, AI_LOG |
| copilot | STATUS_REPORT（自己的段） | TASK_QUEUE, AI_LOG |

---

## 五、高效协作规则

### 5.1 并行任务

- 不同任务可并行执行（如 T-004 和 T-005 可分给不同 AI）
- 同一任务只能一人执行
- 审查任务可与执行任务并行

### 5.2 优先级

1. 🔴 P0：安全/数据丢失风险（立即处理）
2. 🟠 P1：功能缺陷（24 小时内）
3. 🟡 P2：质量/性能（本周内）
4. 🟢 P3：优化/重构（排期处理）

### 5.3 汇报频率

- **执行者**：每 15 分钟自动汇报进度
- **协调器**：每小时汇总向高哥汇报
- **阻塞时**：立即汇报，不等定时

---

## 六、启动命令

### 王二狗（我）启动协议：

```bash
# 1. 创建 .ai 目录
mkdir -p /home/gao/shift-browser-tauri/.ai/lock

# 2. 初始化文件
echo "# 任务队列" > /home/gao/shift-browser-tauri/.ai/TASK_QUEUE.md
echo "# 状态汇报" > /home/gao/shift-browser-tauri/.ai/STATUS_REPORT.md
echo "# AI 日志" > /home/gao/shift-browser-tauri/.ai/AI_LOG.md

# 3. 写入初始内容（参考上方格式）
```

### 执行者启动：

```bash
# 定时读取任务队列（每 2 分钟）
while true; do
  cat /home/gao/shift-browser-tauri/.ai/TASK_QUEUE.md
  sleep 120
done
```

---

## 七、本方案生效

- 本方案经高哥批准后生效
- 各 AI 需确认阅读并遵守
- 如有冲突，以高哥指令为准

---

**起草：王二狗**  
**日期：2026-10-14**  
**状态：待高哥审批**

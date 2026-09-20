# 标准跟踪报告：WebMCP 与 Agentic Browsing（2026-09-20）

> 阶段 1.5 · 标准跟踪（strategy-2026.md 1.5）
> 调研时间：2026-09-20
> 结论性质：本报告为**现状调研 + 能力差距分析 + 落地路径建议**，不承诺实现时间表。

---

## 一、WebMCP 标准现状（2026-09 时点）

### 1.1 是什么
WebMCP（Web Model Context Protocol）是 **Google 提出的 Web 标准提案**，让网站向 AI 智能体暴露**结构化工具**（点击、填表、查询等可编程操作），智能体无需猜测 DOM 即可精确操作网站。与 MCP（Model Context Protocol，Anthropic 提出的客户端-服务器工具协议）互补：
- **MCP**：AI 客户端 ←→ 本地/远端工具服务器（ShiftBrowser 阶段 1.3 已实现客户端）
- **WebMCP**：AI 客户端 ←→ **网站自身暴露的工具**（浏览器内运行，如 `navigator.modelContext.registerTool`）

### 1.2 时间线（截至 2026-09）
| 时间 | 事件 |
|---|---|
| 2026-02-10 | Chrome for Developers 发布《WebMCP is available for early preview》 |
| 2026-05-15 | Chromium "Intent to Experiment" 提交，**三天后批准** |
| 2026-05 | Google I/O 2026 正式宣布 WebMCP |
| 2026-06 | InfoQ 报道：WebMCP 在 Chrome **公开 origin trial** 可用（Chrome 149–156） |
| 持续 | W3C **Web Machine Learning CG** 孵化标准化 |

### 1.3 机制要点
- 核心 API：`navigator.modelContext.registerTool(def)`（JS 注册工具声明）
- 另有**声明式变体**：基于 HTML 表单注释标注
- **工具描述预算**（官方建议，防 prompt 溢出）：
  - 工具描述 ≤ 500 字符
  - 参数描述 ≤ 150 字符
  - 工具名/参数名 ≤ 30 字符
  - 单个工具输出 ≤ 1,500 字符
- 官方配套：WebMCP 工具安全指南 + 最佳实践

### 1.4 浏览器支持
- **Chromium 系**：Chrome 149–156 origin trial（需注册 origin 试用）
- **WebKitGTK 2.52.3（ShiftBrowser 当前引擎，系统冻结）**：**不支持** `navigator.modelContext`（WebKit 无此 API 实现，且版本冻结无法升级）→ 这是引擎天花板，与 strategy 既有判断一致

---

## 二、Agentic Browsing 审计（Lighthouse 13.3）现状

### 2.1 是什么
Lighthouse 13.3.0（2026-05-07）把 **Agentic Browsing** 类别从实验性移入**默认配置**（Chrome 150+ 直接可用）。评估网站对机器交互（AI 智能体）的友好度。

### 2.2 评分方式
**不同于其他类别的 0-100 加权分**：目前是**确定性检查 + 通过率（pass/fail 比率）+ 信息性计数**。原因：agentic web 标准仍在演进，现阶段重点是收集数据而非打分。

### 2.3 检查项（四类信号）
| 信号 | 检查内容 |
|---|---|
| **WebMCP 工具注册** | 网站是否用 `registerTool` 注册工具；schema 是否有效；是否覆盖页面表单 |
| **无障碍树健康**（代理为中心） | 名称和标签（每个互动元素有程序化名称）；树完整性（角色/父子关系有效）；可见性（互动时内容不从无障碍树隐藏） |
| **稳定性** | Cumulative Layout Shift (CLS)：视觉稳定性（代理依赖元素定位） |
| **可发现性** | llms.txt：网域根目录是否存在机器可读摘要（llms.txt 为提案，当前 AI 工具采用率低） |

### 2.4 关联动态
- 2026-09-15：**Cloudflare 新域名默认阻止 Training 与 Agent 爬虫**（广告页面），Search 保持默认允许——"agent 流量"正在成为独立治理对象
- llms.txt：提案标准（Jeremy Keith 提出），AI 工具采用率有限，但 Lighthouse 已纳入检查

---

## 三、ShiftBrowser 能力差距分析

| 维度 | 标准要求 | ShiftBrowser 现状 | 差距 |
|---|---|---|---|
| 消费 WebMCP 工具 | 读取网站 `registerTool` 声明 | 无（引擎不支持，且无 polyfill） | **大**：AI 面板无法利用网站暴露的 WebMCP 工具 |
| 页面理解 | 无障碍树为主数据模型 | 阶段 1.1 语义快照（DOM 提取，非无障碍树） | 中：可补无障碍树（aria-label/role）提取 |
| llms.txt | 检测网域根 `/llms.txt` | 无 | 中：可在页面理解时检测并纳入上下文 |
| 网站侧 agentic 自测 | Lighthouse 13.3 类别 | 无 | 中：可做"页面对 AI 友好度"检测工具 |
| 表单覆盖 | WebMCP 工具覆盖页面表单 | Agent 动作（1.4）用 CSS 选择器操作表单 | 中：WebMCP 声明式表单标注可提升精度 |
| 稳定定位 | CLS 低 | 引擎级无 CLS 控制 | 低：Agent 依赖选择器，CLS 影响小（当前单页任务） |

---

## 四、落地路径建议（按价值/成本排序）

### 4.1 【高价值·低成本】WebMCP 客户端 polyfill 试验（推荐首做）
- **思路**：引擎不支持 `navigator.modelContext` 没关系——注入 polyfill 捕获网站的工具声明，转成 ShiftBrowser 的 function calling 工具（复用阶段 1.3/1.4 工具执行链路）
- **做法**：
  - 注入脚本定义 `navigator.modelContext.registerTool(def)`（仅捕获声明到 `window.__webmcp_tools`，不伪造执行能力）
  - 语义快照（content_snapshot）附带捕获的 WebMCP 工具
  - AI 面板把 WebMCP 工具作为可用工具暴露（与 MCP/Agent 工具并列）
- **风险控制**：作为**可开关试验特性**（设置项，默认关），不干扰正常浏览；polyfill 只实现 registerTool，其他 API 不伪造
- **价值**：即使 WebKit 不支持，ShiftBrowser 也能"消费"WebMCP 生态；为将来引擎升级（若换 Chromium 内核）预演

### 4.2 【中价值·低成本】llms.txt 检测增强
- 页面理解时检测 `https://域名/llms.txt`（HEAD 请求，超时短），存在则纳入快照上下文
- 注意：这是出网请求，需授权/限流（与 content_extract 同策略）

### 4.3 【中价值·中成本】无障碍树语义快照增强
- 语义快照（1.1）补充 aria-label/role/表单 label 提取，对齐"代理为中心的无障碍"信号
- 提升 Agent（1.4）动作定位精度

### 4.4 【低价值·中成本】网站 agentic 自测工具
- 做一个"页面对 AI 友好度"检测（无障碍树健康 + 表单可访问性 + llms.txt 存在性），对齐 Lighthouse 检查项
- 价值有限（浏览器工具对网站侧优化帮助小），可后置

---

## 五、标准跟踪机制（strategy 承诺落地）

strategy-2026.md 承诺"标准跟踪机制，每季度复核规划"。落地为：

| 项 | 内容 |
|---|---|
| 跟踪对象 | WebMCP（origin trial 进度/正式化）、MCP 协议版本、llms.txt、Lighthouse Agentic Browsing、Cloudflare/平台 agent 流量治理政策 |
| 复核节奏 | **每季度**（下次 2026-12-20） |
| 触发复核 | 季度末或任一对象有重大进展（如 WebMCP 正式化、WebKit 支持）时即时复核 |
| 复核输出 | 更新本报告 + strategy 相关章节 |
| 责任人 | 王二狗（每季度主动提醒高哥） |

---

## 六、结论

1. WebMCP 已进入 Chrome 公开 origin trial（149–156），**标准方向明确**，但 WebKitGTK 引擎不支持——ShiftBrowser 需走**客户端 polyfill 试验**路径
2. Lighthouse 13.3 的 Agentic Browsing 类别明确了"agent 友好网站"的信号：WebMCP 工具、无障碍树、CLS、llms.txt
3. 建议**首做 4.1（WebMCP polyfill 试验）**——成本低、复用现有工具链路、为生态预演
4. 标准跟踪机制落地：每季度复核（下次 2026-12-20），由王二狗主动提醒
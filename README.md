# ShiftBrowser

> AI 时代的个人网络操作系统 —— 数据主权归用户、AI 原生、可自托管。

ShiftBrowser 是一个基于 Tauri 2 + Rust + Vue 3 的桌面端 AI 浏览器项目。它不是单纯的“浏览器应用”，而是在 WebKitGTK 上构建的、面向多任务协作与 AI 自动化的本地工作空间：页面内容可被本地理解、AI 助手可调用工具与执行动作、并在明确授权下完成浏览器级操作。

当前版本：v5.8.3
平台：Linux / GTK

---

## 1. 项目定位

ShiftBrowser 的定位可以概括为三层：

1. 浏览器内核层
   - 多标签页、地址栏、书签、历史、下载、工作区、会话恢复
   - WebKitGTK 提供真实网页渲染能力
   - 负责导航、页面生命周期、布局、事件、交互控制

2. AI 原生层
   - 页面快照、本地语义理解
   - AI 侧边栏、Provider 抽象、Agent 执行动作
   - MCP / Open API / tool registry 统一能力入口

3. 本地数据与安全层
   - SQLite 持久化
   - 密码、证书、会话、设置、审计日志
   - SSRF/内网拦截、TLS 检查、权限白名单、内容过滤、GPC

这使得它既是一款浏览器，也是一种“本地优先、AI 驱动、可审计”的网络操作平台。

---

## 2. 产品特点

### 2.1 浏览器基础能力

- 多标签页与多 WebView 组合布局
- 地址栏与搜索词智能判断
- 书签栏与书签管理面板
- 历史记录与清理
- 标签分组、工作区、会话恢复
- 下载管理与本地文件定位
- 阅读模式、页面提取与净化
- 按需翻译、跨网页理解
- 快捷键、全局命令、托盘集成
- 启动页、RSS 聚合、快捷入口
- 默认浏览器设置和单实例管理

### 2.2 AI 原生能力

- AI 侧边栏对话
- 页面上下文理解
- Agent 自动化执行（click/fill/scroll/read/...）
- 工具调用与动作白名单
- 多 Provider 抽象（OpenAI 兼容协议）
- MCP Client / MCP Server
- WebMCP 能力捕获与边界治理
- 自动任务编排与断点续跑

### 2.3 安全与隐私能力

- 内容过滤与广告拦截
- 反指纹与 UA/指纹治理
- GPC 请求头与站点声明支持
- 零遥测审计与网络出口收集
- 密码库加密与主密码保护
- 证书检查、异常放行与保存
- 明确的 SSRF 和内网访问限制
- 本地账号与 PBKDF2 安全哈希

### 2.4 可观测性与工程治理

- Selfcheck 自检
- 结构化日志
- 兼容性测试与 smoke 测试
- 统一常量/契约/能力表治理
- build.rs + ts-rs 自动生成前端类型

---

## 3. 技术栈

| 层 | 技术 |
|---|---|
| 桌面应用框架 | Tauri 2 + Rust |
| 浏览器内核 | WebKitGTK + GTK |
| 前端 | Vue 3 + TypeScript + Pinia + Tailwind + Vite |
| 数据库 | SQLite + sqlx |
| AI 通信 | reqwest + OpenAI 兼容协议 + MCP JSON-RPC |
| 安全 | AES-GCM / PBKDF2 / HMAC-SHA2 |
| 契约生成 | ts-rs + build.rs |

---

## 4. 代码结构

```text
.
├── src/                             # Vue 3 前端
│   ├── App.vue                      # 主应用布局
│   ├── components/                  # 地址栏、AI 面板、书签栏、设置与弹窗等
│   ├── stores/                      # Pinia 状态管理
│   ├── constants.ts                 # 前端常量入口
│   ├── types/                       # 类型定义
│   ├── utils/                      # 工具函数
│   └── styles/                      # 统一样式
│
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs                   # 应用入口，注册命令和插件
│   │   ├── app/                     # 启动、环境、panic、权限、初始化
│   │   ├── tabs/                    # 标签页生命周期与浏览器页面管理
│   │   ├── engine/                  # WebKit / 页面理解 / MCP / Agent / 内容过滤
│   │   │   ├── webview.rs
│   │   │   ├── layout.rs
│   │   │   ├── content.rs
│   │   │   ├── content_filter.rs
│   │   │   ├── ai_provider.rs
│   │   │   ├── mcp.rs
│   │   │   ├── agent.rs
│   │   │   ├── webmcp.rs
│   │   │   └── llmstxt.rs
│   │   ├── db/                      # SQLite 数据访问层
│   │   ├── api/                     # API / OpenAPI / tools / handlers
│   │   ├── ai.rs                    # AI 对话与工具链协调
│   │   ├── auto_tasks.rs            # 自动化任务编排
│   │   ├── reader.rs                # 阅读模式提取
│   │   ├── translate.rs             # 翻译
│   │   ├── tls.rs                  # TLS / 证书管理
│   │   ├── tray.rs                 # 系统托盘
│   │   ├── extensions.rs           # 用户脚本 / 扩展注入
│   │   ├── feed.rs                 # RSS 聚合
│   │   ├── netmon.rs               # 网络诊断与出口监测
│   │   ├── net_guard.rs            # 网络白名单与防护
│   │   ├── gpc.rs                  # GPC 处理
│   │   ├── audit.rs                # 审计日志
│   │   ├── selfcheck.rs            # 自检与健康检查
│   │   ├── perms.rs                # 权限审批与白名单
│   │   ├── default_browser.rs      # 默认浏览器设置
│   │   ├── useragent.rs            # UA 处理
│   │   ├── proto/                  # protocol / custom scheme 相关
│   │   └── bin/
│   │       └── mcp_server.rs       # 独立 MCP Server
│   │
│   ├── migrations/                 # SQLite schema / 迁移脚本
│   ├── icons/                      # 应用图标
│   ├── target/                     # 构建输出（本地）
│   └── Cargo.toml
│
├── docs/                            # 设计文档
├── package.json
├── tsconfig.json
├── vite.config.ts
├── README.md
├── rust-toolchain.toml
└── .gitignore
```

---

## 5. 架构分层

### 5.1 前端层（src/）

前端负责：

- 应用框架与布局
- 标签页 UI
- 地址栏输入与快捷键入口
- AI 侧边栏与对话展示
- 书签/历史/下载/设置弹窗
- 复杂状态管理

它本身不直接管理 SQLite，不直接操作底层 WebKit；前端通过 Tauri invoke 调用后端命令获取能力。

### 5.2 Tauri + Rust Runtime（src-tauri/src/lib.rs）

这是系统总入口。它负责：

- 注册 invoke handler
- 安装日志、panic hook、环境初始化
- 初始化单实例行为
- 启动应用窗口
- 绑定页面相关命令与数据命令

它是整个浏览器运行时的控制中心。

### 5.3 Tabs / WebView / Engine 层

这是浏览器最核心的底层抽象：

- TabManager 负责页面和状态生命周期
- WebView 负责真实网页渲染
- Engine 层封装与页面相关的底层能力，如：
  - JS 执行
  - 导航与状态查询
  - 内容过滤
  - 页面快照
  - 交互控制
  - 工具注册与能力收集

它是浏览器能力吸收的核心层。

### 5.4 数据层（db/ + migrations/）

负责：

- 书签、历史、下载、工作区、会话
- 密码、证书、AI 审计
- 设置项、用户配置、用户数据
- 迁移与结构演化

设计原则：

- 数据以本地为优先
- Rust 作为唯一控制入口
- 前端不直接访问数据库

### 5.5 AI 与自动化层

这个层包含：

- ai.rs
- auto_tasks.rs
- opencode_chat.rs
- webqa.rs
- webzh.rs
- agent_cmd.rs
- engine/agent.rs
- engine/mcp.rs
- engine/ai_provider.rs

职责：

- 提供模型调用接口
- 读取页面上下文
- 自动生成动作计划
- 执行 click / fill / scroll / submit 等动作
- 对接 MCP tools 和开放 API

### 5.6 安全治理层

横向管理整个系统的安全需要：

- perms.rs：权限与请求审批
- net_guard.rs：网络防护，内网地址过滤
- netmon.rs：网络诊断与出口监控
- tls.rs：证书检查和异常处理
- audit.rs：审计日志
- gpc.rs：隐私浏览标志处理
- selfcheck.rs：自检工具

安全能力不是独立“功能”，而是贯穿所有动作与入口。

---

## 6. 关键运行时流程

### 6.1 启动流程

1. 应用启动
2. 安装 panic hook
3. 执行环境初始化（env_setup）
4. 配置日志与插件
5. 注册 Tauri invoke handlers
6. 创建主窗口和 TabManager
7. 进入 UI 状态

### 6.2 浏览网页流程

1. 前端地址栏输入 URL / 搜索词
2. 前端调用 Rust 的 tab 创建 / 跳转命令
3. TabManager 创建 WebView / 绑定导航事件
4. WebKit 请求页面并渲染
5. 页面可被内容抽取器处理
6. 内容抽取与安全规则检查同步执行
7. 页面状态写入本地数据

### 6.3 AI 对话与动作执行流程

1. 用户在 AI 面板发起对话
2. Rust 获取当前页面上下文快照
3. AI Provider 生成响应或 tool call
4. 调用 MCP / Open API / Agent Action
5. Agent 对页面执行受控动作
6. 执行前经过白名单、权限和上下文校验
7. 返回结果给前端并记录审计日志

### 6.4 数据落库流程

- 历史记录、书签、设置、下载、证书、会话等均通过 Rust 层写入 SQLite
- 迁移脚本维护结构演化
- 关键状态可用于恢复、审计和隐私检查

---

## 7. 统一治理：为什么项目强调单一事实来源

项目中有一组极重要的工程约束：

- 常量不能散落在前后端各处
- IPC 接口名不能随机手写
- 设置项不能前端和后端不同步
- Tool schema 和能力声明不能“各写一份”
- 前后端类型不能靠人工维护

因此项目引入了统一治理设计：

- `src/constants.rs`：定义常量与关键配置
- `src/contracts.rs`：维护跨端契约
- `src/capabilities.rs`：维护能力清单
- `build.rs`：生成前端对应常量和契约输出
- `ts-rs`：生成前端 TS 类型

目标是让 “Rust 作为唯一事实来源”，前端、API、MCP 能力和设置项都从同一来源产生，减少漂移和不一致。

---

## 8. 设计亮点

### 8.1 本地优先

- 数据保存在本地 SQLite
- AI 能力本地上下文抽取
- 除非必要，不依赖远程状态

### 8.2 AI 可控

- 自动动作通过白名单执行
- 每步操作可审计
- 当前页上下文只在需要时传入
- 用户仍然处于“授权控制”的中心位置

### 8.3 安全收益明显

- SSRF 和内网访问保护
- TLS 与证书异常检测
- 隐私规则、GPC、内容净化
- 审计日志与可追溯记录

### 8.4 工程上有体系化思维

- 统一能力表
- 单一事实来源
- 迁移和测试驱动
- 多模块收口

---

## 9. 当前项目的主要工程风险

项目非常强，但是复杂度也明显较高，主要风险集中在下面几个方面：

1. Linux / GTK 偏强绑定
   - 当前平台偏 Linux
   - WebKitGTK 和 GTK 必须兼容性良好
   - 这会限制跨平台演进

2. 架构复杂度高
   - 前端、浏览器内核、AI、数据库、安全治理交织
   - 模块边界需要持续维护，否则容易出现耦合回退

3. Tauri unstable API 风险
   - 项目使用了 unstable 能力
   - 升级 Tauri 版本时，底层行为可能变化

4. 契约漂移风险
   - 这是项目特别强调治理的原因
   - 只要常量、契约或 schema 没有统一来源，就会出“看起来能用但实际上错位”的问题

5. AI + 浏览器控制 + 本地系统访问叠加
   - 这是强能力，但也是高风险面
   - 必须持续维护白名单、权限、审计边界

---

## 10. 适合的工程定位

从当前实现看，ShiftBrowser 更接近以下定位：

- 本地优先的 AI 浏览器平台
- 桌面端网络操作系统原型
- 可扩展的 MCP / OpenAPI 浏览器控制运行时
- AI 自动化浏览器的安全控制框架

它已经不再是“普通浏览器”的范畴，而是一个自带 AI 代理能力和本地控制能力的网络工作台。

---

## 11. 快速开始

### 依赖安装

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
```

### 安装前端依赖

```bash
npm install
```

### 开发模式

```bash
cd src-tauri
cargo tauri dev
```

### 生产构建

```bash
npm run build
cd src-tauri && cargo build --release
```

### MCP Server 构建

```bash
cd src-tauri
cargo build --release --bin shift-browser-mcp
```

---

## 12. 结语

ShiftBrowser 是一个典型的“高复杂度、AI 原生、桌面端、可扩展”的浏览器项目。它的价值在于：

- 将浏览器从“展示网页工具”提升到“AI 能操作的本地数字工作空间”
- 把传统浏览器的隐私、授权和本地数据治理做成第一等公民
- 把 MCP / OpenAPI / Agent 跟浏览器动作真正打通

从工程角度看，它已经具备相当强的系统设计基底；从产品角度看，它更像“下一代本地网络操作系统”的早期实现。

如果你要持续迭代这个项目，最关键的原则是：

- 继续收口 engine 与 tabs 的边界
- 继续强化统一契约机制
- 继续保持 AI 行为的白名单化与审计化
- 明确平台范围和版本兼容策略

---

## 13. 开发者详细架构说明

### 13.1 模块职责总表

| 模块 | 主要职责 | 关键依赖 | 设计目标 |
|---|---|---|---|
| `src/` | 前端 UI / 状态 / 展示 | Vue, Pinia, Tauri invoke | 用户交互与界面展示 |
| `src-tauri/src/lib.rs` | 总入口、插件注册、命令暴露 | Tauri, GTK | 系统启动与总控 |
| `src-tauri/src/app/` | 启动环境、panic、初始化、文件加固 | Rust std, GTK | 稳定启动与安全基线 |
| `src-tauri/src/tabs/` | tab 生命周期、webview 管理、导航控制 | WebKit, GTK | 标签页核心运行时 |
| `src-tauri/src/engine/` | 页面理解、内容过滤、MCP、Agent | WebKit, reqwest | 浏览器能力的底层封装 |
| `src-tauri/src/db/` | 本地数据库、数据访问 | SQLite, sqlx | 数据主权与状态保留 |
| `src-tauri/src/api/` | Open API / tool handlers / RPC 入口 | Tauri, serde | 对外能力暴露 |
| `src-tauri/src/ai.rs` | AI 编排、tool use、上下文 | OpenAI-compatible provider | 统一 AI 对话入口 |
| `src-tauri/src/auto_tasks.rs` | 自动任务编排 | Rust async | 批量工作流运行 |
| `src-tauri/src/extensions.rs` | 用户脚本/样式扩展 | WebView + DOM hooks | 扩展能力 |
| `src-tauri/src/reader.rs` | 阅读模式提取 | readability + ammonia | 无干扰阅读 |
| `src-tauri/src/translate.rs` | 翻译 | HTTP / provider | 页面理解增强 |
| `src-tauri/src/tls.rs` | 证书检查与例外管理 | Gio, GTlsCertificate | 安全可验证性 |
| `src-tauri/src/perms.rs` | 权限审批 | 规则存储 | 强制授权控制 |
| `src-tauri/src/net_guard.rs` | 网络访问拦截 | IP / URL 解析 | SSRF 与隐私守护 |
| `src-tauri/src/audit.rs` | 审计日志 | SQLite | 可追溯与安全分析 |
| `src-tauri/src/selfcheck.rs` | 自检健康检查 | Rust std | 启动状态验证 |
| `src-tauri/migrations/` | schema 版本管理 | SQLite SQL | 数据兼容与演化 |
| `src-tauri/src/bin/mcp_server.rs` | 独立 MCP Server | JSON-RPC / stdio | 外部 client 驱动 |

### 13.2 模块依赖图

```text
┌────────────────────────────────────────────────────────────────────┐
│                         Frontend Layer (Vue 3)                    │
│  src/                                                             │
│  App.vue                                                          │
│  components/*                                                     │
│  stores/*                                                         │
│  constants.ts                                                     │
│  types/*                                                          │
└───────────────────────────────────────┬────────────────────────────┘
                                        │ invoke / emit
                                        ▼
┌────────────────────────────────────────────────────────────────────┐
│                    Tauri Runtime + Command Bridge                    │
│  src-tauri/src/lib.rs                                             │
│  app/*                                                            │
│  misc_cmds, env_setup, panic_hook, harden, bootstrap              │
└───────────────────────────────────────┬────────────────────────────┘
                                        │
                                        ├──────────────► Tabs / WebView Core
                                        │                     │
                                        │                     ▼
                                        │          src-tauri/src/tabs/
                                        │          TabManager + navigation lifecycle
                                        │
                                        ├──────────────► Engine Layer
                                        │                     │
                                        │                     ▼
                                        │      src-tauri/src/engine/
                                        │      webview.rs / content.rs / agent.rs / mcp.rs
                                        │
                                        ├──────────────► Data Layer
                                        │                     │
                                        │                     ▼
                                        │       src-tauri/src/db/
                                        │       bookmarks / history / settings / downloads / users
                                        │
                                        ├──────────────► AI & Automation Layer
                                        │                     │
                                        │                     ▼
                                        │       ai.rs / auto_tasks.rs / webqa.rs / opencode_chat.rs
                                        │
                                        └──────────────► Security / Governance Layer
                                                              │
                                                              ▼
                                           perms.rs / net_guard.rs / tls.rs
                                           gpc.rs / audit.rs / selfcheck.rs
```

### 13.3 运行时调用链：从用户输入到 AI 自动操作

```text
用户输入 URL / 搜索词
        │
        ▼
Frontend AddressBar / Command Palette
        │
        ├─ 解析输入类型：URL / 搜索 / 书签 / 历史
        └─ 发送 Tauri invoke 请求
        │
        ▼
Rust tabs::commands
        │
        ├─ 解析用户意图
        ├─ 创建或复用 Tab
        ├─ 绑定导航事件
        ├─ 初始化 WebView
        └─ 更新 tab state / session cache
        │
        ▼
WebKitGTK WebView
        │
        ├─ 真实网页渲染
        ├─ DOM / JS / 网页事件
        └─ 网络请求与页面生命周期管理
        │
        ▼
Engine content extraction / page snapshot
        │
        ├─ 抽取正文、链接、表单、元素结构
        ├─ 安全检测（内容过滤、权限、内网检查）
        └─ 产出页面快照用于 AI
        │
        ▼
AI orchestration (ai.rs / engine/ai_provider.rs)
        │
        ├─ 读取页面快照 + 上下文
        ├─ 路由 Provider
        ├─ 选择 tool / action
        ├─ 调用 MCP / OpenAPI / Agent action
        └─ 执行受控动作（click/fill/scroll/navigation）
        │
        ▼
Action whitelist + permission gate + audit log
        │
        ├─ 校验是否允许写当前标签
        ├─ 校验是否允许访问该网站 / 该域名
        ├─ 检查是否命中风险策略
        └─ 记录审计事件
        │
        ▼
前端展示结果 / 页面状态更新 / 数据持久化
```

### 13.4 关键边界设计

#### 1) 前端不直接访问底层系统能力

设计原则：
- UI 层只负责展示交互
- 真正的能力都集中在 Rust 后端
- 这样能保证统一安全、权限和审计

缺点：
- 调用链更长
- 前端与后端契约要求更严格

#### 2) Engine 是统一封装层

集中封装 WebKit 相关能力，避免各业务模块直接调用底层 WebKit API，减少耦合。

典型职责：
- 页面抽取
- JS 执行
- 视图管理
- 内容过滤
- 网页操作

#### 3) AI 是“浏览器动作编排器”，不是独立魔法层

AI 能力依赖：
- 当前页面快照
- 当前 tab 状态
- tool registry
- Agent action whitelist
- permission gate

也就是说，AI 的价值依赖浏览器运行时的真实上下文，不是脱离浏览器运作的“聊天插件”。

#### 4) 安全是横切治理，不是单一模块

关键安全能力在多个模块同时参与：
- `perms.rs`：授权
- `net_guard.rs`：网络防护
- `tls.rs`：证书校验
- `audit.rs`：审计
- `content_filter.rs`：内容过滤
- `selfcheck.rs`：状态健康检查

### 13.5 统一治理设计的价值

这一层是这个项目最重要的工程能力之一。它解决的是长期项目中最容易出现的问题：

- 前端常量和后端常量不一致
- IOC / IPC 事件名漂移
- 设置键白名单不一致
- tool schema 与 UI 能力不一致
- 前后端 type definition 不一致

项目通过以下方式控制：

- `src/constants.rs`：定义规范常量与关键基础配置
- `src/contracts.rs`：维护跨端契约
- `src/capabilities.rs`：维护工具与技能能力表
- `build.rs`：生成 TS 端常量与契约输出
- `ts-rs`：从 Rust 类型生成前端 TS 类型

理论上，改一处即可自动同步到前端所需的能力和默认值，减少“看起来功能可用，实际上配置漂移”的问题。

### 13.6 数据流模型

```text
Browser UI
   ↓
Tauri invoke
   ↓
Rust command handlers
   ↓
TabManager / Engine / AI
   ↓
Local SQLite
   ↓
Persistence / restore / audit / security check
```

它体现出一个明确设计原则：

- UI 不直接做数据治理
- 页面控制不直接做数据库管理
- AI 不直接越过权限边界执行任意动作
- 所有改动都经过统一治理层

### 13.7 技术演进方向

未来如果继续迭代，建议从以下几个方向收口：

1. 进一步收束 `tabs` 和 `engine` 的职责边界
   - 避免 browser runtime 在多个模块中重复实现同类逻辑

2. 继续强化统一契约与能力表
   - 让所有工具、设置、事件、快捷键和 UI 入口都从同一套定义生成

3. 把 AI 自动化动作继续收为“可审计、可回放、可回滚”的任务模型
   - 让每一个 action 都可进行审计、重播、失败恢复

4. 保持平台策略清晰
   - 当前偏 Linux / GTK，后续若要扩展到其他平台，必须先把平台依赖抽离到统一接口层

5. 安全由“补丁式”升级为“工程化”闭环
   - 强制权鉴、网络过滤、证书校验、日志审计要由统一安全层持续维护，而不是分散修补

---

## 14. 总体判断

ShiftBrowser 的最核心价值并不只是“浏览器做得像 App”，而是它把浏览器升级成：

- 本地优先的网络工作平台
- AI 可理解与操作的页面环境
- 安全、审计、权限可控的 Web 自动化容器

因此，它不是一款单纯的浏览器，而是一个正在构建中的“本地 AI 代理浏览器平台”。

从工程实现上看，它已经具备：

- 较完整的浏览器基础能力
- 统一的 Rust 后端控制层
- 一套正在完善的安全与治理框架
- 较强的 AI / MCP / Open API 集成能力

从工程成熟度上看，它还处于：

- 有方向、有结构、有功能，但仍需持续收口和统一治理的阶段

如果继续保持当前的架构治理思路，并把能力边界和安全边界持续收紧，这个项目很有可能成为真正可持续的下一代“本地浏览器 + AI 控制平台”。

### 测试与检查

```bash
npm run lint                     # ESLint
npm run build                    # 含 vue-tsc 类型检查
cargo test                       # Rust 单测（数量以 cargo test --lib -- --list 实测为准）
scripts/smoke.sh                 # E2E 冒烟（10 项，完整模式会重启浏览器）
scripts/smoke.sh --no-restart    # 只检查当前实例
scripts/package-deb.sh           # 一键 deb 打包（校验工具链 → 构建扩展 → 前端 → release deb）
```

CI（GitHub Actions）：前端 lint + 构建；后端 `cargo fmt --check` + `check` + `clippy -D warnings` + `test`。

### AI 功能配置

AI 对话默认使用 AGNES AI（OpenAI 兼容协议）。API Key 两种配置方式：

- 环境变量 `AGNES_API_KEY`
- 设置页「AI 助手」区填入（掩码存储，不回显、不回写）

## 数据位置

| 内容 | 路径 |
|---|---|
| 应用数据目录（权限 700） | `~/.local/share/com.shiftbrowser.app/` |
| SQLite 数据库（权限 600） | `~/.local/share/com.shiftbrowser.app/shift-browser.db` |
| 密码主密钥（权限 600） | `~/.local/share/com.shiftbrowser.app/master.key` |
| 日志 | `~/.local/share/com.shiftbrowser.app/logs/` |

数据库备份：`scripts/backup.sh`（sqlite3 在线一致性备份，WAL 安全，默认保留 14 份），可挂 cron 定时。

## 路线图

战略全景见 [docs/strategy-2026.md](docs/strategy-2026.md)。

| 阶段 | 主题 | 状态 |
|---|---|---|
| 0 · 基座加固 | 引擎治理（版本冻结 + 跟踪）、引擎抽象层、DevTools、广告拦截 2.0、全局快捷键、E2E 冒烟、自检 | ✅ |
| 1 · AI 原生内核 | 页面理解 / Provider 抽象 / MCP 工具层 / Agent 执行层 / 标准跟踪 | ✅（持续增强） |
| 2 · 隐私主权 | 反指纹 2.0、GPC、零遥测审计、密码库主密码、E2E 加密同步（服务器零知识） | ✅（Passkeys 实测引擎未暴露 WebAuthn，已降级为密码管理器增强） |
| 3 · 交互形态与平台化 | 分屏 ✅、垂直标签 ✅、命令栏（Ctrl+K）✅、空间（Spaces）✅、真扩展体系（网络层拦截/头改写/热更新）✅、用户脚本 ✅、自动化（批量操作/定时任务/Agent 编排）✅、系统托盘 ✅ | ✅ |
| 4 · 开放生态 | 开放 API（48 工具 + 7 元/旁路命令，Unix socket + HTTP 双形态，端口可配 / 局域网开关 / eval 闸门 / 只读模式 `api_readonly` / IP 白名单 `api_allowed_ips`）、MCP Server（`shift-browser-mcp`，48 工具，AI 标准协议驱动）、多端协同（密码库/书签/会话/历史 E2E 加密同步）、自托管服务端套件（同步 / 工具网关 MCP / 管理页状态面板 / 模型网关） | ✅（持续增强：多端形态） |

## 文档

| 文档 | 说明 |
|---|---|
| [CHANGELOG.md](CHANGELOG.md) | 更新日志（Keep a Changelog 格式，自 v5.8.3 起启用） |
| [docs/archive/HANDOFF-2026-09-24.md](docs/archive/HANDOFF-2026-09-24.md) | 历史工程交接归档：逐轮交付记录、踩坑与根因分析、验证方法 |
| [docs/strategy-2026.md](docs/strategy-2026.md) | 战略规划：定位 / 五大支柱 / 分阶段路线图 / 风险治理 |
| [docs/standard-tracking-2026-09-20.md](docs/standard-tracking-2026-09-20.md) | WebMCP / Agentic Browsing 标准跟踪 |
| [docs/browser-privacy-fingerprint-research-2026.md](docs/browser-privacy-fingerprint-research-2026.md) | 浏览器指纹识别调研 |
| [docs/local-api.md](docs/local-api.md) | 本地开放 API：48 工具、Unix socket + HTTP 双形态、鉴权与配置 |
| [docs/跨平台移植规划_2026-09-22.md](docs/跨平台移植规划_2026-09-22.md) | Windows / Android 双端移植规划 |
| [scripts/](scripts/) | `smoke.sh`（E2E 冒烟）· `backup.sh`（数据库备份）· `build-extension.sh`（构建 Web 扩展 .so）· `package-deb.sh`（一键 deb 打包） |

## License

个人项目，未采用公开许可证（All Rights Reserved）。

# ShiftBrowser

> AI 时代的个人网络操作系统 —— 数据主权归用户、AI 原生、可自托管。

ShiftBrowser 是一款基于 **Tauri 2 + Rust + Vue 3** 的 AI 增强型桌面浏览器。它不是"又一个浏览器"，而是在 WebKitGTK 之上构建的、面向多任务与 AI 协作的个人工作空间：本地数据自持、AI 助手能读页面 / 调工具 / 在授权下执行任务、隐私能力可验证。

**当前版本：v5.7.0**（平台：Linux / GTK）

> **v5.7.0 更新**：AI 侧边栏交互修复（✕ 可即时点击关闭、打开后立即响应鼠标，根因 GtkOverlay natural-size 布局）+ 应用图标全平台统一（关于页 / 桌面 / 托盘 / 打包图标同源同图）。

---

## 特性

### 核心浏览器

- **多标签页**：单窗口内嵌多 webview（GtkOverlay + GtkFixed 绝对定位布局），标签拖拽排序、固定标签、逐标签静音、鼠标中键关闭
- **地址栏**：URL / 搜索词智能识别，书签 + 历史联合建议下拉，键盘导航（↑↓ / Enter / Esc）
- **书签**：书签栏 + 管理面板，文件夹、右键编辑 / 删除 / 新建文件夹
- **历史记录**：Ctrl+H 历史面板（搜索 / 单条删除 / 清空），记录开关可整体关闭
- **标签分组**：颜色分组、折叠展开、跨组拖拽归组，随会话持久化
- **工作区**：多套独立标签集合（类似 Chrome 配置文件），切换自动保存 / 恢复
- **会话**：自动保存 + 命名会话（Ctrl+Shift+S），重启恢复上次现场
- **下载**：真实进度跟踪、完成后打开 / 定位文件、记录自动清理，落盘目录跟随系统 XDG 下载目录（中文系统即 `~/下载`）
- **阅读模式**：正文提取（readability）+ HTML 净化（ammonia 白名单，防 XSS）
- **翻译**：划词 + 整页翻译，Google / Bing 引擎可切换、失败自动回退（Ctrl+Shift+T）
- **全局快捷键**：Ctrl+T / W / R / L / H、Ctrl+Tab 前后切换、F12 开发者工具 —— GTK 顶层统一路由，焦点在网页时也生效
- **启动页**：Bing 每日壁纸、RSS 资讯聚合（62 源：URL 去重 / 编码自动探测 / 标题质量过滤 / 失败重试）、快捷访问、天气
- **系统托盘**：关闭窗口 = 缩托盘（不退出），托盘左键单击显隐窗口；菜单四项：显示/隐藏窗口 · 下载管理 · 密码库 · 退出（ksni 走 D-Bus SNI，图标用应用自身图标）
- **默认浏览器**：设置页一键设为默认（xdg-settings 集成），desktop 文件声明 MimeType + `%U`，外部链接启动自动开新标签
- **单实例**：已有实例运行时新实例退出并把命令行 URL 转发给已有实例开新标签 + 窗口前置（tauri-plugin-single-instance）
- **登录持久化**：Cookie 落盘（cookies.sqlite），重启保留登录态

### AI 原生内核

- **AI 侧边栏**：对话问答、流式输出、停止生成、思考态 / 失败回显
- **页面理解**：本地语义快照（正文 / 表单 / 可交互元素 / 链接），AI 对话前自动附带页面上下文，提取全程本地不出网
- **多推理 Provider**：OpenAI 兼容协议（默认 AGNES AI），Provider 抽象层 + 模型路由，本地模型接口预留
- **MCP 客户端**：接入本地 MCP server（stdio / JSON-RPC 2.0，30s 超时保护），工具白名单 + 调用审计日志
- **Agent 执行层**：动作白名单（click / fill / scroll / read / wait / navigate / back / snapshot / submit）、**每步人工确认**、仅当前标签、密码框拒绝读写、参数 JSON 序列化防注入
- **多步任务编排**：`browser_action_plan` 整体确认 + 按序执行，动作回放 / 整批重试（断点续跑）
- **AI 友好度检测**：对齐 Lighthouse Agentic Browsing 四信号（WebMCP 工具注册 / 无障碍树 / CLS / llms.txt）
- **WebMCP polyfill 试验**（默认关闭）：捕获网站 WebMCP 工具声明，只捕获不伪造能力
- **MCP Server**：独立二进制 `shift-browser-mcp`（MCP 2025-03-26 stdio / newline-delimited JSON-RPC），25 个工具（13 读 + 12 写）与开放 API 同名同参，直连 Unix socket 零依赖；任何 MCP 客户端（opencode / Claude Desktop 等）配一行即驱动浏览器，eval / tool_call 不暴露

### 隐私与安全

- **广告拦截 2.0**：引擎级内容过滤（`WebKitUserContentFilterStore`，手写 FFI 对接），79 条内置网络层规则 + 自定义关键词，规则变更免重启即时生效
- **反指纹 2.0**：加噪 / 统一化双模式 + 指纹自测面板（"信任需要证明"）
- **GPC**：`Sec-GPC: 1` 请求头 + `/.well-known/gpc.json` 站点声明感知
- **零遥测审计**：全量网络出口清单，可验证"无外联"
- **密码库**：AES-256-GCM 字段级加密 + 主密码，主密钥独立文件（权限 600，损坏绝不静默重建），settings 敏感键前端不可见 / 掩码
- **数据保护**：数据目录 700 / 数据库文件 600，settings 读写白名单，CSP 全面配置
- **本地账号**：PBKDF2-SHA512（21 万迭代，旧账号验证成功后自动透明升级）
- **AI 调用 SSRF 防护**：仅允许 https、拒绝内网 / 本机地址
- **证书管理**：查看站点 TLS 证书（颁发者 / 有效期 / DNS 名 / 指纹 / 例外状态），自签 / 过期 / 主机名不匹配时拦截并提示，用户可一键放行并落库例外（gio GTlsCertificate，glib v2_70 特性）

### 可观测性

- **自检（selfcheck）**：17 项只读体检（版本环境 / 数据存储 / 运行时 / 能力状态），设置页一键查看，启动自动执行
- **结构化日志**：tauri-plugin-log，分级 + 模块 + 时间戳
- **E2E 冒烟**：`scripts/smoke.sh` 10 项关键链路检查（自检健康 / 引擎版本 / 快捷键 / 布局 / reparent / 注入 / 无 panic），防架构解耦回归

---

## 技术栈

| 层 | 技术 |
|---|---|
| 桌面框架 | Tauri 2.11（`unstable` 多 webview + `custom-protocol`） |
| 渲染引擎 | WebKitGTK 2.52（GTK 3；GtkOverlay + GtkFixed 多 webview 布局） |
| 前端 | Vue 3 + TypeScript + Pinia + Tailwind CSS 4 + Vite |
| 数据库 | SQLite（sqlx 0.8 异步连接池，WAL 模式，自建迁移系统） |
| 加密 | aes-gcm / pbkdf2 / hmac-sha2 |
| AI 通信 | reqwest（rustls，流式 SSE）· OpenAI 兼容协议 · MCP JSON-RPC 2.0 |
| 契约生成 | `build.rs`（常量/契约 → TS）+ `ts-rs`（Rust struct → TS interface） |

## 架构

```
src/                    # Vue 3 前端
├── App.vue             # 主布局：标签栏 / 地址栏 / 书签栏 / 内容区 / 状态栏
├── components/         # TabBar / VerticalTabBar / AddressBar / AISidebar / CommandPalette(Ctrl+K) / SpaceSwitcher / SettingsDialog / CertificatesDialog 等管理弹窗
├── stores/             # Pinia：tabs / bookmarks / ai / workspaces / settings / passwords / downloads / extensions / groups 等
└── types/              # 与 Rust 端的跨端契约（语义快照等）

src-tauri/src/
├── engine/             # 引擎抽象层（隔离 WebKitGTK 调用）
│   ├── layout.rs       #   内容区几何：布局常量唯一事实来源 + ContentGeometry
│   ├── webview.rs      #   webview 基础操作（设置 / JS 执行 / 导航状态 / 静音 / DevTools）
│   ├── content.rs      #   页面理解：语义快照（本地 DOM 提取，不出网）
│   ├── content_filter.rs # 引擎级内容过滤器（广告拦截 2.0，手写 FFI）
│   ├── ai_provider.rs  #   推理 Provider 抽象（云端 AGNES + 本地 stub）
│   ├── mcp.rs          #   MCP 客户端（stdio JSON-RPC 2.0）
│   ├── agent.rs        #   Agent 执行层（动作白名单 + JS 模板防注入）
│   ├── webmcp.rs       #   WebMCP polyfill（试验，默认关）
│   └── llmstxt.rs      #   llms.txt 检测（Agentic Browsing 信号）
├── db/                 # sqlx 数据层：bookmarks / history / passwords / sessions / settings / vault / sync / certificates / workspaces …
├── tabs.rs             # TabManager：多 webview 生命周期 + WebKit 信号回调
├── ai.rs               # AI 命令：对话 / 流式 / function calling 工具循环（≤6 轮）
├── api.rs              # 开放 API（28 命令，Unix socket + HTTP 双形态，鉴权 / 只读 / IP 白名单）
├── auto_tasks.rs       # 自动化任务（批量操作 / 定时任务 / Agent 编排）
├── extensions.rs       # 用户脚本 / 样式注入（UserContentManager）
├── feed.rs             # RSS 聚合（62 源，16 路并发，4MB 响应上限）
├── reader.rs           # 阅读模式正文提取 + ammonia 净化
├── translate.rs        # 翻译（选中文本 / 整页分批）
├── tray.rs             # 系统托盘（ksni → D-Bus SNI）
├── tls.rs              # 证书管理 / 监测 / 验证（P1-P3，gio GTlsCertificate）
├── netmon.rs           # 网络出口监控（零遥测审计数据源）
├── gpc.rs / audit.rs   # GPC 请求头 / 零遥测审计
├── selfcheck.rs        # 只读自检
├── bin/mcp_server.rs   # MCP Server 二进制（25 工具，stdio JSON-RPC 2.0）
└── migrations/         # SQLite 迁移（版本 + 事务 + 记录表）
```

关键设计决策：

- **不用前端 SQL**：SQLite 由 Rust 侧 sqlx 统一管理，前端经 `invoke()` 调命令，杜绝注入面
- **引擎抽象层**：WebKitGTK 调用收敛进 `engine/`，业务模块不再直接引 webkit2gtk（耦合度 tabs.rs 25 → 1）
- **多 webview 布局**：主 webview（Vue UI）+ 标签 webview 经 GtkFixed 绝对定位叠层，resize / 书签栏显隐 / AI 分屏由 `ContentGeometry` 一处算清
- **Agent 安全铁律**：动作白名单、每步人工确认、仅当前标签、凭据永不外泄

## 统一治理（单一事实来源）

> 规划与实施记录：[docs/架构-能力与常量统一治理方案_2026-09-23.md](docs/架构-能力与常量统一治理方案_2026-09-23.md)

### 为什么

常量、跨端契约、工具、技能、插件原先散落各处，靠注释写"与后端一致"人肉同步。已修复的典型漂移：

- 搜索引擎 URL **4 处**、默认 favicon **2 处**、布局高度**前端 + Rust + CSS 三层**各写一遍
- 后端设置白名单 **41 键** ↔ 前端默认值 **24 键**（缺 17 个 → `get()` 回退空串导致显示失真）
- MCP 工具 schema 手写 **200 行** + 工具名清单 **2 份** + Agent 动作表各写一遍
- 前端 `snap.webmcpTools` vs 后端 `webmcp_tools`（字段名不匹配 → AI 上下文**静默失效**）

### 机制

```
Rust 单一来源                    build.rs / 运行时生成              消费方
──────────────────────────────────────────────────────────────────────────────
src/constants.rs      ──────▶  src/constants.ts          ──▶ 前端 LAYOUT / SEARCH_ENGINES / UI / EVENTS / URLS / TIMEOUT / SHORTCUTS / SKILLS
src/contracts.rs      ──────▶  （同上文件）                ──▶ 后端设置白名单 + 前端默认值（同源）
src/capabilities.rs   ──────▶  capabilities/list API      ──▶ MCP Server 运行时拉取（零编译耦合）
src/tabs.rs 等 struct ──────▶  src/types/generated/       ──▶ 前端类型（cargo test --lib ts_export）
```

- **`src/constants.rs`**：布局尺寸 / 搜索引擎 / 色板 / favicon（自包含，供 `build.rs` `include!`）
- **`src/contracts.rs`**：跨端契约 —— 设置键（41）/ IPC 事件名（18）/ 内部 URL / 超时（14）/ 快捷键（12）/ 技能（4）
- **`src/capabilities.rs`**：能力总表 —— 工具（25）/ Agent 动作（9）/ 技能（4）/ 插件形态（4）
- **`src/types/generated/`**：由 `ts-rs` 从 Rust struct 生成（14 个类型），**前端零手写**

### 怎么加 / 改（改一处，两端自动跟上）

| 要加什么 | 改哪里 | 自动生效于 |
|---|---|---|
| MCP 工具 / 开放 API 命令 | `capabilities.rs`（schema）+ `api.rs`（实现） | MCP `tools/list`、API 分发 |
| AI 技能 | `contracts.rs::skills` | 前端快捷入口按钮 + `skills/list` |
| 设置项 | `contracts.rs::settings` | 后端白名单 + 前端默认值 |
| 跨端类型 | Rust struct 加 `#[derive(TS)]` + 导出测试 | `src/types/generated/` |
| 常量 / 快捷键 / 事件名 / 超时 | `constants.rs` / `contracts.rs` | 前端重新构建即得 |

### 校验

```bash
cd src-tauri && cargo test --lib ts_export   # 重新生成 TS 类型
cd src-tauri && cargo test --lib             # 含"防漂移"测试（IDS/ALL 与主表一致性等）
```

---

## 快速开始

### 前置依赖

- Rust 1.97.1（仓库 `rust-toolchain.toml` 已固定，rustup 自动切换）
- Node.js ≥ 20
- Linux 系统库：

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev patchelf
```

### 开发模式

```bash
npm install
cd src-tauri
cargo tauri dev        # 自动拉起 vite(5173) + 编译运行
```

### 生产构建

```bash
npm run build                    # vue-tsc 类型检查 + vite 构建 dist
cd src-tauri && cargo build --release
```

> **铁律**：Tauri release 把 `dist` 编译期嵌入二进制，改前端必须 `npm run build && cargo build --release`（前端在前），否则跑旧前端。

### MCP Server 构建（可选）

```bash
cd src-tauri
cargo build --release --bin shift-browser-mcp   # 生成 MCP Server 二进制（25 工具）
cp target/release/shift-browser-mcp ~/.local/bin/   # 装到 PATH，供 MCP 客户端调用
```

配置示例（opencode / Claude Desktop 等）：`{ "mcpServers": { "shift-browser": { "command": "shift-browser-mcp" } } }`，详见 [docs/local-api.md](docs/local-api.md)。

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
| 4 · 开放生态 | 开放 API（28 命令，Unix socket + HTTP 双形态，端口可配 / 局域网开关 / eval 闸门 / 只读模式 `api_readonly` / IP 白名单 `api_allowed_ips`）、MCP Server（`shift-browser-mcp`，25 工具，AI 标准协议驱动）、多端协同（密码库/书签/会话/历史 E2E 加密同步）、自托管服务端套件（同步 / 工具网关 MCP / 管理页状态面板 / 模型网关） | ✅（持续增强：多端形态） |

## 文档

| 文档 | 说明 |
|---|---|
| [HANDOFF.md](HANDOFF.md) | 工程交接：逐轮交付记录、踩坑与根因分析、验证方法 |
| [docs/strategy-2026.md](docs/strategy-2026.md) | 战略规划：定位 / 五大支柱 / 分阶段路线图 / 风险治理 |
| [docs/spike-engine-capabilities-2026-09-19.md](docs/spike-engine-capabilities-2026-09-19.md) | 引擎能力实测报告（WebKitGTK 边界） |
| [docs/standard-tracking-2026-09-20.md](docs/standard-tracking-2026-09-20.md) | WebMCP / Agentic Browsing 标准跟踪 |
| [docs/browser-privacy-fingerprint-research-2026.md](docs/browser-privacy-fingerprint-research-2026.md) | 浏览器指纹识别调研 |
| [docs/local-api.md](docs/local-api.md) | 本地开放 API：28 命令、Unix socket + HTTP 双形态、鉴权与配置 |
| [docs/spike-extension-capabilities-2026-09-21.md](docs/spike-extension-capabilities-2026-09-21.md) | 扩展体系能力边界实测（WebProcessExtension 网络层拦截） |
| [docs/架构修复方案_2026-09-22.md](docs/架构修复方案_2026-09-22.md) | 架构修复方案（待审批） |
| [docs/跨平台移植规划_2026-09-22.md](docs/跨平台移植规划_2026-09-22.md) | Windows / Android 双端移植规划 |
| [scripts/](scripts/) | `smoke.sh`（E2E 冒烟）· `backup.sh`（数据库备份）· `build-extension.sh`（构建 Web 扩展 .so）· `package-deb.sh`（一键 deb 打包） |

## License

个人项目，未采用公开许可证（All Rights Reserved）。

# ShiftBrowser

> AI 时代的个人网络操作系统 —— 数据主权归用户、AI 原生、可自托管。

ShiftBrowser 是一款基于 **Tauri 2 + Rust + Vue 3** 的 AI 增强型桌面浏览器。它不是"又一个浏览器"，而是在 WebKitGTK 之上构建的、面向多任务与 AI 协作的个人工作空间：本地数据自持、AI 助手能读页面 / 调工具 / 在授权下执行任务、隐私能力可验证。

**当前版本：v5.6.0**（平台：Linux / GTK）

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
- **下载**：真实进度跟踪、完成后打开 / 定位文件、记录自动清理
- **阅读模式**：正文提取（readability）+ HTML 净化（ammonia 白名单，防 XSS）
- **翻译**：划词 + 整页翻译，Google / Bing 引擎可切换、失败自动回退（Ctrl+Shift+T）
- **全局快捷键**：Ctrl+T / W / R / L / H、Ctrl+Tab 前后切换、F12 开发者工具 —— GTK 顶层统一路由，焦点在网页时也生效
- **启动页**：Bing 每日壁纸、RSS 资讯聚合（62 源：URL 去重 / 编码自动探测 / 标题质量过滤 / 失败重试）、快捷访问、天气

### AI 原生内核

- **AI 侧边栏**：对话问答、流式输出、停止生成、思考态 / 失败回显
- **页面理解**：本地语义快照（正文 / 表单 / 可交互元素 / 链接），AI 对话前自动附带页面上下文，提取全程本地不出网
- **多推理 Provider**：OpenAI 兼容协议（默认 AGNES AI），Provider 抽象层 + 模型路由，本地模型接口预留
- **MCP 客户端**：接入本地 MCP server（stdio / JSON-RPC 2.0，30s 超时保护），工具白名单 + 调用审计日志
- **Agent 执行层**：动作白名单（click / fill / scroll / read / wait / navigate / back / snapshot / submit）、**每步人工确认**、仅当前标签、密码框拒绝读写、参数 JSON 序列化防注入
- **多步任务编排**：`browser_action_plan` 整体确认 + 按序执行，动作回放 / 整批重试（断点续跑）
- **AI 友好度检测**：对齐 Lighthouse Agentic Browsing 四信号（WebMCP 工具注册 / 无障碍树 / CLS / llms.txt）
- **WebMCP polyfill 试验**（默认关闭）：捕获网站 WebMCP 工具声明，只捕获不伪造能力

### 隐私与安全

- **广告拦截 2.0**：引擎级内容过滤（`WebKitUserContentFilterStore`，手写 FFI 对接），75 条内置网络层规则 + 自定义关键词，规则变更免重启即时生效
- **反指纹 2.0**：加噪 / 统一化双模式 + 指纹自测面板（"信任需要证明"）
- **GPC**：`Sec-GPC: 1` 请求头 + `/.well-known/gpc.json` 站点声明感知
- **零遥测审计**：全量网络出口清单，可验证"无外联"
- **密码库**：AES-256-GCM 字段级加密 + 主密码，主密钥独立文件（权限 600，损坏绝不静默重建），settings 敏感键前端不可见 / 掩码
- **数据保护**：数据目录 700 / 数据库文件 600，settings 读写白名单，CSP 全面配置
- **本地账号**：PBKDF2-SHA512（21 万迭代，旧账号验证成功后自动透明升级）
- **AI 调用 SSRF 防护**：仅允许 https、拒绝内网 / 本机地址

### 可观测性

- **自检（selfcheck）**：17 项只读体检（版本环境 / 数据存储 / 运行时 / 能力状态），设置页一键查看，启动自动执行
- **结构化日志**：tauri-plugin-log，分级 + 模块 + 时间戳
- **E2E 冒烟**：`scripts/smoke.sh` 12 项关键链路检查，防架构解耦回归

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

## 架构

```
src/                    # Vue 3 前端
├── App.vue             # 主布局：标签栏 / 地址栏 / 书签栏 / 内容区 / 状态栏
├── components/         # TabBar、AddressBar、AISidebar、各管理弹窗
├── stores/             # Pinia：tabs / bookmarks / ai / workspaces / mcp 等
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
│   └── llmsxtxt.rs     #   llms.txt 检测（Agentic Browsing 信号）
├── db/                 # sqlx 数据层：bookmarks / history / passwords / sessions / mcp …
├── tabs.rs             # TabManager：多 webview 生命周期 + WebKit 信号回调
├── ai.rs               # AI 命令：对话 / 流式 / function calling 工具循环（≤6 轮）
├── extensions.rs       # 用户脚本 / 样式注入（UserContentManager）
├── feed.rs             # RSS 聚合（62 源，16 路并发，4MB 响应上限）
├── reader.rs           # 阅读模式正文提取 + ammonia 净化
├── translate.rs        # 翻译（选中文本 / 整页分批）
├── gpc.rs / audit.rs   # GPC 请求头 / 零遥测审计
├── selfcheck.rs        # 只读自检
└── migrations/         # SQLite 迁移（版本 + 事务 + 记录表）
```

关键设计决策：

- **不用前端 SQL**：SQLite 由 Rust 侧 sqlx 统一管理，前端经 `invoke()` 调命令，杜绝注入面
- **引擎抽象层**：WebKitGTK 调用收敛进 `engine/`，业务模块不再直接引 webkit2gtk（耦合度 tabs.rs 25 → 1）
- **多 webview 布局**：主 webview（Vue UI）+ 标签 webview 经 GtkFixed 绝对定位叠层，resize / 书签栏显隐 / AI 分屏由 `ContentGeometry` 一处算清
- **Agent 安全铁律**：动作白名单、每步人工确认、仅当前标签、凭据永不外泄

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

### 测试与检查

```bash
npm run lint                     # ESLint
npm run build                    # 含 vue-tsc 类型检查
cargo test                       # Rust 单测（108 个）
scripts/smoke.sh                 # E2E 冒烟（12 项，完整模式会重启浏览器）
scripts/smoke.sh --no-restart    # 只检查当前实例
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
| 2 · 隐私主权 | 反指纹 2.0、GPC、零遥测审计、密码库主密码、E2E 加密同步（服务器零知识） | ✅（Passkeys 待运行时实测） |
| 3 · 交互形态与平台化 | 分屏、**垂直标签 ✅**、**命令栏（Ctrl+K）✅**、空间、真扩展体系 | 进行中 |
| 4 · 开放生态 | 开放 API、多端协同、自托管服务端套件 | 规划 |

## 文档

| 文档 | 说明 |
|---|---|
| [HANDOFF.md](HANDOFF.md) | 工程交接：逐轮交付记录、踩坑与根因分析、验证方法 |
| [docs/strategy-2026.md](docs/strategy-2026.md) | 战略规划：定位 / 五大支柱 / 分阶段路线图 / 风险治理 |
| [docs/spike-engine-capabilities-2026-09-19.md](docs/spike-engine-capabilities-2026-09-19.md) | 引擎能力实测报告（WebKitGTK 边界） |
| [docs/standard-tracking-2026-09-20.md](docs/standard-tracking-2026-09-20.md) | WebMCP / Agentic Browsing 标准跟踪 |
| [docs/browser-privacy-fingerprint-research-2026.md](docs/browser-privacy-fingerprint-research-2026.md) | 浏览器指纹识别调研 |
| [scripts/](scripts/) | `smoke.sh`（E2E 冒烟）· `backup.sh`（数据库备份） |

## License

个人项目，未采用公开许可证（All Rights Reserved）。

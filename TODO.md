# ShiftBrowser 待办清单

> 更新于 2026-09-23 · 对应 HANDOFF 第八十七章（MCP Server）+ 设置页重构（`002afe1`）
> 优先级：🔴 高 / 🟡 中 / 🟢 低 / ⚪ 遗留

---

## A. 文档滞后（明确技术债，建议先做）

- [x] **A1 · 更新 README 路线图（🔴）**
  路线图里阶段 4 仍标「规划」，实际**已交付**：
  - 开放 API（28 命令，Unix socket + HTTP 双形态，端口可配、局域网开关、eval 闸门、
    只读模式 `api_readonly`、IP 白名单 `api_allowed_ips`）
  - 多端协同数据层（密码库 / 书签 / 会话 / 历史 的 E2E 加密同步）
  - 自托管服务端套件四块：同步 / 工具网关（MCP `tools`+`tool_call`）/ 管理页状态面板 / 模型网关
  → 状态应改为 ✅（或「进行中：剩多端形态」）。参见 HANDOFF 第 70–83 章。

- [x] **A2 · README 特性列表补「系统托盘」（🔴）**
  关闭窗口 = 缩托盘（不退出）· 托盘左键单击显隐窗口 ·
  菜单四项：显示/隐藏窗口 · 下载管理 · 密码库 · 退出。
  参见 HANDOFF 第八十二章（`f69d6ab` / `c583b24`）。

- [x] **A3 · README 补下载落盘目录修复（🔴）**
  下载目的地改用系统 XDG 下载目录（`glib::user_special_dir`），
  修正中文系统「文件落 `~/下载`、应用却在 `~/Downloads` 找」的不一致。
  参见 HANDOFF 第八十一章（`16c8f10`）。

---

## B. 实战检验（高哥试用，二狗配合修）

- [x] **B1 · 托盘交互实测（✅ 高哥实测通过，2026-09-21）**
  - [x] 点托盘菜单「下载管理」「密码库」→ 真弹出对应面板
  - [x] 点托盘图标 / 菜单「显示/隐藏窗口」→ 窗口正常显隐
  - [x] 点窗口右上角 X → 缩托盘（应用不退出）
  - [x] 点托盘菜单「退出」→ 真退出
  → 六项全部正常，托盘功能验收通过。

- [x] **B2 · deb 打包安装闭环（✅ 2026-09-21）**
  一键打包脚本 `scripts/package-deb.sh` 固化（校验工具链 → 构建扩展库 → 构建前端 →
  release deb 全自动），`sudo dpkg -i` 升级 4.4.0 → 5.6.0 真机验证通过。
  详见 HANDOFF 第八十四章（`b708ede` / `df5468b`）。

- [x] **B3 · 默认浏览器设置（✅ 2026-09-21）**
  设置页「通用」新增默认浏览器区块（状态显示 + 一键设为默认）；desktop 文件补齐
  MimeType 声明 + `%U`；外部 URL 启动自动开新标签。真机验证 xdg 链路通。
  详见 HANDOFF 第八十五章（`21c0f3f`）。**待重新打包安装后由高哥实测。**

- [x] **B4 · 登录状态持久化 + 单实例（✅ 2026-09-21）**
  高哥实测反馈两问题：① 登录态重启丢失 → Cookie 持久化落盘（cookies.sqlite）；
  ② 外部链接重复开进程 → 单实例插件转发 URL。真机验证均通过。
  详见 HANDOFF 第八十六章（`9ce60c0`）。**登录态保留最终确认需高哥重启实测。**

- [ ] **B5 · 其它常用场景试用（🟡）**
  高哥接着试，报问题当场诊断修复。

---

## C. 阶段 4 剩余（路线图规划项）

- [ ] **C1 · 多端形态（🟢）**
  移动 / 平板协同（工程量最大，最远）。

- [x] **C2 · 开放 API 扩展（✅ 2026-09-21）**
  新增 12 命令（8 只读 + 4 操作）+ 鉴权细化（只读模式 `api_readonly` / IP 白名单 `api_allowed_ips`）。
  命令总数 16 → 28。详见 HANDOFF 第八十三章。

- [x] **C3 · MCP Server——AI 标准协议驱动浏览器（✅ 2026-09-21）**
  `shift-browser-mcp` 二进制（MCP 2025-03-26 stdio），25 个工具（13 读 + 12 写）与 API
  命令同名同参，直连 Unix socket 零依赖；eval/tool_call 不暴露。任何 MCP 客户端配一行即用。
  详见 HANDOFF 第八十七章。**待高哥在 opencode/其它 MCP 客户端接入实测。**

---

## D. 遗留疑点

- [ ] **D1 · Passkeys（⚪）**
  运行时实测结论：引擎未在 JS 层暴露 WebAuthn（`PublicKeyCredential=false`），
  已降级为「密码管理器增强」。待 WebKitGTK 支持后方可重启。

- [x] **D2 · 「应用自动退出」疑点（已销案）**
  确认是托盘「退出」菜单被点击，属正常行为。

- [x] **D3 · 默认浏览器单实例（✅ 2026-09-21）**
  加 tauri-plugin-single-instance（首个插件），已有实例运行时新实例退出、
  URL 转发给已有实例开新标签 + 窗口前置。真机验证进程数保持 1。
  详见 HANDOFF 第八十六章（`9ce60c0`）。

---

## E. 已闭环（记录）

- [x] HANDOFF 逐章记录补至第 86 章
- [x] 工作区干净（0 未提交）
- [x] 下载落盘目录修复（`16c8f10`）
- [x] 系统托盘（`f69d6ab` + 图标 `c583b24`）
- [x] 书签搜索 LIKE 通配符转义（与 search_history 对齐，`ESCAPE '\'`，修 `%`/`_` 误匹配）
- [x] 默认浏览器设置（`21c0f3f`：xdg-settings 集成 + MimeType 声明 + 外部 URL 打开）
- [x] 登录状态持久化 + 单实例（`9ce60c0`：Cookie 落盘 + single-instance 转发 URL）
- [x] MCP Server（`cdfdc97`：`shift-browser-mcp` stdio 二进制，25 工具，AI 标准协议驱动浏览器）

---

## F. 设置页重构（2026-09-23，`002afe1`）

- [x] **F1 · 隐私选项卡拆分（✅ 2026-09-23）**
  原先一个「隐私」选项卡扛 5 组内容太重，拆成三个独立选项卡：
  - **隐私防护**（浏览防护 + 网络与追踪）
  - **数据安全**（数据安全 + 数据管理）
  - **开放 API**（开放 API 配置）
  导航由 7 项扩为 10 项：通用 / 外观 / AI 助手 / MCP 工具 / 扩展 /
  隐私防护 / 数据安全 / 开放 API / 自检 / 关于。

- [x] **F2 · 全局字迹/对比度修复（✅ 2026-09-23）**
  高哥反馈设置页「字迹不清晰」，根因：`.field-hint` 用了 5 处但零样式定义、
  无全局 placeholder 样式、`--border-subtle` 透明度 0.06 太淡、`--text-muted`
  对比度不足、select 箭头 #999 太暗。修复：
  - App.vue 主题变量提亮（深/浅两套：text-secondary/muted、border-color/subtle）
  - 全局 `::placeholder` 样式
  - 新增 `.field-hint` 提示块样式、MCP/AI 输入框边框 + focus 高亮、
    select 箭头提亮、`.mini-btn` 普通/primary/danger 三态对比度加强

- [x] **F3 · 新增「关于」选项卡（✅ 2026-09-23）**
  导航最后一项（自检下面），4 卡片：产品信息（版本/WebKitGTK/OS/架构，动态读
  不写死）+ 核心能力 + 数据与隐私承诺 + 项目与致谢（打开仓库/数据目录按钮）。
  新增 Rust 只读命令 `app_about_cmd`（`about.rs`）+ AppIcon `info` 图标。

- [x] **F4 · 打包修复（✅ 2026-09-23，`afd5d8e`）**
  Cargo.toml 显式 `default-run = "shift-browser"`，避免 tauri bundle
  因双 bin（主浏览器 + shift-browser-mcp）报 "failed to find main binary"。

- [x] **F5 · 提交推送（✅ 2026-09-23）**
  两个提交推 gitcode master（`6c06928..afd5d8e`），Git Hooks 检查通过。

---

## G. 能力与常量统一治理（2026-09-23 规划）

> 方案全文：[docs/架构-能力与常量统一治理方案_2026-09-23.md](docs/架构-能力与常量统一治理方案_2026-09-23.md)
> 目标：把散落的**常量 / 技能 / 工具 / 插件**提取为**单一事实来源（SSOT）**。
> 统一手段：**Rust 侧单一来源 → `build.rs` 生成前端契约层**（消除"注释写'与后端一致'"的人肉同步）。

### P1 · 常量 + 契约生成管线

- [x] **P1-a · 常量中心 + 生成管线（✅ 2026-09-23）**
  - 新建 `src-tauri/src/constants.rs`（自包含设计，供 build.rs `include!`）：
    布局 11 个 / 搜索引擎 3 个 / 色板 3 套 / favicon
  - `build.rs` 生成 `src/constants.ts` → 前后端常量零漂移
  - 迁移收敛：布局常量（`layout.rs` re-export 保持调用路径不变）、
    搜索引擎 URL **4 处 → 1 处**、favicon **2 处 → 1 处**（**统一地球图标**）、
    色板 **3 套 → 1 处**
  - CSS 变量注入：`App.vue` 注入 `--sb-*-h`，
    `TabBar`/`AddressBar`/`BookmarksBar`/`StatusBar` 改用 `var()`
  - 验证：cargo check/clippy 0 error、118 测试过、vue-tsc/eslint/vite build 过

- [ ] **P1-b · 契约生成（🟡 进行中）**
  - [x] **设置键契约（✅ 2026-09-23，`9470872`）**
    `contracts.rs::settings` 单一来源（41 键 + 默认值 + 类型 + 敏感标记）→
    `build.rs` 生成 `SETTINGS_SCHEMA`/`DEFAULT_SETTINGS`；
    后端 `db/settings.rs` 删除手写 `ALLOWED_KEYS`（40 行）改委托；
    前端 `settings.ts` 删除手写默认值（24 键）→ 用生成物（41 键），**补齐 17 个缺失键**
  - [x] **事件名 + 内部 URL 契约（✅ 生成完成，前端迁移待做）**
    `contracts.rs::events`（18 个 IPC 事件名）+ `contracts.rs::urls`（shift:// 路由）→
    `build.rs` 生成 `EVENTS` / `URLS`
  - [x] **前端 listen + 后端 emit 迁移（✅ 2026-09-23）**
    前端 19 处 `listen('xxx')` → `listen(EVENTS.XXX)`（App.vue/StatusBar/AISidebar/
    TranslateProgress/DownloadsDialog/tabs.ts 共 6 文件）；
    后端 11 处 `emit("xxx")` → `crate::contracts::events::XXX`（9 个 rs 文件）。
    事件名不再是裸字符串，拼错类型检查/编译期即暴露
  - [x] **内部 URL + 超时契约（✅ 2026-09-23）**
    `contracts.rs::urls` 补 `WORLD_NAV_FILE`/`READER_MARKER` → 生成 `URLS`（5 常量）；
    内部 URL 迁移（`api.rs` 的 `shift://newtab`、`commands.rs`/`tabs.rs` 的
    `/newtab/`、`/reader/`、`world-nav.html`）；
    `contracts.rs::timeout`（14 项语义命名）→ 生成 `TIMEOUT`；
    超时裸数字替换：mcp(30s)/feed(15s)/content_cmd+agent_cmd(6s)/agent_cmd(120s)/
    translate(120s)/db(10s)/sync(20s)
  - [x] **快捷键表契约定义（✅ 2026-09-23）**
    `contracts.rs::shortcuts`（12 条：id / key / 修饰键 / GDK keyval / 兜底标记 / 描述）
    → 生成前端 `SHORTCUTS`（组合键字符串）
  - [x] **快捷键迁移（✅ 2026-09-23）**
    Rust GTK 兜底表（`tabs.rs` 裸 keycode 0xFFC9/0x69 等 20+ 处）→ 改为遍历
    `contracts::shortcuts::SHORTCUTS` 查表（keyval + 修饰键 + 兜底标记）；
    前端 `App.vue::onGlobalKeydown` 硬编码键判断 → `matchShortcut(e, SHORTCUTS.XXX)`；
    快捷键现在只有一处定义，改键位两端同步
  - [x] **验收（✅ 2026-09-23）**
    设置页各项显示正常（17 个补齐键不再空串失真）；事件链路（托盘菜单/启动页按钮/
    资讯分屏/状态栏网络状态）实测正常

### P2 · 类型契约生成

- [ ] 引入 `ts-rs` 或 `typeshare`：`Tab`/`Bookmark`/`HistoryEntry` 等 Rust struct → TS interface
  - 前端 `src/types/*.ts`（8 个文件）逐步替换为生成物（保留手写 interface 过渡）
  - 验收：字段/可选性与 Rust 一致；vue-tsc 干净

### P3 · 工具注册表（MCP / API / Agent 三源合一）

- [ ] **P3a · 读类能力迁移（约 13 个）**
  - 新建 `capabilities/registry.rs`（`id` / `desc` / `params` schema / `risk` / `handler` 单一声明）
  - MCP `tool_defs()`、API `match cmd`、Agent 动作表改为**从 registry 派生**
  - 验收：MCP `tools/list` 数量不变（25）；API 行为不变（基线样本回放）
- [ ] **P3b · 写类能力迁移（12 个）+ `capabilities/list` + MCP 动态拉取**
  - MCP Server（独立二进制）启动时向浏览器请求 `capabilities/list`，动态构建 `tools/list`
  - 验收：MCP 客户端实测调用；只读闸门行为不变

### P4 · 技能清单

- [ ] `capabilities/skills.rs`：声明式技能（总结页 / 翻译 / 阅读模式 / 解释页面…）
  - 新增 `skills/list` API；前端快捷操作改为**数据驱动**（不再硬编码在 `ai.ts`）
  - 注入 AI system prompt 的"能力自述"（AI 知道自己能做什么）
  - 验收：AI 面板快捷按钮正常；prompt 含能力清单

### P5 · 插件体系

- [ ] `capabilities/plugins.rs`：统一四形态（用户脚本 / 内容过滤 / Web 扩展 / MCP server）注册
  - 统一启停 `plugins::set_enabled`、统一审计（复用 `tool_audit_log`）、统一发现 `plugins/list`
  - 现有四套注册逻辑各包一层 `Plugin` 适配器（底层实现不动）
  - 验收：设置页扩展列表；启停生效；审计有记录
# ShiftBrowser 架构文档（ARCH.md）

> 建于 2026-10-15（F-11）。**2026-10-31 重写技术栈与形态两节**：CEF 换核后原来的
> "Tauri + WebKitGTK 单窗口多 webview" 已不是主线，文档滞后了一个月，与发布物完全相反。
> 来源：仓库源码实读（非臆测）；行号会漂移，以文件/模块名为锚。
> 配套文档：README.md（功能与使用）、CHANGELOG.md（变更台账）、
> `docs/工程全面分析_2026-10-31.md`（质量门与产品缺口现状）、docs/（专项方案）。

## 一、技术栈与总览

| 层 | 技术 |
|---|---|
| 桌面壳 | Tauri v2（Linux 主平台）+ **CEF（Chromium 154）单栈窗口** |
| 备用引擎 | WebKitGTK（`--features cef-engine` 关闭时的回退构建，功能对等） |
| 后端 | Rust（sqlx 0.8 + SQLite，WAL；tokio 异步） |
| 前端 | Vue 3 `<script setup>` + TypeScript + Pinia + vue-i18n + vitest |
| 打包 | deb / appimage（package-deb.sh）；CEF 构建另有 scripts/install-cef.sh |

**双轨构建（编译期，不是运行期切换）**：

| 构建 | 命令 | 形态 |
|---|---|---|
| CEF（主线，2026-10-27 高哥拍板） | `cargo build --release --features cef-engine` | CEF 自己创建的顶层窗口，自带工具栏/标签栏/内容区（`engine/cef/shell.rs`）；Tauri 主窗口**不创建** |
| WebKit（回退） | `cargo build --release` | Tauri + GTK/WebKitGTK，单窗口多 webview |

CEF 路径的界面是 `engine/cef/toolbar.rs` 里的单字符串 HTML/CSS/JS，跑在独立的
`BrowserView` 上；动作经 `console.log("SHIFT_ACT:...")` 回到 Rust 侧
`shell.rs::handle_action`（99 个动作分支 + 13 个面板注册表）。
**这条路径不经过 Vue**，所以 `AISidebar.vue` / `TranslatePanel.vue` 只在 WebKit 回退构建里生效。

WebKit 路径的形态：每个标签一个 child webview（`tab-<id>`），Rust 侧 `TabManager`
（内存态）为唯一事实来源，前端经事件 + invoke 同步。
两条路径共用 SQLite 落盘（`app_data_dir/shift-browser.db`，0600，目录 0700）。

## 二、目录地图

```
src/                     前端（Vue）
  App.vue                主窗口布局 + 全局快捷键分发（keydown → handleGlobalAction）
  components/            UI 组件（TabBar/AddressBar/BookmarksBar/各 Dialog/CommandPalette…）
  components/ui/         通用 UI 底座（UiDialog 基于 Reka）
  stores/                Pinia 状态（tabs/settings/bookmarks/ai/shield…）
  composables/           组合式函数（useConfirmDelete 二次确认等）
  utils/                 工具（webview 可见性、shortcut-guard 弹窗守卫、bookmarks 树）
  constants.ts           build.rs 由 Rust contracts.rs 生成（LAYOUT/EVENTS/SHORTCUTS 单一来源）
  proto/                 shift:// 协议注入脚本（页面内 IPC 桥，含整页翻译）

src-tauri/src/           后端（Rust）
  lib.rs                 入口：command 注册表 + setup
  contracts.rs           契约唯一来源：设置键白名单/事件名/快捷键/常量（build.rs 生成前端侧）
  db/                    SQLite 层（迁移在 db/mod.rs run_migrations，文件在 src-tauri/migrations/）
    bookmarks.rs passwords.rs settings.rs sync.rs translate_memory.rs …
    password_crypto.rs   主密钥文件（master.key，600）+ AES-256-GCM 原语
    vault.rs             密码库主密码（vault.key 存在 = 已设锁；解锁会话持密钥）
  tabs/                  标签域
    model.rs             TabManager 内存态 + now_ts 等
    backend/create/      建签子步骤（url.rs 协议转换、signals.rs GTK 信号接线）
    backend/layout.rs    GTK 布局与 webview 挂载
    commands/            Tauri command（lifecycle/nav_cmds/tab_state_cmds…）
    util.rs              clean_tracking_params / sanitize_filename
  engine/                引擎抽象与实现
    webview.rs           WebKit 路径的 eval/快照封装（回退构建用）
    content.rs           SNAPSHOT_JS 注入 + 语义快照（签名绑定 webkit2gtk::WebView，CEF 侧待抽象，见分析报告 P1-5）
    mcp/ MCP client      MCP 工具桥
    cef/                  CEF 引擎（`--features cef-engine` 才编译，14230 行）
      shell.rs           单栈窗口 + 99 个 SHIFT_ACT 动作分发 + 13 个面板注册表（LINES-EXEMPT）
      toolbar.rs         工具栏 HTML/CSS/JS 单字符串内嵌页（LINES-EXEMPT）
      browser.rs         单标签 Browser 封装（388 行，拆出 browser/ 下的 inject/events/ops）
      newtab.rs          新标签页（HTML 在 newtab_page.html，include_str! 引入）
      *_ui.rs            各面板后端（书签/密码/历史/下载/证书/用户脚本/会话/分组/命令面板）
      app.rs             CefApp 初始化与 CEF 消息循环
  extensions/            注入体系：adblock / privacy / userscripts / cef_inject（CEF 侧预计算注入）
  cef_cmd.rs             CEF 的 Tauri 命令 + 子进程入口（拆出 cef_cmd/{cef_boot,toolbar_data}.rs）
  translate.rs webzh.rs  引擎翻译 + AI 整页汉化（翻译记忆查/写在 db/translate_memory.rs）
  api/                   开放 API（本机 HTTP，SSRF 守卫 + 只读闸门 + 审计）
  capabilities/          能力注册表 SSOT（registry/skills/plugins/agent_actions）
  proto_shift.rs         shift:// 自定义协议与注入脚本
  perms.rs               webview 权限请求（挂起 + 白名单 + 通知前端）
  tls.rs net_guard.rs    TLS 错误拦截（危险站点 UI）/ 网络守卫
```

## 三、关键数据流

### 建标签（create_tab）
前端 invoke → `lifecycle_cmds::create_tab` → `validate_create_url`（scheme 白名单）
→ `create::url::resolve_webview_url`（file:// 内部页转 `shift://` 自定义协议，无痕例外）
→ engine 建 child webview → `signals.rs` 接 GTK 信号
（load_failed / TLS 拦截 / 权限挂起 / 崩溃冷却恢复 / 标题地址实时同步）→ 事件 `tabs-updated`。

### 翻译（C-8/C-9/C-11 链路）
整页汉化：`webzh.rs` 收集分批 → `translate_memory` 查缓存（HMAC 查询键，key 在独立
`translate.key`，原文不落明文）→ miss 走 AI/引擎 → 回填 DOM + 写记忆。
引擎单发：`translate.rs::translate_text_cached`（同记忆，`engine:<engine>:<target>` 命名空间）。

### 设置（契约闸门）
前端 invoke → `settings.rs` → 键必须在 `contracts::settings::SETTINGS` 白名单
（敏感键 MASKED/SENSITIVE 分级）→ 落 settings 表。**新增设置键必须进白名单**，
暗键会对真实库做差集审计暴露（F-9 教训）。

## 四、安全体系速览

- **密钥**：`master.key`（密码库，受 vault 主密码保护——设锁后只从解锁会话内存取）、
  `vault.key`（锁存在性标记）、`translate.key`（翻译记忆 HMAC，独立于解锁状态）。
  三者皆 600、create_new 原子创建、损坏绝不静默重生。
- **列级加密**：仅 passwords（AES-256-GCM）；translate_memory 用 HMAC 原文不落明文（C-11）；
  同步包用主密码派生密钥整包 AES-GCM（零知识）。
- **SSRF**：开放 API 读页双查（入口 + 最终 URL），私有地址判定单一实现。
- **导航白名单**：建签/导航均走 scheme 白名单（file 仅内部页）。
- **能力注册表**：MCP 工具 / 开放 API / Agent 动作三方同源（capabilities/registry）。

## 五、迁移与兼容

- 迁移文件 `src-tauri/migrations/YYYYMMDDNNNNNN_*.sql`，注册于 `db/mod.rs::run_migrations`
  （version 递增，`_migrations` 表记录）。**已提交的迁移禁止修改**，修正一律追加新迁移。
- 新增迁移必须带单测（正向 + 幂等/回滚）；设置键/超时/快捷键改动必须跑
  `cargo test --lib ts_export` 重生成前端契约并同步单测期望。

## 六、测试体系

| 层 | 工具 | 现状（2026-10-31 实跑） |
|---|---|---|
| Rust 单测 | `cargo test --lib` | **670 passed / 0 failed / 2 ignored**（110s） |
| Rust 单测（CEF） | `cargo test --lib --features cef-engine` | **726 passed / 0 failed / 2 ignored**（含 CEF 模块 56 例） |
| Rust 关口 | `cargo clippy --all-targets -- -D warnings` | 0 警告；加 `--features cef-engine` 同样 0 警告 |
| 格式 | `cargo fmt --check` | 0 diff（2026-10-31 全量格式化后） |
| 前端组件/store | vitest（jsdom） | **902 例 / 86 文件全过**（约 55s） |
| 前端类型/lint | vue-tsc / eslint | 0 错 |
| 前端构建 | vite build + 首屏体积卡 | 通过（index 184 kB，豁免清单内） |
| CEF 冒烟 | `scripts/smoke-cef.sh` | 2026-10-31 新增（检查项见脚本头注释） |
| 真机 E2E | playwright MCP（e2e-verify 技能） | 按批次做，无常驻 |

**数字口径**：以上均为本机实跑结果，不引用历史文档里的旧数字（ARCH 旧版写 460/506，
已作废）。改动后以 `cargo test --lib` 与 `npx vitest run` 的实际输出为准。

手法惯例：**核心逻辑抽纯函数使其可单测**（如 nav_scheme_error、crash_reload_allowed、
file_internal_page），命令层/信号层薄壳依赖 AppHandle+GTK 的不硬测。

## 七、已知取舍（诚实记录）

- 单用户桌面应用假设：DB 明文 + OS 权限 0600 是基准防护，全库加密依赖全盘加密（运维层）。
- 无痕标签的内部页走 file:// 直载（ephemeral context 无 shift:// 协议），代价是无痕下 IPC 桥不可用。
- 崩溃冷却 30s 硬编码于 signals.rs（CRASH_COOLDOWN_SECS），防崩溃循环优先于自动恢复率。

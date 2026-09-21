# ShiftBrowser v5.0.0（Tauri 重构）交接文档

> 最后更新：2026-09-21（逐章记录补至第 57 章：阶段 1.5–2 收尾、阶段 3 交互形态/平台化、扩展体系网络层拦截/头改写全链路）
> 交接人：王二狗 → 下任 agent

---

## 一、项目概况

**目标**：将 Electron 版 ShiftBrowser（v4.5.0，`/home/gao/ShiftBrowser/`）重构为 **Tauri 2.0 + Rust + Vue 3 + Tailwind 4 + Pinia + TypeScript + SQLite(sqlx)**。

**重构项目位置**：`/home/gao/shift-browser-tauri/`（独立目录，和 Electron 版分开）

**路线图**：`/home/gao/ShiftBrowser/doc/UPGRADE-ROADMAP.md`（v2.0，含 Phase 0-4 完整规划）

**当前进度**：**阶段 0–2 完成 + 阶段 3 进行中**（详见文末逐章记录）。
- 阶段 0 基座加固 ✅、阶段 1 AI 原生内核 ✅（持续增强）、阶段 2 隐私主权 ✅（Passkeys 待运行时实测）；
- 阶段 3 交互形态与平台化：命令栏 / 垂直标签 / 分屏 / 空间 ✅，真扩展体系（网络层拦截）已出原型。

---

## 二、已完成的工作

### 2.1 项目脚手架 ✅

| 文件 | 说明 | 状态 |
|------|------|------|
| `package.json` | v5.0.0-alpha.1，含 vue/pinia/vite/tailwindcss 依赖 | ⚠️ `@tauri-apps/plugin-sql` 残留未删 |
| `vite.config.ts` | 端口 5173，host 0.0.0.0，忽略 src-tauri 目录 | ✅ |
| `index.html` | 入口 HTML | ✅ |
| `tsconfig.json` | ES2022 + strict，别名 `@/*` → `src/*` | ✅ |
| `src/main.ts` | createApp + createPinia | ✅ |
| `src/styles/main.css` | Tailwind 4 + CSS 变量（tab/address/bookmark 高度） | ✅ |
| `src/vite-env.d.ts` | Vue 模块声明 | ✅ |

### 2.2 Tauri 后端（Rust） ✅

| 文件 | 说明 | 状态 |
|------|------|------|
| `src-tauri/Cargo.toml` | tauri 2.11.3 + sqlx 0.8 (sqlite) + tokio + 插件 | ✅ |
| `src-tauri/tauri.conf.json` | 1280x800, identifier `com.shiftbrowser.app`, devUrl localhost:5173 | ✅ |
| `src-tauri/src/main.rs` | `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` + 调用 run() | ✅ |
| `src-tauri/src/lib.rs` | 注册插件 + 初始化 SQLite + 注册 21 个 Tauri 命令 | ✅ |
| `src-tauri/build.rs` | 标准 `tauri_build::build()` | ✅ |
| `src-tauri/capabilities/default.json` | core/opener/shell/dialog/fs 权限（已移除 sql 权限） | ✅ |
| `src-tauri/icons/` | 完整图标集（32x32 → icon.icns/ico） | ✅ |

### 2.3 数据库层（Rust sqlx） ✅

| 文件 | 说明 | 状态 |
|------|------|------|
| `src-tauri/migrations/20260905000000_init.sql` | 10 张表：bookmark_folders, bookmarks, history, settings, passwords, reading_list, downloads, tab_groups, workspaces, sessions, extensions | ✅ |
| `src-tauri/src/db/mod.rs` | Db 结构体（连接池） + init() + run_migrations() + DbError 类型 | ✅ |
| `src-tauri/src/db/settings.rs` | 5 个命令：get/set/delete/get_all/bulk | ✅ |
| `src-tauri/src/db/bookmarks.rs` | 8 个命令：CRUD + 搜索 + 级联删除 | ✅ |
| `src-tauri/src/db/history.rs` | 7 个命令：记录/搜索/最多访问/清除 | ✅ |

**已注册的 21 个 Tauri 命令**：
```
设置：get_setting, get_all_settings, set_setting, delete_setting, set_settings_bulk
书签：get_bookmarks, get_bookmark_folders, create_bookmark, create_bookmark_folder,
      update_bookmark, delete_bookmark, delete_bookmark_folder, search_bookmarks
历史：record_history, get_recent_history, search_history, get_most_visited,
      clear_history, clear_history_before, delete_history_entry
```

### 2.4 前端骨架 ✅

`src/App.vue`：用 `invoke()` 调用 Rust 命令测试数据库（set_setting → get_setting → create_bookmark → get_bookmarks），显示测试结果。

### 2.5 编译验证 ✅

- `cargo check` 通过（零错误零警告）
- `cargo run` 编译 581 个 crate，WebKit 渲染进程正常启动
- 当前有运行中的进程（vite + shift-browser 二进制）

---

## 三、未完成 / 待处理

### 3.1 立即需要做的 ✅ 已完成

1. **清理 `package.json`**：删除 `"@tauri-apps/plugin-sql": "^2.4.1"` 行（后端已不用这个插件）✅
2. **验证数据库**：杀旧进程 → 重启 → 检查 `~/.local/share/com.shiftbrowser.app/shift-browser.db` 是否自动创建 ✅（已创建，迁移 v1 完成）
3. **验证命令调用**：App.vue 里的 invoke 测试是否正常返回 ✅（前端显示"✅ 数据库命令正常"）

### 3.2 Phase 1 任务（核心浏览器框架）

#### Rust 后端
- `src-tauri/src/db/tabs.rs`：标签管理命令（create/close/switch/move/reorder）
- `src-tauri/src/db/tab_groups.rs`：标签分组命令
- `src-tauri/src/db/workspaces.rs`：工作区命令
- `src-tauri/src/db/sessions.rs`：会话快照命令
- `src-tauri/src/db/downloads.rs`：下载管理命令
- `src-tauri/src/db/passwords.rs`：密码管理（AES-256-GCM 加密）
- 更新 `lib.rs` 注册新命令

#### Vue 前端
- `src/components/TabBar.vue`：标签栏组件（标签切换/关闭/拖拽排序/新建）
- `src/components/AddressBar.vue`：地址栏组件（URL 输入/搜索/后退前进刷新）
- `src/components/BookmarkBar.vue`：书签栏组件
- `src/stores/tabs.ts`：Pinia 标签状态管理
- `src/stores/bookmarks.ts`：Pinia 书签状态管理
- `src/views/MainWindow.vue`：主窗口布局
- **集成 WebView**：Tauri 2 的 `WebView` 标签嵌入网页（这是最关键的一步）

#### WebView 集成要点（技术方案已确定 ✅）

**推荐方案：混合方案（Vue UI + 嵌入式多 webview）**

Tauri 2.0+ 的 `Window::add_child(WebviewBuilder)` 支持在同一窗口内嵌入多个 webview，通过 `hide()/show()` 切换显隐，实现标签页效果。

架构：
```
┌─────────────────────────────────────────┐
│  TabBar (Vue)     | AddressBar (Vue)    │ ← Vue 渲染的 UI
├─────────────────────────────────────────┤
│                                          │
│  WebView (外部网页)                       │ ← webview 嵌入区域
│  每个标签页一个 webview，用 hide/show 切换  │
│                                          │
└─────────────────────────────────────────┘
```

Rust 侧 API：
```rust
// 创建嵌入式 webview
window.add_child(
    WebviewBuilder::new("tab-1", WebviewUrl::External(url.parse().unwrap()))
        .auto_resize(),
    LogicalPosition::new(0., TAB_BAR_HEIGHT),
    LogicalSize::new(width, height),
);
```

JS 侧 API：
```javascript
const webview = new Webview(appWindow, 'tab-1', {
    url: 'https://example.com',
    x: 0, y: 40, width: 1280, height: 760,
});
await webview.hide(); // 隐藏
await webview.show(); // 显示
await webview.setFocus(); // 聚焦
```

前端事件通信：webview URL/标题变化通过 `webview.emit()` 通知主窗口。

参考资料：
- https://v2.tauri.app/reference/javascript/api/namespacewebview/
- https://github.com/tauri-apps/tauri/issues/10011（多 webview 示例代码）

---

## 四、技术栈明细

| 组件 | 版本 | 说明 |
|------|------|------|
| Tauri | 2.11.3 | 桌面框架 |
| Rust | 1.97.1 | 后端语言 |
| Vue | 3.5.42 | 前端框架 |
| Pinia | 4.0.3 | 状态管理 |
| Vite | 8.3.0 | 构建工具 |
| TypeScript | 7.0.2 | 类型系统 |
| Tailwind CSS | 4.3.3 | 样式框架 |
| sqlx | 0.8 | SQLite 异步驱动 |
| tauri-plugin-log | 2 | 日志 |
| tauri-plugin-opener | 2 | URL/文件打开 |
| tauri-plugin-shell | 2 | shell 命令 |
| tauri-plugin-dialog | 2 | 对话框 |
| tauri-plugin-fs | 2 | 文件系统 |

---

## 五、开发环境

### 运行中的进程（交接时）
- Vite dev server：PID 129659（端口 5173）
- shift-browser 二进制：PID 130008
- **重启前需杀掉这些进程**

### 启动方式

**开发模式**（在 `/home/gao/shift-browser-tauri/src-tauri/` 目录下）：
```bash
# 方法 1：Tauri CLI（会自动启动 vite + cargo）
cd /home/gao/shift-browser-tauri/src-tauri
cargo tauri dev

# 方法 2：分开启动
# 终端 1：Vite
cd /home/gao/shift-browser-tauri && setsid nohup npm run dev > /tmp/vite-dev.log 2>&1 &

# 终端 2：Rust（debug 模式连 devUrl localhost:5173）
cd /home/gao/shift-browser-tauri/src-tauri
setsid nohup env XDG_RUNTIME_DIR=/run/user/1000 DISPLAY=:0 WAYLAND_DISPLAY=wayland-0 GDK_BACKEND=wayland cargo run > /tmp/shift-run.log 2>&1 &
```

### 关键路径

| 路径 | 说明 |
|------|------|
| `/home/gao/shift-browser-tauri/` | Tauri 重构项目根目录 |
| `/home/gao/shift-browser-tauri/src-tauri/` | Rust 后端 |
| `/home/gao/shift-browser-tauri/src/` | Vue 前端 |
| `~/.local/share/com.shiftbrowser.app/shift-browser.db` | SQLite 数据库文件（运行时自动创建） |
| `/home/gao/ShiftBrowser/` | 旧 Electron 版（保留参考，v4.5.0） |
| `/home/gao/ShiftBrowser/doc/UPGRADE-ROADMAP.md` | 重构路线图 |

---

## 六、关键设计决策

1. **不用 tauri-plugin-sql**：前端直连 SQL 不安全。改用 sqlx 在 Rust 侧管理 SQLite，前端通过 `invoke()` 调用 Tauri 命令。
2. **迁移系统自建**：不用 sqlx-migrate 宏，用 `include_str!` + `_migrations` 表手动管理版本，简单可控。
3. **标签页管理**：Tauri 不像 Electron 能轻松嵌入多个 webview。每个标签页可能需要独立的 `WebviewWindow`，用 Tauri 多窗口 API 管理。**这是 Phase 1 最大的技术挑战**。
4. **Wayland 环境**：高哥的系统用 Wayland，需要 `GDK_BACKEND=wayland` 环境变量。截图工具受限（import/gnome-screenshot 不可用）。
5. **sudo 密码**：不写入文档（安全铁律）。凭据在权限 600 的本地文件，需要时问二狗/高哥本人。

---

## 七、问题记录与排查（数据库连接失败）✅ 已解决

### 7.1 现象

新会话启动后，前端 invoke 调用数据库命令失败，`/tmp/shift-run.log` 显示：

```
[ShiftBrowser] 数据库初始化失败: error returned from database: (code: 14) unable to open database file
```

SQLite 错误码 14 = 无法打开数据库文件。`shift-browser.db` 未创建。

### 7.2 已确认的事实

- `~/.local/share/com.shiftbrowser.app/` 目录**存在**（WebKit 缓存文件在里面，说明 app_data_dir 解析正常）
- 但目录下**没有** `shift-browser.db`
- `Db::init()` 里 `std::fs::create_dir_all(&app_data_dir)?` 没报错（否则错误信息不同）
- 错误发生在 `SqlitePoolOptions::connect(&db_url)` 这一步

### 7.3 根因（最终确认）

**sqlx 的 `SqliteConnectOptions::create_if_missing` 默认是 `false`！**

- sqlx 默认 flags = `SQLITE_OPEN_READWRITE`（**没有** `SQLITE_OPEN_CREATE`）
- 用没有 CREATE 的 flags 打开**不存在的文件**时，SQLite 直接返回 `SQLITE_CANTOPEN` (14)
- 排查时用原生 `libsqlite3-sys` 测试成功，是因为测试代码里手动加了 `SQLITE_OPEN_CREATE`，而 sqlx 默认没有
- 内存数据库（`sqlite::memory:`）不受影响，所以内存库测试通过、文件库全挂

### 7.4 修复（已写入代码）

`src-tauri/src/db/mod.rs` 的 `Db::init()` 改为：

```rust
let options = SqliteConnectOptions::new()
  .filename(&db_path)
  .create_if_missing(true)   // ← 关键！默认是 false
  .foreign_keys(true)
  .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal);

let pool = SqlitePoolOptions::new()
  .max_connections(5)
  .connect_with(options)
  .await?;
```

### 7.5 验证结果 ✅

```
[ShiftBrowser] 迁移 v1 init_core_tables 完成
[ShiftBrowser] 数据库已初始化: /home/gao/.local/share/com.shiftbrowser.app/shift-browser.db
[ShiftBrowser] 数据库初始化成功
```

前端显示"✅ 数据库命令正常"，设置/书签读写全部通过。

### 7.6 经验教训

1. **sqlx 的 `create_if_missing` 默认是 false**，新建数据库必须显式加 `.create_if_missing(true)`，或 URL 带 `?mode=rwc`
2. **排查依赖库问题时，对比测试必须逐项核对默认值**，不能想当然认为"默认应该会创建文件"
3. 用 `pkill -f "关键字"` / `grep "关键字"` 杀进程时，**命令行里含该关键字的进程会误杀自己**，要用 `ps -eo pid,comm | grep -i 关键字`（只看进程名）或 `pgrep -x 精确进程名`

---

## 八、进度记录（Phase 1 多标签核心已跑通 ✅ 2026-09-11 19:30）

### 8.1 已完成

1. **多标签页方案调研完成**：Tauri 2.11.5 `Window::add_child(WebviewBuilder)` + `WebviewUrl::External`，需 `unstable` feature（详见 7.2 技术方案）
2. **Rust 侧 `src-tauri/src/tabs.rs`**：`TabManager` 标签管理器（内存 State，对应 Electron 版 `tabManager.ts`）
   - 6 个命令：`create_tab` / `close_tab` / `switch_tab` / `navigate_tab` / `get_tabs` / `update_tab`
   - webview 位置 `(0, 82)`（40 标签栏 + 42 地址栏），高度扣 22px 状态栏
   - 切换：`hide()/show()/set_focus()`；关闭：`close()`（注意：Webview 没有 `destroy()`）
3. **前端**：`src/types/tab.ts`、`src/stores/tabs.ts`（Pinia）、`src/components/TabBar.vue`、`src/components/AddressBar.vue`、`App.vue` 主布局
4. **端到端实测通过**（xdotool 模拟交互 + 日志验证）：
   - ✅ 窗口创建（需 `GDK_BACKEND=x11`）
   - ✅ 点"+"创建标签 → `create_tab` + webview 创建成功
   - ✅ 点击标签切换 → `switch_tab`（hide/show）
   - ✅ 关闭标签 → `close_tab`
   - ✅ 地址栏输入 URL 回车 → `navigate_tab` + webview 加载百度页面（截图彩色采样验证）
5. **修复的 bug**：
   - UUID 生成 `{:x}` 不补零导致切片越界 panic → 改 `{:08x}`（详见 ERRORS.md）
   - vue-tsc 3.3.11 不兼容 typescript 7 → 降 typescript 到 `^5.9.3`
   - TabBar favicon `@error="$event.target.remove()"` 类型错误 → 移到 script 函数
6. **🔴 白色空白 bug 已修复（2026-09-11 关键突破）**：
   - **根因**：Tauri 在 Linux/GTK 上 `add_child` 的 webview 不走位置参数，被塞进窗口 vbox（GtkBox）用 `pack_start(webview, true, true, 0)` 布局；主 webview + child webview 都 expand=true → GTK 垂直平分窗口，child 落在 y=400 起，中间 310px 是主 webview 残留白色内容区。`set_bounds` 无效（GTK 布局强制覆盖）。
   - **解法**：把主 webview 和所有 child webview 移到 **GtkFixed 容器**（绝对定位叠层）：窗口 vbox → [GtkFixed] → 主 webview(0,0 全屏) + child webview(0,82 内容区)。
   - **实现要点**：
     - gtk 对象非 Send，不能存 State/跨线程 → 所有 gtk 对象在 `with_webview` 闭包内创建/查找，vbox 从 webview 的 `parent()` 拿（避免调 window 方法导致主线程死锁）
     - gtk-rs 0.18 无 `reparent` 方法 → 用 `vbox.remove()` + `fixed.put()` 实现
     - `tabs::setup_gtk_layout(app)` 在 setup 阶段初始化（主线程创建 Fixed + reparent 主 webview）；`fix_gtk_layout()` 处理每个 child
     - Cargo.toml 加 `gtk = { version = "0.18", features = ["v3_24"] }`
   - **验证**：日志 `[tabs] child_webview reparent 到 Fixed: pos=(0,82) size=(1280,696)`；截图采样页面内容从 y≈85 开始（白色空白消失）；多标签创建/切换正常
7. **标题同步轮询已完成**：Rust 侧 `on_page_load` 回调更新 TabManager.title（MVP 用 host 作标题，日志 `page_load_finished: title=cn.bing.com`）；前端 `stores/tabs.ts` 加 `startPolling()`（每 2s 调 `get_tabs`）/`stopPolling()`，`App.vue` onMounted/onUnmounted 启停。**验证**：标签栏 OCR 显示标题 "cn.bing.com" ✓
    - 说明：Tauri 2 内置无 `plugin:event|listen` 命令，`tauri-plugin-event` 不存在于 crates.io → 事件方案走不通，改用轮询
 8. **后退/前进按钮已完成**：Rust 侧 `tabs.rs` 加 `go_back`/`go_forward` 命令（tauri Webview 无直接 back/forward API，用 `webview.eval("history.back()")`/`eval("history.forward()")` 实现），lib.rs 注册；前端 `stores/tabs.ts` 加 `goBack()`/`goForward()`，`AddressBar.vue` 的 ← → 按钮去掉 disabled 绑定点击。**验证**：临时改默认 URL 为 baidu 重启 → 点主页按钮导航到 bing 造历史 → 点后退回到 baidu（`go_back` + `page_load_finished: title=www.baidu.com`）→ 点前进回到 bing（`go_forward` + `page_load_finished: title=cn.bing.com`）✓
 9. **书签栏 + 状态栏已完成**：Rust 侧 `tabs.rs` 加 `BOOKMARKS_BAR_HEIGHT=30`，`CONTENT_TOP_OFFSET = 40+42+30 = 112`（child webview 从 y=112 起高 666）；前端新增 `components/BookmarksBar.vue`（30px，显示书签列表 + ☆ 收藏当前页，点击导航）和 `components/StatusBar.vue`（22px，左侧当前 URL + 右侧标签数/版本），`stores/bookmarks.ts`（get_bookmarks/create_bookmark/delete_bookmark）。**验证**：日志 `child_webview reparent 到 Fixed: pos=(0,112) size=(1280,666)`；截图像素采样确认 5 个区域（标签栏 0-40/地址栏 40-82/书签栏 82-112/内容区 112-778/状态栏 778-800）颜色和位置全对齐 ✓
 10. **db 扩展模块已完成**：新增 `db/sessions.rs`（save_session/load_last_session/list_sessions/delete_session，data 字段存标签 JSON 快照）、`db/downloads.rs`（create_download/update_download_progress/get_downloads/delete_download）、`db/tab_groups.rs`（CRUD）、`db/workspaces.rs`（CRUD）；`db/mod.rs` 声明 + lib.rs 全部注册
 11. **密码加密存储已完成**：`db/passwords.rs` 用 AES-256-GCM（依赖 `aes-gcm 0.10` + `rand 0.8` + `base64 0.22`），主密钥随机 32 字节存 settings 表（key=master_key），每条密码随机 12 字节 nonce + 密文 base64；命令 save_password/get_password/get_passwords_for_domain/delete_password。**验证**：3 个单元测试通过（加解密往返、随机 nonce 密文不同、错误密钥解密失败）
 12. **会话恢复已完成**：前端 `stores/tabs.ts` 加 `restoreLastSession()`（load_last_session → 解析 JSON → 逐个 createTab 恢复）/`saveCurrentSession()`；`App.vue` 启动时无标签则恢复会话（失败才创建默认标签）+ watch 标签变化 3 秒防抖自动保存。**验证**：数据库 sessions 表有自动保存记录（`https://cn.bing.com/`），重启后 `create_tab: url=https://cn.bing.com/`（恢复上次会话）✓
 13. **密码管理 UI 骨架已完成**：Rust 侧 `tabs.rs` 加 `list_passwords` 命令 + `set_webview_visible` 命令；前端新增 `components/PasswordsDialog.vue`（弹窗组件：列表/添加表单/查看密码/删除）+ `stores/passwords.ts`。**关键发现**：child webview 是独立 GTK 部件，用 GtkFixed 绝对定位叠在主 webview（Vue UI）之上，导致 Vue 弹窗被 child 遮挡看不见。**解法**：`set_webview_visible` 命令先 hide 所有标签 webview → 弹窗可见 → 关闭时 show 恢复。验证：点击菜单按钮 `set_webview_visible: visible=false count=1`，截图弹窗面板渲染成功，点击"+ 添加密码"表单展开（像素差异 6057 个采样点）✓
 14. **窗口 resize 同步已完成**：Rust 侧 `setup_gtk_layout` 注册 `on_window_event` 监听 Resized → 更新 Fixed 内主 webview 全屏 + child webview 位置/尺寸（w, h-134）。**验证**：`wmctrl` 最大化到 1920x1011 → 日志 `window resized: 1920x1011 content_h=877` → 截图确认地址栏/标签栏/书签栏铺满 1920 宽、内容区 1920x877、状态栏正常 ✓
 15. **git 仓库已初始化**：`.gitignore` 排除 node_modules/dist/target/learnings/db，首次提交 `7c2df83`（57 个文件）
 16. **target=_blank / window.open 链接已处理**：Rust 侧 `WebviewBuilder::on_new_window` 回调——MVP 方案：在**当前 webview** 导航到链接 URL（`wv.navigate`），返回 `NewWindowResponse::Deny` 不创建新窗口。**关键**：webkit2gtk 的 `javascript_can_open_windows_automatically` 默认 false（JS 不能自动开窗口），需在 `fix_gtk_layout` 里用 `Settings::builder().javascript_can_open_windows_automatically(true).build()` + `WebViewExt::set_settings` 开启（Cargo.toml 加 `webkit2gtk = "=2.0"` 依赖）。**验证**：window.open 触发 `on_new_window: url=https://www.qq.com/` → 当前 webview 导航成功 → `page_load_finished: title=www.qq.com` ✓
 17. **GtkOverlay 布局重构（窗口还原修复）**：原 GtkFixed 方案把主 webview `set_size_request(窗口尺寸)` 导致窗口 natural size 被撑大（1920x1011），WM 无法缩小窗口（最大化后还原失败、无法手工调整大小）。**重构**：窗口 vbox → [GtkOverlay] → 主 webview（主层，自动填满窗口，natural size 固定 1280x800）+ GtkFixed（上层，child webviews 绝对定位，natural size 固定 1280x800）；resize 监听**只更新 child webviews**（主 webview 在 overlay 主层自动跟随窗口）。**验证**：最大化 1920x1011（地址栏铺满 1920 宽、内容区 1920x877）→ 还原 1280x800 ✓ 循环多次正常
 18. **滚动问题排查结论（非 bug）**：Bing 首页 `scrollHeight 666 ≈ 视口 666`（内容刚好一屏，无滚动条正常）；网易等长页面可正常滚动。之前 xdotool 滚轮/PageDown 测试"失效"是**合成事件不被 WebKitGTK 处理**（同合成键盘），真实用户操作正常
 19. **密码管理完整功能已验证（高哥手动）**：弹窗打开 → "+ 添加密码" → 填域名/密码 → 保存成功。验证数据库：passwords 表 1 条（domain=127.0.0.1，密文 44 字符 base64 = AES-256-GCM 非明文），settings 表 master_key 已生成（32 字节）。**修复两个关键问题**：
    - **弹窗不显示**：PasswordsDialog 在 App.vue 里**始终挂载**（无 v-if），`onMounted` 只在应用启动时执行一次 → ⋮ 点击时 child 不隐藏。**改 watch(props.show)**：show 变 true 时隐藏 child + 加载列表，false 时恢复
    - **弹窗显示但点不了**：GtkOverlay 重构后 Fixed 在 overlay 上层**填满整个窗口**，拦截所有点击（地址栏按钮、弹窗按钮都收不到）→ ① Fixed 用 `set_valign(Start)` + `set_margin_top(112)` 定位到内容区（不覆盖地址栏）；② `set_webview_visible(false)` 时**连 Fixed 一起 hide**（弹窗可交互），恢复时 show
    - **验证**：点后退按钮有 `go_back` 日志（点击到达 Vue UI）；点 ⋮ → `visible=false count=3` → 弹窗遮罩 (#13,13,13) + 面板 (#222) 渲染；点"+ 添加密码" → 表单展开（输入框 11688 像素）

 20. **GtkOverlay 布局修正**：Fixed 容器用 `set_valign(Start)` + `set_margin_top(112)` 定位到内容区，不再覆盖地址栏/标签栏/书签栏；`set_webview_visible` 命令同时 hide/show Fixed（否则 Fixed 覆盖整个窗口拦截弹窗点击）。**验证**：地址栏按钮可点击（visible=true 时有 go_back 日志），弹窗按钮可交互
 21. **地址栏 ⋮ 菜单修复**：原 ⋮ 下拉菜单在 WebKitGTK 中菜单项文字不渲染（高度塌陷 25px）+ 点击菜单项后弹窗瞬间打开又关闭（WebKitGTK 合成 click 事件穿透到新渲染的遮罩 backdrop）。**解法**：去掉下拉菜单，改为 🔑（密码管理）和 🗂（标签分组）两个直接按钮
 22. **弹窗遮罩 @click.self 关闭去掉**：WebKitGTK 中合成点击事件会穿透到新渲染的遮罩，触发 `@click.self="emit('close')"` 导致弹窗瞬间关闭。PasswordsDialog/GroupsDialog 遮罩改为纯展示，只用 ✕ 按钮关闭
 23. **标签分组 UI 骨架已完成**（commit 57d9caf）：
     - **前端**：`types/tab.ts` 加 `groupId`，`stores/groups.ts` 新建（loadGroups/createGroup/updateGroup/deleteGroup），`stores/tabs.ts` 保留 groupId + assignTabToGroup
     - **TabBar.vue** 重写：分组头显示（颜色条 + 折叠/展开 + 计数），右键菜单分配分组（移动到分组/取消分组/新建分组并移动）
     - **GroupsDialog.vue** 新建：创建/重命名/改色（8色选择器）/删除分组
     - **App.vue**：GroupsDialog 接入 + loadGroups 启动时**并行加载**（不等 loadTabs/restoreLastSession，避免右键菜单打开时分组为空）
     - **TabBar 右键菜单兜底加载**：打开时如果 groups 为空自动 loadGroups
     - **WebKitGTK 兼容**：prompt() 不显示 → 默认名"新分组 N"；confirm() 不显示 → 删除分组不确认
 24. **标签分组功能已验证**：
     - ✅ GroupsDialog 打开/关闭正常（set_webview_visible false/true + 面板渲染）
     - ✅ 新建分组（"工作"）成功：数据库 tab_groups 1 条（id=1, name=工作, color=#4a9eff）
     - ✅ 分组列表显示（工作/生活/娱乐 3 个分组，含颜色点 + 删除按钮）
     - ✅ 组头在 TabBar 显示（颜色条 + 名字 + 计数 + ▶/▼ 按钮），3 个组头间距正确
     - ✅ 折叠/展开功能：点击 ▼ 按钮切换 collapsed 状态，数据库同步更新
     - ✅ 右键标签 → "移动到分组" → 选分组 → TabBar 实时更新（组头 + 组内标签）
     - ⚠️ 标签归组是前端运行时概念（tab.groupId 不持久化），重启后归组丢失
 25. **🔴 前端加载失败已修复（2026-09-12 关键突破）**：现象 "Connection refused"（前端白屏）。
     - **根因 1**：Cargo.toml tauri features 缺 `"custom-protocol"` → 应用进入 dev 模式 → 主 webview 连 http://localhost:5173（无 dev server）→ 报错。**修复**：features 加 `"custom-protocol"`。
     - **根因 2**：`strip = true` 会删除 cdylib（web extension）的符号 → WebKit 找不到入口函数 → `tauri://localhost` 协议失效。**修复**：改 `strip = "debuginfo"`（保留符号）。
     - **辅助**：lib.rs 手动设置 web extensions 目录（`WebContext::default()` + `set_web_extensions_directory(exe 目录)`），Tauri 2 不自动设置。
     - **验证**：启动后前端 UI 完整（标签栏/地址栏/书签栏正常，窗口 1280x800）。
 26. **🔴 链接点击不跳转已修复（2026-09-12）**：现象：网页内点链接无反应。
     - **根因**：child webview 被 reparent 到 GtkFixed 后，**WebKit 输入窗口不处理 DOM 点击**（点击到达 WebKitWebView 的 button_press 信号，但 WebKit 内部不导航）。grab_focus、标准 `gtk_widget_reparent`（FFI）均无效。
     - **解法**：`connect_button_press_event` 里用 JS 检查点击位置：`elementFromPoint(x,y)` + `closest('a[href]')`，命中链接则 `window.open(a.href)`（旧方案 `window.location.href` 是当前页导航）。
     - **关键**：`settings.set_javascript_can_open_windows_automatically(true)` 必须开（在现有 Settings 上改，不能新建替换，否则丢失 Tauri 默认设置导致链接点击失效）。
     - **验证**：日志 `on_new_window: url=...` → `create_tab` → `create_webview` → 新标签创建成功。
 27. **点击链接打开新标签页已实现（2026-09-12）**：`on_new_window` 回调由"当前 webview 导航"改为"**创建新标签页**"：`tauri::async_runtime::spawn` + `create_tab(app, state, url)` + 返回 `Deny`。前端靠 `startPolling()` 轮询（2s）自动显示新标签。**验证**：点新浪链接 → 新标签创建成功 → 标签栏出现新标签（OCR 5 个标签）→ 页面在新标签加载 ✓
 28. **书签删除 UI 已实现**：`BookmarksBar.vue` hover 显示 × 删除按钮。**关键**：改 src/ 里的 .vue 必须 `npm run build` 重建 dist（生产从 dist 加载），否则不生效。**验证**：hover 书签出现 ×（OCR：`www.qg.com * cn.bing.com`）✓
 29. **密码管理完整功能已验证（高哥手动 2026-09-12）**：查看密码 / 删除密码功能正常 ✓
 30. **下载管理 UI 已完成（2026-09-12）**：`DownloadsDialog.vue`（列表/进度条/状态标签/删除）+ `stores/downloads.ts` + `types/download.ts`；地址栏加 ⬇ 按钮（App.vue 接入，同密码弹窗：显示时 `set_webview_visible(false)` 隐藏 child webview）。**验证**：点 ⬇ → `get_downloads: 3 条` → 列表渲染（3 条 + 蓝色进度条）→ 点删除 → `delete_download: id=3` → 前端实时刷新 2 条（store filter 生效）→ ✕ 关闭（visible=true）✓
     - **注意**：列表 `ORDER BY id DESC` 倒序（最新在前），弹窗第一条是最大 id
     - **弹窗实际位置**：520px 宽居中（x=380-898），高度随内容（3 条记录时 y≈296-548）；xdotool 点击删除按钮用记录中心 y + x≈850
 31. **标签分组持久化已完成（2026-09-12）**：`SessionTab` 加 `groupId` 字段，`saveCurrentSession` 存 groupId，`restoreLastSession` 恢复时按顺序 `assignTabToGroup`（分组 id 是数据库持久自增，重启后不变；标签按 session 顺序恢复，顺序一致即可匹配）。**验证**：改 session JSON 给 2 个标签分别归组 生活(2)/娱乐(3) → 重启 → 会话恢复 2 标签 → TabBar 显示 2 个组头颜色条（x=21-32 + x=204-215）✓（commit a35ba0d）
 32. **分组管理功能验证（2026-09-12）**：
     - ✅ **改色**：点娱乐分组红色颜色点 → 数据库 color #4a9eff → #e53935（update_tab_group 生效）
     - ✅ **删除**：点删除按钮 → 娱乐分组删除（delete_tab_group 生效）
     - ✅ **折叠/重命名**：高哥手动验证通过（2026-09-12）——分组折叠/展开、重命名正常
     - **弹窗位置漂移问题**：分组/下载弹窗打开后位置可能变化（xdotool 点击前必须重新截图确认；改色/删除验证时曾因位置漂移误改生活分组颜色）
     - **改进**：`restoreLastSession` 恢复时校验分组存在性（useGroupsStore + groups.some），分组已删除则标签回未分组（commit 9036ef3）——避免 sessions 残留脏 groupId
 33. **设置页面已完成（2026-09-12，commit ecece8e）**：
     - **SettingsDialog.vue**：设置弹窗，三节——通用（主页 URL/搜索引擎）、外观（主题/书签栏显隐）、隐私（记录历史开关/清除历史按钮）
     - **settings store**（`src/stores/settings.ts`）：封装 get_all_settings/set_setting/clear_history，带默认值
     - **App.vue**：⚙ 设置按钮、弹窗、主题 class 切换（CSS 变量深/浅两套）、书签栏显隐
     - **AddressBar.vue**：主页按钮用设置的 homepage、地址栏搜索用设置的搜索引擎（bing/baidu/google）
     - **主题 CSS 变量改造**：TabBar/AddressBar/BookmarksBar/StatusBar 换成 var(--xxx)（弹窗保持深色浮层风格）
     - **Rust tabs.rs**：page_load_finished 按 record_history 开关记录历史（默认开；关掉则不写 history 表）
     - **验证**：设置写入数据库（theme=light 等 5 项）→ 重启 → 浅色主题生效（地址栏背景 245,245,245）✓ 书签栏隐藏生效 ✓ 历史记录写入 history 表 ✓ 高哥手动验收通过 ✓
     - **⚠️ ⚙ 按钮位置教训**：⚙ 放地址栏左侧导航区时点击无效（flex 布局挤压/被输入框盖住）→ 移到右侧（替换无功能的 ☆ 收藏按钮）后正常
     - **⚠️ 多标签并发崩溃**：恢复会话（5 个标签并发加载）时 WebKit 偶发 WebLoaderStrategy 崩溃（网络层 bug，与历史记录逻辑无关——清空 sessions 后正常）；当前 4 标签 session 正常
     - **⚠️ "Operation was cancelled" 提示**：WebKitGTK 原生控件干扰提示，功能正常，无需处理
 34. **工作区 UI 已完成（2026-09-12，commit f6824ca）**：
     - **概念**：浏览器的多工作区（类似 Chrome 配置文件）——每个工作区独立标签集合，切换工作区 = 保存当前标签 → 清空 → 恢复目标工作区标签
     - **workspaces store**（`src/stores/workspaces.ts`）：load/create（自动命名"工作区 N"）/rename/delete/switchWorkspace
     - **TabBar.vue**：左侧工作区按钮（☰ 当前工作区名 ▼）+ 工作区弹窗（默认工作区/工作区列表/新建/删除）
     - **sessions.rs**：save_session 加 workspace_id 参数；load_last_session 按工作区过滤（null=默认工作区）
     - **tabs.rs**：新增 close_all_tabs 命令（切换工作区清空标签）
     - **tabs store**：saveCurrentSession/restoreLastSession 带 workspace_id；clearAllTabs
     - **App.vue**：启动加载工作区列表、恢复当前工作区会话、标签变化按当前工作区保存
     - **验证**：创建"工作区 1/2"成功（workspaces 表）✓ 切换（close_all_tabs 执行）✓ 会话按工作区隔离（sessions.workspace_id）✓ 高哥验收通过 ✓
     - **⚠️ WebKitGTK 浮层菜单点击 bug**：position absolute/fixed 的下拉菜单渲染可见但点击事件不响应（xdotool 和真人点击都无效）→ **改用弹窗模式**（overlay + dialog，与设置弹窗一致）后点击可靠
     - **⚠️ HTML 弹窗被 GTK webview 盖住**：弹窗打开时必须 set_webview_visible(false) 隐藏 webview（HTML overlay 盖不住 GTK 层），关闭时恢复——TabBar watch showWorkspaceDialog 处理
     - **⚠️ 关闭按钮字符**：×（U+00D7）在 WebKitGTK 可能不渲染 → 统一用 ✕（U+2715，与设置弹窗一致）
     - **⚠️ release 编译**：Rust 改动必须 `cargo build --release`（运行的是 release 二进制，dev 编译不生效）
 35. **AI 侧边栏已完成（2026-09-12，commit 2bf5947）**：
     - **功能**：浏览器内置 AI 助手（右侧滑出面板）——对话问答 + 快捷操作（总结当前页/翻译当前页/解释当前页）
     - **AI 后端**：对接 **AGNES AI**（`https://apihub.agnes-ai.com/v1`，OpenAI 兼容格式），key 从环境变量 `AGNES_API_KEY` 读取，默认模型 `agnes-2.5-flash`
     - **Rust**：`ai.rs` 的 `ai_chat` 命令（reqwest 调用 chat/completions）；Cargo.toml 加 reqwest（rustls-tls 避免 openssl）
     - **前端**：`stores/ai.ts`（AI store：对话历史/发送/快捷操作/清空）；`components/AISidebar.vue`（overlay + 右侧 380px 面板：对话区/快捷按钮/输入框/发送）
     - **入口**：AddressBar 右侧蓝色 "AI" 按钮（emit openAi）→ App.vue showAI → AISidebar
     - **验证**：高哥验收通过（侧边栏打开、对话可用）
     - **⚠️ Vue 事件名陷阱**：emit 事件名含连续大写（如 `openAI`）会被 Vue 3 转成 `open-a-i`，模板 `@open-ai` 不匹配 → 必须用 `openAi`（避免连续大写）——**这是 AI 按钮点击无响应的根因**
     - **⚠️ WebKitGTK 面板高度**：flex 子元素 `height: 100%` 不生效（面板只有内容高度）→ 用 `position: fixed; right: 0; height: 100vh` 解决
     - **⚠️ emoji 按钮点击**：🤖（U+1F916）在 WebKitGTK 渲染异常（hover 正常但 click 不触发）→ 按钮内容改用文字 "AI"
     - **⚠️ overlay 关闭事件**：`@click.self` 会因 WebKitGTK 点击判定（release 元素触发 click）导致弹窗开了又关 → 去掉 overlay 关闭，只保留 ✕ 按钮（后续可优化）
     - **⚠️ xdotool 自动化**：`xdotool click`（屏幕坐标）在窗口失焦时无效 → 用 `xdotool click --window $WID`（窗口相对坐标）可靠；窗口激活用 `windowactivate --sync`
 36. **AI 设置参数已完成（2026-09-12，commit 6c168d3）**：
     - **功能**：设置对话框（⚙）加"AI 助手"区——模型选择（agnes-2.0-flash 推荐 / agnes-2.5-pro / agnes-2.5-flash 可能超时）、API 地址、API Key（留空用环境变量 AGNES_API_KEY）
     - **存储**：settings 表新增 ai_model / ai_base_url / ai_api_key（DEFAULT_SETTINGS 有默认值）
     - **Rust**：`ai_chat` 命令新增 base_url / api_key 可选参数（api_key 优先用传入，否则环境变量；base_url 默认 apihub.agnes-ai.com/v1）
     - **前端**：`ai.ts` sendMessage 从 settingsStore 读模型/地址/key 传给 ai_chat
     - **⚠️ 模型实测**：agnes-2.5-flash 在 AGNES 上 POST 超时（60s+ 无响应）不可用，agnes-2.0-flash 正常（~5s 返回）——默认模型用 2.0-flash
     - **⚠️ 安全**：API Key 明文存 settings 表（本地 SQLite，可接受）；任何输出不得泄露 key
 37. **下载管理真实下载流程已完成（2026-09-12，commit de574aa）**：
     - **功能**：网页触发下载（点击下载链接等）→ 自动记录到 downloads 表 + 进度实时更新 + 完成/失败状态
     - **Rust**：`tabs.rs` 用 `WebContext::connect_download_started`（全局注册一次，static AtomicBool 防重复）——下载开始：从 URL 推断文件名 → 设置目的地 `~/Downloads/<文件名>` → 同步插入记录拿 id（block_on，避免进度回调拿不到 id）→ 注册 received_data（进度更新）/ finished（completed）/ failed 回调 → 每次变更 emit `downloads-updated` 事件
     - **前端**：`DownloadsDialog.vue` onMounted 监听 `downloads-updated` 事件，弹窗打开时实时刷新列表（进度条/状态）
     - **⚠️ WebKit 下载 API 要点**：WebView 无 download 信号（webkit2gtk-rs 2.0）→ 用 `WebContext::connect_download_started`（需 use WebContextExt）；Download 对象无 get_suggested_filename/get_uri → 用 `request().uri()`（需 URIRequestExt）推断文件名、`response().content_length()`（需 URIResponseExt）拿总字节
      - **⚠️ 验证**：自动化测试受阻（地址栏合成键盘输入不可靠，导航不触发）——需高哥真人测试（打开下载页点下载链接）
  39. **广告拦截自定义关键词已完成（2026-09-12）**：
      - **功能**：设置 → 隐私 → 广告拦截下方可手工加/删关键词；内置词（关闭广告/头条报/推广等）+ 用户词一起注入
      - **Rust**：`get_adblock_keywords` / `set_adblock_keywords`，settings 表 `adblock_keywords` JSON 数组；`build_adblock_js` 替换模板 `__SB_KEYWORDS__`
      - **CSS**：扩充电商联盟链接、知乎/哔哩/视频平台类名；**去掉** `a[href*=tmall.com]`（购物页会全灭）和全局 `position:fixed+z-index`（会误杀弹窗）
      - **JS**：文本匹配限制 `offsetHeight<=520` 且字数 4-400，避免把整页正文藏掉
      - **验证**：单测 `build_js_*` 通过；release 已编译。需高哥重启后：⚙ → 隐私里看到加词框，加词后刷新页面生效

### 8.2 待办（下一步）

1. **已解决**：~~P1：密码管理 UI 完整功能验证（查看/删除密码）~~ → 29. 高哥手动验证通过（2026-09-12）
2. **已解决**：~~P1：下载管理 UI（下载记录列表/进度展示）~~ → 30. 已完成（2026-09-12）
3. **已解决**：~~P1：标签分组持久化（groupId 重启后丢失）~~ → 31. 已完成（sessions JSON 存 groupId，2026-09-12）
4. **已解决**：~~P1：GroupsDialog 重命名/改色/删除分组验证~~ → 32. 全部验证通过（高哥手动 2026-09-12）
5. **已解决**：~~P2：设置页面~~ → 33. 已完成（通用/外观/隐私，2026-09-12）
6. **已解决**：~~P2：工作区 UI~~ → 34. 已完成（独立标签集合，2026-09-12）
7. **已解决**：~~P2：AI 侧边栏~~ → 35. 已完成（AGNES AI 对话，2026-09-12）
8. **已解决**：~~P2：下载管理实际下载流程~~ → 37. 已完成（WebKit download 信号接入，2026-09-12，commit de574aa）
  9. **已解决**：~~P2：扩展管理（Chrome 扩展加载，Tauri 支持有限）~~ → 38. 已完成（UserContentManager 用户脚本/样式注入，2026-09-12，commit v5.4.0；高哥验证红色边框+链接波浪线生效）
  11. **已解决**：~~广告拦截增强（自定义关键词）~~ → 39. 已完成（2026-09-12）：内置词 + 设置页手工加词（`adblock_keywords`）+ CSS 扩充电商/内容/视频平台；去掉 `a[href*=tmall.com]`、全局 `position:fixed` 等误杀规则；文本匹配限制高度 520 / 字数 4-400
5. **已解决**：~~P2：窗口 resize 时同步 Fixed 内 webview 尺寸~~ → 14. 已完成
6. **已解决**：~~P2：初始化 git 仓库~~ → 15. 已完成
7. **已解决**：~~窗口最大化后无法还原/调整大小~~ → 17. GtkOverlay 重构已完成
8. **已解决**：~~target=_blank 链接打不开~~ → 16. on_new_window 已完成
9. **已解决**：~~标签分组/工作区 UI~~ → 23-24. 分组 UI 骨架 + 功能验证已完成
10. **已解决**：~~链接点击不跳转 / 前端加载失败~~ → 25-27. 已修复（custom-protocol + JS 手动导航 + 新标签页）

### 8.3 测试环境注意事项

- **Wayland**：应用必须 `GDK_BACKEND=x11` 启动才有窗口；`ffmpeg x11grab` 截屏全黑，要用 `import -window <wid>`；窗口位置会漂移，xdotool 用 `mousemove --window <wid>` 相对坐标
- **启动**（2026-09-12 更新）：生产模式，无需 vite：`(setsid env XMODIFIERS= GDK_BACKEND=x11 ./src-tauri/target/release/shift-browser > /tmp/shift-browser.log 2>&1 < /dev/null &)`（release 版，前端从 dist 加载）
- **日志**：Rust 命令有 `[tabs]` 前缀 println（create/close/switch/navigate/button_press 都打日志），验证交互看 `/tmp/shift-browser.log`；窗口坐标 = child webview 坐标 + 112（CONTENT_TOP_OFFSET）
- **⚠️ IME 干扰**：系统有中文输入法时，`xdotool type` 输入的 ASCII 会被 IME 当拼音转换（如 "baidu" → "豆包"），导致地址栏输入测试不可靠。绕过方式：`xclip` 设置剪贴板 + `ctrl+v` 粘贴，或直接改前端默认 URL 验证
- **OCR 验证**：`import -window <wid>` 截图 + `tesseract` OCR 可识别界面文字（标签标题、输入框内容）；裁剪区域放大 3-4 倍 + `ImageOps.autocontrast` 提高识别率
- **窗口 resize 测试**：窗口有最小尺寸约束（1280x800），`xdotool windowsize` 无法缩小；最大化用 `wmctrl -i -r <WID> -b add,maximized_vert,maximized_horz`，还原用 `wmctrl -i -r <WID> -b remove,...`（GtkOverlay 方案后还原正常）
- **WebKitGTK 合成键盘限制**：`xdotool type` 无法在 WebKitGTK 输入文字字符（合成键盘事件被忽略），但 ctrl 组合键（ctrl+a/ctrl+v）有效。密码管理 UI 的表单输入只能由高哥手动操作
- **滚动条**：Bing 首页内容不足一屏时无滚动条（WebKitGTK 正常行为），百度等长页面有滚动条

---

## 九、注意事项

- **禁用语**：永远不要说"中不中"（高哥明确禁止）
- **语言**：所有输出用中文（代码/变量名除外）
- **安全**：sudo 密码、API key、数据库密码不向输出透露
- **测试**：Wayland 下截图受限，验证窗口效果主要靠日志 + 进程状态
- **Git**：Tauri 项目已有 git 仓库（master 分支，39+ commits）

---

## 十、2026-09-13 安全体检修复（二狗执行）

### 10.1 安全（P0/P1）

1. **密码主密钥移出数据库**：`db/passwords.rs` 的 `get_master_key` 改为从独立密钥文件（`app_data/master.key`，权限 600）读取；旧版本存在 settings 表的 `master_key` 首次使用时自动迁移到文件并删除残留。`db/mod.rs` 的 `Db` 加 `key_path` 字段，数据库文件启动时权限收紧到 600。
2. **get_all_settings 脱敏**：`db/settings.rs` 加敏感键保护——`master_key` 前端永远拿不到（get_setting/set_setting/set_settings_bulk 三层拦截），`ai_api_key` 返回掩码（前4后4中间****）。
3. **newtab XSS 修复**：`newtab.rs` 加 `json_script_safe()`（`<` → `\u003c`），所有嵌入 `<script>` 的 JSON（feed_data/快捷访问/书签/最近访问）统一转义；serde_json 默认不转义 `<`，RSS 标题含 `</script>` 可注入任意 JS。
4. **CSP 配置**：`tauri.conf.json` 配完整 CSP；newtab/world-nav 两个 file:// 页面加 meta CSP（Tauri 的 CSP 对 file:// 不生效，必须 meta）。
5. **HANDOFF.md 移除明文 sudo 密码**（git 历史里仍有，如需彻底清除需 filter-repo 重写历史）。

### 10.2 功能修复（P1/P2）

6. **标签分组持久化接通**：`db/sessions.rs` 的 `SessionTab` 加 `group_id` 字段（`#[serde(default, rename = "groupId")]`）。此前前端 save/restore 都传了 groupId，但 Rust 侧没有该字段，serde 静默忽略——"以为做了，实际没接上"。
7. **close_tab 切相邻标签**：`tabs.rs` 改为删除前记录位置，优先切右邻、没有切左邻（原代码 `order.last()` 永远切最右 + 死代码）。
8. **update_bookmark 支持移出文件夹**：`folder_id=0` 置 NULL。
9. **密码查看按 id 精确定位**：新增 `get_password_by_id` 命令，PasswordsDialog 同域名多账号不再显示错。
10. **deleteWorkspace 恢复默认工作区会话**：删当前工作区后不再只清空标签。
11. **adblock 关键词转义补全**：`js_escape` 补换行/回车（原只转义 `\` 和 `'`，关键词含换行会生成非法 JS）。
12. **版本号统一 5.5.0**：Cargo.toml 从 5.0.0-alpha.1 对齐 package.json/tauri.conf.json。
13. **clippy 全绿**：21 个警告清零（含弃用 `run_javascript` → `evaluate_javascript`、死代码清理、未处理 Result 等）。
14. **测试增至 7 个**：新增主密钥迁移集成测试（旧值迁移沿用/新值生成/文件权限 600/settings 无残留）。

### 10.3 验证结果

- `cargo check` ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 7 passed ✅ / `npm run build` ✅
- release 编译 ✅，程序启动正常（日志无异常）
- 数据库权限已 600；settings 表 `master_key` 待用户下次使用密码功能时自动迁移（懒触发，符合设计）

### 10.4 遗留事项

- git 历史含明文密码（旧提交），如需彻底清除用 `git filter-repo` 重写历史（有风险，暂缓）
- sessions 表自动保存记录无限增长（328 条），建议后续加清理（保留最近 N 条）
- `reader.rs` 正文未净化（readability 提取 content 直接 innerHTML），建议后续用 ammonia 白名单净化

## 十一、2026-09-13 第二轮完善（P2/P3 收尾）

### 11.1 数据库增长控制（P2）

1. **sessions 表清理**：`db/sessions.rs` 的 `save_session` 插入后自动删除"自动保存"记录中超出最近 100 条的部分（`name = '自动保存'` 限定，手动命名的会话不受影响），解决 sessions 表无限增长（原 328 条）。
2. **record_history 清理优化**：`db/history.rs` 改为先 `SELECT COUNT(*)`，超过 10000 条才执行清理 DELETE，避免每次访问都跑全表 DELETE OFFSET 10000。

### 11.2 阅读模式加固（P2-20/P2-8）

3. **抓取超时**：`reader.rs` 新增 `scrape_article` 统一封装，`tokio::time::timeout` 20 秒兜底（readability 内部 blocking reqwest 无超时配置，慢站会挂死线程）。
4. **正文净化**：新增 `sanitize_html`，正则剥离 script/style/iframe/object/embed/svg 整块 + on* 事件属性，`fetch_article` 与 `enter_reader_mode` 输出都过净化，堵住阅读模式 XSS（readability 提取的 content 直接 innerHTML 有注入风险）。

### 11.3 安全与正确性（P2）

5. **数据目录权限 700**：`db/mod.rs` 的 `Db::init` 把整个 app_data_dir 收紧到 700（目录不可进，cookies 等 644 文件同机其他用户也读不到），数据库文件保持 600。
6. **真 UUID v4**：`tabs.rs` 弃用手写 xorshift+时间种子的伪 uuid（硬编码版本位但随机源弱），换 `uuid` crate 的 `Uuid::new_v4()`，删除无用 `rand_u32`。
7. **扩展计数真实化**：`extensions.rs` 的 `read_extension_dir` 返回 js/css 文件真实数量（原 `map(|_| 1)` 恒 0/1），`ExtensionDirResult` 扩为 7 元组。
8. **newtab 字号按钮修复**：`newtab.rs` 的 `applyInlineFs` 删除 `panelBody.innerHTML = panelBody.innerHTML` 重渲染——这行重建 DOM 丢掉 bindInline 绑定的事件，字号按钮点一次就失效；改为只更新 `--fs` 变量。
9. **TabBar 死代码**：`TabBar.vue` 删除 `.filter((g) => g.tabs.length > 0 || true)`。

### 11.4 工程化（P3）

10. **ESLint + Prettier**：新增 `eslint.config.js`（Vue 官方 flat config 链：pluginVue essential + typescript + prettier skip-formatting）、`.prettierrc.json`（semi:false、singleQuote、100 列）。注意：ESLint 9.39 的 @eslint/config-array 不支持数组嵌套数组，配置里必须 `...` 展开（否则报 "Unexpected array"）。src/ 全部格式化，`npm run lint` 0 错误 0 警告。
11. **GitHub Actions CI**：`.github/workflows/ci.yml`，前端 npm ci + lint + build，后端 cargo check + clippy（-D warnings）+ test（含 Tauri 系统依赖安装）。仓库未配远程，推 GitHub 后生效。
12. **数据库备份脚本**：`scripts/backup.sh`，python3 sqlite3 backup API 在线一致性备份（WAL 模式安全，不用停应用），默认保留 14 份，目录 700/文件 600。已实测：796K、integrity_check ok、14 张表完整。可 cron 定时：`0 3 * * * /home/gao/shift-browser-tauri/scripts/backup.sh >> /tmp/sb-backup.log 2>&1`。

### 11.5 新依赖

- Rust：`regex = "1"`、`uuid = { version = "1", features = ["v4"] }`
- 前端 devDependencies：eslint@^9、eslint-plugin-vue、@vue/eslint-config-typescript、@vue/eslint-config-prettier、prettier

### 11.6 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 7 passed ✅
- `npm run lint` 0 错误 ✅ / `npm run build` ✅ / `npx prettier --check src/` ✅
- release 编译 ✅，实机启动正常（日志无异常）
- 数据目录权限 700 生效 ✅，数据库及 -wal/-shm 600 ✅
- 新标签 uuid 为真 v4（日志可见 `4eca` 版本位）✅

### 11.7 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 下载保存走 `tabs.rs` 直接 INSERT + `downloads.rs` create_download 双写路径，后续可统一
- settings 表无 schema 校验，非法键/值可能写入，后续可加白名单
- feed.rs 手写 XML 解析（无命名空间处理），部分源可能解析失败

## 十二、2026-09-13 第三轮完善（遗留事项收尾）

### 12.1 下载双写路径统一

1. **SQL 单点维护**：`db/downloads.rs` 新增内部函数 `insert_download` / `update_download_bytes` / `update_download_state`，`create_download` 命令与 `tabs.rs` 的 WebKit download_started 信号共用同一套 SQL（原 tabs.rs 直接写 INSERT/UPDATE，两处 SQL 重复、改表结构要改两遍）。`tabs.rs` 四处下载 SQL 全部改为调用共用函数。

### 12.2 settings 白名单校验

2. **写白名单**：`db/settings.rs` 加 `ALLOWED_KEYS`（homepage/search_engine/theme/show_bookmarks_bar/record_history/ai_model/ai_base_url/ai_api_key/adblock_enabled/adblock_keywords 共 10 键）。`set_setting` 未知键报错拒绝；`set_settings_bulk` 未知键静默跳过（批量导入容忍脏数据）。读侧保持敏感键拦截（master_key 不可读、ai_api_key 掩码）。

### 12.3 feed 解析加固

3. **link 提取重写**：`extract_link` 遍历块内所有 `<link>`，跳过 `rel="self"`（feed 自身地址），取第一个 http 开头的正文链接；支持单/双引号 href。原实现只取第一个 `<link>`，Atom 源第一个常是 self 链接导致拿错地址。
4. **时间字段兼容**：published 提取加 `dc:date` 命名空间变体（原缺 DC 前缀）。
5. **自闭合判断修正**：`extract_tag` 的 `/>` 判断改为 `block[..gt].trim_end().ends_with('/')`（原 `block[gt-1..gt+1]` 对 `<link ... />` 带空格写法判断错误）。
6. **编码兜底**：`fetch_one` 的 `resp.text()` 改为 `resp.bytes()` + `from_utf8_lossy`（GBK 等非 UTF-8 源不再整源失败，如"人人都是产品经理""阮一峰"两个源此前报 decoding error）。
7. **新增 5 个解析单测**：RSS 标准/Atom self 链接跳过/单引号 href + dc:date/带属性 link/空 item 跳过。测试总数 7 → 12。

### 12.4 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 12 passed ✅
- `npm run lint` ✅ / `npm run build` ✅
- release 编译 ✅，实机启动正常，feed 首轮刷新 873 条 ✅

### 12.5 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- feed 非 UTF-8 源（GBK）lossy 解码后中文显示为 �，如需完美解码可后续接 encoding_rs 自动探测
- feed 源级去重/质量过滤（部分源标题重复）未做，后续可按需加

## 十三、2026-09-13 第四轮完善（feed 质量 + 下载体验）

### 13.1 feed 源级去重

1. **latest_items 按 url 去重**：`feed.rs` 查询改为 `id IN (SELECT MAX(id) FROM feed_items GROUP BY url)`，转载文章（不同源同一 url）不再重复显示，启动页资讯面板更干净。

### 13.2 feed 编码完美解码

2. **encoding_rs 自动探测**：`feed.rs` 新增 `decode_feed_bytes`——BOM（UTF-8/UTF-16LE/BE）优先 → 响应头 charset 显式解码 → 严格 UTF-8 → GBK 兜底（中文源最常见）。此前 `resp.text()` 对 GBK 源整源失败（"人人都是产品经理""阮一峰"），lossy 方案又会让中文变 �，现在 GBK 源能正确显示中文。新依赖 `encoding_rs = "0.8"`。

### 13.3 下载文件名冲突处理

3. **unique_download_path**：`tabs.rs` 下载目标同名文件已存在时自动加 `(1)(2)` 后缀（保留扩展名），不再静默覆盖。新增单测覆盖。

### 13.4 检查结论

4. **ai.rs**：90s 超时、API key 处理、错误返回均正常，无需改动。
5. **extensions.rs**：注入/启停/删除逻辑正常，已有单测。
6. **newtab weather**：WMO 映射表固定字符串 + 数字渲染，无注入面。

### 13.5 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 16 passed ✅（新增编码 3 个 + 下载路径 1 个）
- `npm run lint` ✅ / release 编译 ✅
- 注：运行中的实例仍是旧二进制，新代码（feed 去重/编码/下载唯一化）需重启生效

### 13.6 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- feed 源级质量过滤（标题过短/纯广告源）未做，可按需加

## 十四、2026-09-13 第五轮完善（world-nav 漏洞修复 + feed 质量过滤）

### 14.1 world-nav XSS 修复（防御纵深）

1. **world-nav JSON 注入转义**：`newtab.rs` 的 `render_worldnav_html` 原用 `tpl.replace("__WORLDAV_DATA__", WORLDNAV_JSON)` 直接嵌入原始 JSON，未过 `json_script_safe`——若数据含 `</script>` 会提前闭合标签导致 HTML 注入。已改为先 `serde_json::from_str` 再 `json_script_safe`（`<` → `\u003c`），与启动页保持一致。worldnav.json 虽是内置可信资源，但按防御纵深原则补齐。

### 14.2 feed 质量过滤

2. **标题质量过滤**：`feed.rs` 的 `parse_entry` 增加标题过滤——长度 <2 字符或纯标点（无任何字母数字）的条目剔除，避免资讯面板出现垃圾标题。

### 14.3 检查结论

3. **world-nav 页面**：已有 meta CSP + esc() 转义所有动态内容，功能正常，仅 JSON 注入点补齐。
4. **数据库迁移机制**：`db/mod.rs` 的 `run_migrations` 版本号 + 事务 + 记录表结构规范，无需改动。

### 14.4 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 16 passed ✅
- `npm run lint` ✅ / `npm run build` ✅ / release 编译 ✅

### 14.5 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- 运行中的实例仍是旧二进制，新代码（feed 去重/编码/下载唯一化/world-nav 修复）需重启生效

## 十五、2026-09-13 第六轮完善（feed 源健康检查 + 失效源替换）

### 15.1 全源健康检查

1. **62 源批量检查**：对 feeds.json 全部源做 HTTP 探测（状态码 + 前 200 字节 XML 标记检测），发现 2 个失效源：
   - **36氪**（plink.anyfeeder.com/36kr）：anyfeeder 返回 JS 落地页，非 RSS；官方 36kr.com/feed 同样失效（36氪已停止提供 RSS）
   - **品玩**（plink.anyfeeder.com/pingwest）：404；官方 pingwest.com/feed 同样 404
2. **替换为活跃源**：36氪 → 新浪科技（rss.sina.com.cn/tech/rollnews.xml，实测正常）；品玩 → 开源中国（oschina.net/news/rss，实测正常）。62 源数量不变。
3. **复检确认**：替换后全源复检，唯一"异常"为新浪科技长注释头（>200 字节，脚本前 200 字节窗口误报，实际完整 RSS）+ 光明日报单次 SSL 握手超时（重测正常，网络抖动）。62 源实际全部可用。

### 15.2 代码质量检查结论

4. **TODO/FIXME/HACK**：全项目 0 残留。
5. **日志敏感信息**：println 无 API key/密码/token 泄露；URL 打印为本地日志，可接受。
6. **密码功能**：AES-256-GCM + 随机 12 字节 nonce + 独立密钥文件 600，加密/解密/迁移测试齐全，无需改动。

### 15.3 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 16 passed ✅

### 15.4 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- 运行中的实例仍是旧二进制，第四~六轮新代码（feed 去重/编码/下载唯一化/world-nav 修复/源替换）需重启生效

## 十六、2026-09-13 第七轮完善（数据库运维修复）

### 16.1 修复内容

1. **WAL 文件膨胀**：体检发现 -wal 文件（1497KB）比主库（796KB）还大——大量写入（feed 刷新 1103 条）后 WAL 未收缩。修复：启动时执行 `PRAGMA wal_checkpoint(TRUNCATE)` 把已提交内容合并回主库并截断 WAL。实测：1497KB → 0KB，busy=0 无阻塞。运行中 WAL 超 4MB 由 SQLite 自动 checkpoint 兜底，不会无限增长。
2. **downloads 表无限增长**：下载记录无清理机制（get_downloads 只取最近 100 条，但表会一直涨）。修复：insert_download 插入后自动清理，只保留最近 500 条（与 sessions 100 条、history 10000 条上限同思路）。新增测试验证：插入 505 条后剩 500 条，最早 5 条被删。

### 16.2 其他体检结论

3. 表索引齐全（bookmarks/history/passwords/feed_items 均有索引），downloads 按主键排序无需额外索引。
4. 数据库现状：history 76 / sessions 100（清理生效）/ feed_items 1103（每源 50 上限）/ bookmarks 4 / passwords 1 / workspaces 2，无异常增长。

### 16.3 验证结果

- `cargo check` 0 警告 ✅ / `cargo clippy` 0 警告 ✅ / `cargo test` 17 passed（新增 downloads 清理测试）✅ / release 编译 ✅

### 16.4 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- 运行中的实例仍是旧二进制，第七轮新代码（启动 WAL checkpoint + downloads 清理）需重启生效

## 十七、2026-09-13 第八轮完善（前端统一错误提示 toast）

### 17.1 问题

前端大量 `invoke(...).catch(() => {})` 静默吞错——用户操作（保存设置、增删书签、增删分组、扩展操作、密码保存、删除下载记录、工作区切换）失败时完全无感知，只能查控制台。

### 17.2 修复内容

1. **新增轻量 toast 系统**（不引入第三方库）：
   - `src/stores/toast.ts`：Pinia store，show/error/success 三种类型，3 秒自动消失
   - `src/components/AppToast.vue`：右下角固定、TransitionGroup 动画（组件名多词，过 vue/multi-word-component-names）
   - `App.vue` 挂载 `<AppToast />`
2. **7 个 store 写操作统一加错误提示**：bookmarks（增/删）、extensions（加载/启停/删除）、groups（增/改/删）、workspaces（建/重命名/删/切换）、settings（保存/清历史）、passwords（保存/删除）、downloads（删除）——失败 toast + console.error
3. **settings.set 失败回滚**：乐观更新失败时恢复内存原值，避免 UI 显示已保存但实际未入库
4. **补未捕获异常**：GroupsDialog/TabBar 的 createGroup 调用原无 try/catch（store 失败会 throw），已补 catch（store 已 toast，调用方只做清理）

### 17.3 验证结果

- `npm run lint` 0 错误 ✅ / `npm run build` 通过 ✅ / prettier 格式化新文件 ✅
- 后端未改动，cargo 侧无需重跑

### 17.4 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- 运行中的实例仍是旧二进制，第七、八轮新代码（WAL checkpoint/downloads 清理/toast）需重启生效

## 十八、2026-09-13 第九轮完善（tabs 核心操作错误处理）

### 18.1 问题

tabs store 的 4 个核心操作（createTab/closeTab/switchTab/navigateTab）无 try/catch——创建标签、关闭标签、切换标签、导航失败时抛未捕获异常，用户无感知。

### 18.2 修复内容

1. **createTab**：加 try/catch + toast + console.error；返回类型改为 `Promise<Tab | null>`（失败返回 null）。调用方已兼容（restoreLastSession 有判空，App.vue/AddressBar/TabBar 不依赖返回值）。
2. **closeTab / switchTab / navigateTab**：统一加 try/catch + toast + console.error；switchTab 保持 _switching 合并连点逻辑（catch 后 finally 恢复标记）。
3. **审查 ai.ts**：sendMessage 已有 error 状态 + 面板显示，系统提示注入页面标题/URL 仅作文本传给 AI，无注入风险，无需改动。

### 18.3 验证结果

- `npm run lint` 0 错误 ✅ / `npm run build` 通过 ✅
- 后端未改动

### 18.4 遗留事项（更新）

- git 历史仍含明文 sudo 密码（旧提交），彻底清除需 `git filter-repo` 重写历史（有风险，暂缓）
- settings 表 `master_key` 残留待用户首次使用密码功能时懒迁移（符合设计）
- 全项目日志用 println!，如需结构化日志可后续接 log + tauri-plugin-log
- 运行中的实例仍是旧二进制，第七~九轮新代码需重启生效

## 十九、2026-09-13 挂账任务处理（git 历史密码清理 + 结构化日志）

### 19.1 git 历史明文密码清理

1. **排查**：`git log -S` 定位含明文 sudo 密码的提交——`7c2df83`（v5.0.0 Phase 1 引入）、`ddd98c8`（security 提交移除），中间历史仍含明文。
2. **执行**：仓库无远程（纯本地，无 force push 风险），`git filter-repo --force --replace-text` 重写全部 50 个提交，明文密码 → `***REMOVED***`。
3. **验证**：
   - `git log -S'密码' --all` → 0 匹配 ✅
   - 工作区文件 grep → 0 匹配 ✅
   - `git fsck --full` 健康 ✅
   - 含旧密码的 bundle 备份已删除（避免旧历史残留磁盘）
4. 注意：历史重写后所有 commit hash 已变（HEAD 18ae666），无远程不受影响。

### 19.2 结构化日志（println! → log）

1. **基础设施**：tauri-plugin-log 已注册（Info 级别，sqlx::query 降 Error 防刷屏），log crate 已在依赖。
2. **替换**：65 个 `println!`/`eprintln!` → `log::info!`/`log::error!`，覆盖 lib.rs、db/mod.rs、db/downloads.rs、db/passwords.rs、feed.rs、extensions.rs、tabs.rs 共 7 个文件。
3. **级别划分**：普通信息 info；失败类升级——feed 请求/读取失败、扩展启动注入失败、下载失败、记录插入失败 → error，扩展读取失败、record_history 失败 → warn。
4. **踩坑记录**：sed 先替换 `println!` 会把 `eprintln!` 变成 `elog::info!`（子串匹配），需二次修复 `elog::info!` → `log::error!`。
5. **验证**：cargo check 0 警告 / clippy 0 警告 / 17 测试全过 / release 编译完成。

### 19.3 遗留事项（更新）

- 挂账任务已全部处理完毕 ✅
- 运行中的实例仍是旧二进制，第七~十轮新代码（WAL checkpoint/downloads 清理/toast/tabs 错误处理/日志改造/历史清理）需重启生效

## 二十、2026-09-13 重启验证 + feed 失败重试机制

### 20.1 重启验证（第七~十轮代码全部生效）

1. **结构化日志生效**：日志格式 `[时间][级别][模块] 消息`，带时间戳/级别/模块名。
2. **WAL checkpoint 生效**：启动时 WAL 收缩，运行中保持小体积（453KB 内）。
3. **目录权限 700** ✅。
4. **feed 刷新**：发现 3 源偶发失败（人民日报/雷峰网/知乎日报），逐一排查确认源本身正常（anyfeeder 偶发限流/抖动，同批 3 源同时失败）。

### 20.2 feed 失败重试机制

1. **问题**：anyfeeder 偶发限流会让单个源整轮丢失（返回 0 条），要等 30 分钟下一轮才恢复。
2. **修复**：refresh_all 中单源首次抓取返回 0 条时，延迟 2 秒重试一次（warn 日志"首次抓取失败，2 秒后重试"）。62 源并发，总耗时影响小。
3. **验证**：重启后首轮刷新 994 条（上轮 904 条），豆瓣影评/钛媒体/新智元 3 源首次失败后重试成功补回。

### 20.3 验证结果

- cargo check 0 警告 / clippy 0 警告 / 17 测试全过 / release 编译完成
- 应用运行正常（PID 53724），feed 994 条

### 20.4 遗留事项

- 挂账任务全部处理完毕；当前无阻塞项
- 后续可选：feed 源内容级质量过滤（已有标题过滤）、前端组件进一步 UI/UX 优化

## 二十一、2026-09-13 第十二轮完善（密码弹窗安全 + 下载文件操作）

### 21.1 PasswordsDialog 安全修复

1. **关闭弹窗未清空已查看明文**：查看密码后关闭弹窗，再次打开密码仍显示在 UI 上。修复：watch 关闭时清空 revealId/revealPassword。
2. **onReveal 无错误处理**：getPasswordById 失败会抛未捕获异常。修复：加 try/catch + console.error。

### 21.2 DownloadsDialog 功能补全

3. **下载完成后无操作入口**（只有删除）。修复：completed 且带路径的记录显示"打开"（openPath）和"文件夹"（revealItemInDir）按钮，用 tauri-plugin-opener（插件早已注册但未使用，本次补装前端包 @tauri-apps/plugin-opener）。

### 21.3 组件/模块审查（健康，无需改动）

4. BookmarksBar：导航/删除/收藏均正常（store 已有 toast）。
5. StatusBar：URL/标签数/广告拦截/版本正常。
6. AddressBar：URL 补全、搜索、阅读模式正常。
7. history.rs：记录/清理（10000 上限）/查询均正常；search_history 的 LIKE 未转义通配符（影响极低，暂不改）。
8. bookmarks.rs：结构正常。

### 21.4 验证结果

- npm run lint 0 错误 ✅ / npm run build 通过 ✅
- 后端未改动

### 21.5 遗留事项

- 无阻塞项；后续可选：search_history LIKE 通配符转义、前端组件 UI/UX 进一步优化

## 二十二、2026-09-13 第十三轮完善（AI 接口健壮性 + 全模块体检）

### 22.1 ai.rs 健壮性微调

1. **服务错误响应体截断**：AI 服务返回错误时，响应体可能是一大坨 HTML/JSON，直接透传前端难以阅读。修复：错误正文截断 200 字符。
2. **结构化日志**：ai 调用成功时记录 model + 回复字符数（不含 key/消息内容），失败记录状态码/截断错误体，方便排查。
3. **修复编译错误**：model 移入 ChatRequest 后日志借用报错（E0382），用 `model.clone()` 修复。

### 22.2 全模块体检（本轮复查）

4. extensions.rs：adblock 开关/关键词/扩展加载/列表全部正常，失败路径有 warn 日志。
5. newtab.rs：数据全部转义；world-nav 用 json_script_safe（防 `</script>` 注入）；内联阅读正文经 reader.rs sanitize_html 净化（剥 script/style/iframe/object/embed/svg + on* 属性），外部网页正文插入本地页面无 XSS 风险。
6. reader.rs：fetch_article 返回前 sanitize_html，安全。

### 22.3 验证结果

- cargo check 0 警告 ✅ / cargo clippy 0 警告 ✅ / cargo test 17 passed ✅

### 22.4 遗留事项

- 无阻塞项；第十二~十三轮改动尚未重启生效（等高哥授权"重启"）
- 后续可选：search_history LIKE 通配符转义、前端组件 UI/UX 进一步优化

## 二十三、2026-09-13 第十四轮体检修复（高危 2 + 中危 11 + 低危 15）

> 背景：第十三轮重启后，按高哥要求做了一次全面代码体检（4 个并行探查代理 + 交叉验证），
> 输出报告 2 高危 / 11 中危 / 8 低危（低危展开 15 项），高哥批示"按照建议修复"，本轮全部落地。

### 23.1 高危（2/2）

1. **H1 AI Key 掩码被当真 key 回传（AI 功能失效）**：设置页保存 key 时把掩码回写数据库，请求时把 `sk-xxx****xxxx` 当真实 key 发给服务商。修复三处：
   - `ai.rs`：api_key 参数含 `****` 视为未配置，回退环境变量。
   - `ai.ts`：`realKey = apiKey.includes('****') ? '' : apiKey`，掩码不上网络。
   - `SettingsDialog.vue`：AI Key 输入框改为"已配置（不回显），留空保持不变"，不再写回掩码。
2. **H2 master.key 损坏静默重新生成（存量密码全废）**：`db/passwords.rs` 原来文件无法读取时重新生成新主密钥，导致已存密码全部无法解密。修复：区分"文件不存在（首次生成/迁移）"与"文件存在但损坏/读不了（报错停用，绝不覆盖）"。

### 23.2 中危（11/11）

3. **M1 ai.rs 错误体字节切片 panic**：错误正文截断改按字符（`chars().take(200)`），中文不再 panic。
4. **M2 弹窗 watch 竞态（Passwords/Groups/Downloads/Settings）**：统一加 `seq` 令牌丢弃过期回调 + try/catch/finally 保证 `set_webview_visible` 必执行 + 加载失败弹 toast。
5. **M3 扩展路径穿越**：`extensions.rs` 读 manifest 引用的 js/css 前校验落在扩展目录内（`base.join` + `starts_with`）。
6. **M4 switch_tab 悬空 active**：`tabs.rs` 先校验标签存在再写 active_id。
7. **M5 history 重复行**：新增迁移 v3 `20260913000001_history_uniq.sql`（合并重复行保留 MAX(id)、visit_count 累加、建 UNIQUE 索引），`record_history` 改 UPSERT（`ON CONFLICT(url)`）。
8. **M6 feed 响应体无上限 + 无并发限制**：`feed.rs` 响应体 4MB 上限；`refresh_all` 信号量压到 16 路并发。
9. **M7 阅读页 XSS**：`reader.rs` sanitize_html 增加剥 `<base>` + 剥 `href/src/action/xlink:href` 的 `javascript:/vbscript:/data:text/html` 协议；阅读页补 meta CSP（`default-src 'none'`）。
10. **M8 未用插件**：删除 shell/dialog/fs 三个插件（lib.rs 注册、Cargo.toml 依赖、capabilities 权限），只留 `core:default + opener:default`。
11. **M9 启动 block_on 阻塞**：newtab 生成改 `tauri::async_runtime::spawn`，不再阻塞主线程。
12. **M10 tabs 竞态**：`tabs.ts` 加 `_opSeq` 令牌，closeTab/switchTab/poll 过期回调不回写 activeTabId；goBack/goForward 补 try/catch。
13. **M11 工作区切换失败无回滚**：`workspaces.ts` currentId 在恢复完成后才提交，失败回滚原工作区；`restoreLastSession` 内部容错返回 false。

### 23.3 低危（15/15）

14. settings.rs 掩码改按字符（防中文 key 字节切片 panic）+ `get_setting` 对 ai_api_key 也掩码。
15. db/mod.rs 数据库权限 600 时序修正：移到 `connect_with`（建库）之后执行，首次运行才真正生效。
16. 广告拦截 JS 文本扫描节点上限：超过 3000 节点放弃文本特征词扫描（CSS 规则仍生效），防超大 DOM 卡主线程。
17. lib.rs 窗口恢复 `thread::sleep` → `tokio::time::sleep`，不占阻塞线程。
18. 版本漂移：feed.rs UA 硬编码"5.6" → `env!("CARGO_PKG_VERSION")`；newtab.rs 硬编码 v5.5.0 → format 参数 `{sb_version}`。
19. 死索引清理：迁移 v4 `20260913000002_drop_dead_index.sql` 删 `idx_feed_items_fetched`（无查询使用，纯写开销）。
20. CSP 收紧（tauri.conf.json）：删 `script-src 'unsafe-inline'`、connect-src 去掉 `https: http: ws:` 通配（AI 请求走后端 Rust）、加 `object-src 'none'` + `base-uri 'self'`。
21. SettingsDialog 广告开关双写 + saveKeywords 失败回滚 + toast。
22. GroupsDialog 删除分组失败不清标签归属（deleteGroup 失败 rethrow）；折叠/换色乐观更新 + 失败回滚（双击竞态）。
23. AddressBar：空 URL 清空地址栏（不残留旧标签地址）；localhost/IP/host:port 识别为 URL 而非搜索词。
24. ai.ts 消息历史只回传最近 20 条（长对话 payload 不膨胀）。
25. App.vue 启动初始化加 try/catch 兜底（至少一个标签可用）+ loadTabs 内部容错 + startPolling 无论如何都启动。
26. extensions.ts 开关失败回写原值（checkbox 视觉与 store 一致）。
27. AISidebar loading 期不发送不清输入（不丢用户输入）。
28. SettingsDialog 移除 `@click.self` 关闭（与其余弹窗统一，防误触）。

### 23.4 验证结果

- cargo clippy --all-targets 0 警告 ✅
- cargo test 18 passed ✅（新增 test_history_upsert_dedupes_and_counts）
- npm run lint 0 错误 ✅ / npm run build（vue-tsc + vite）通过 ✅

### 23.5 遗留事项

- 第十四轮改动尚未编译重启（等高哥授权"重启"）
- 后续可选：search_history LIKE 通配符转义、前端组件 UI/UX 进一步优化

## 二十四、2026-09-13 第十五轮收尾（search_history 通配符转义）

### 24.1 修复内容

1. **search_history LIKE 通配符转义**（第十三/十四轮遗留技术债）：用户搜索历史时，输入含 `%` 或 `_` 的字符（如 "100%_off"）会被 SQL LIKE 当作通配符，导致误匹配无关记录。修复：查询前对 `\`、`%`、`_` 做转义，SQL 加 `ESCAPE '\'` 子句，按字面量匹配。

### 24.2 验证结果

- cargo clippy --all-targets 0 警告 ✅
- cargo test 19 passed ✅（新增 test_search_history_escapes_like_wildcards）

### 24.3 遗留事项

- 无阻塞项；前端组件 UI/UX 进一步优化（可选）

## 二十五、2026-09-13 第十六轮前端 UI/UX 优化

### 25.1 主题一致性（H1）

1. 5 个弹窗 + AI 侧栏 + 工作区弹窗/右键菜单 + 书签栏：硬编码暗色（`#222/#2a2a2a/#e0e0e0/#444/#4a9eff` 等）批量替换为 CSS 变量（`--bg-* / --text-* / --border-* / --accent`），浅色主题切过去弹窗不再是深色块。
2. 新增语义色 `--success` / `--danger`；StatusBar 广告拦截绿、Toast 边框跟变量走。
3. `main.css` body 背景/文字改走变量。

### 25.2 交互反馈

4. **弹窗动画**：密码/分组/下载/设置 + 工作区弹窗用 fade；AI 侧栏用 slide。
5. **全局按钮**：`focus-visible` 焦点环 + `:active` 按压缩放。
6. **加载指示**：`tabs` store 加 `loadingTabId`；刷新按钮转圈、地址栏底部 2px 进度条；8 秒兜底关掉（同 URL 刷新轮询看不出变化）。
7. **后退/前进置灰**：`canGoBack` / `canGoForward` 乐观追踪（导航后可退、后退后可进、切标签重置）。
8. **Esc 关闭弹窗**（AI → 设置 → 下载 → 分组 → 密码优先级）；关闭按钮补 `aria-label`；AI 遮罩点击关闭。
9. **激活标签关闭按钮常显** + 鼠标中键关闭。
10. **AI 思考中**显示 spinner；发送失败把原文填回输入框。
11. 弹窗宽度 `min(480~520px, calc(100vw - 32px))`；设置页 select 补自绘下拉箭头。
12. 地址栏 HTTPS 锁图标、focus-within 描边；阅读模式 tip 改为绝对定位不挤按钮。
13. Toast 补 `.toast-move` 过渡，颜色改走主题变量。

### 25.3 验证结果

- npm run lint 0 错误 ✅ / npm run build（vue-tsc + vite）通过 ✅
- 后端未改动（无需 cargo test）

### 25.4 遗留事项

- 书签栏隐藏时 webview 仍从 y=112 起（Rust `CONTENT_TOP_OFFSET`），需 Rust 侧按显隐重算偏移
- 后退/前进是前端乐观追踪，webview 内链跳靠轮询补 canGoBack，不是 WebKit 原生 can_go_back
- 弹窗列表加载骨架屏未做（打开即拉数据，空白窗口极短）

## 二十六、2026-09-13 第十七轮可选打磨（书签栏偏移/原生导航/日志/样式抽取）

### 26.1 书签栏偏移动态化（M3，遗留项）

1. `TabManager` 新增 `show_bookmarks_bar: Mutex<bool>`（默认 true）+ `top_offset()`：书签栏显示时 112（标签40+地址42+书签30），隐藏时 82。
2. 新增常量 `CONTENT_TOP_OFFSET_NO_BOOKMARKS = TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT`；`content_size(window, top)` 改为接收动态 top。
3. `create_webview` / `setup_gtk_layout` / `on_window_event`（Resized）全部改用动态 top，日志带 `top=` 便于排查。
4. 新增命令 `set_bookmarks_bar_visible(visible) -> f64`（返回新 top）；前端 `SettingsDialog.onSet` 与 `App.vue` onMounted 初始化时调用同步。

### 26.2 WebKit 原生导航状态（H3+，遗留项）

5. 新增命令 `get_nav_state() -> {canGoBack, canGoForward}`，通过 `WebViewExt::can_go_back/forward()` 直接查原生历史栈（不再依赖前端乐观追踪）。
6. 注意：`with_webview` 闭包要求 `'static + Send`，不能借用局部变量，用 `Arc<Mutex<(bool,bool)>>` 捕获结果。
7. 前端 `tabs.ts` 轮询循环每 2s 调 `get_nav_state` 覆盖乐观值，webview 内链跳转/JS history 也能正确反映后退/前进可用性。

### 26.3 ai.rs 失败路径日志

8. ai_chat 入口、HTTP 客户端创建、请求失败、响应体读取、服务错误截断、JSON 解析失败、choices 为空均补 `log::error!/warn!`，排障可定位到具体环节。

### 26.4 启动主题闪变

9. `index.html` 加同步 `<script>`：启动前读 `localStorage.theme`，为 light 则给 `documentElement` 加 `theme-light`，避免浅色用户启动先深后浅闪一下。

### 26.5 弹窗公共样式抽取

10. `.dialog-overlay / .dialog / .dialog-header / .close-btn / .dialog-body` 从 4 个弹窗（密码/分组/下载/设置）提到 `App.vue` 全局 style（非 scoped），组件内只保留宽度覆盖（分组/下载 520px，密码/设置用默认 480px）；新增 `.close-btn:hover` 统一 hover 反馈。CSS 产物 37.48KB → 35.39KB。

### 26.6 其余打磨

11. **右键菜单贴边**：`TabBar.onTabContextMenu` 按估算宽 180px / 高（100 + 分组数×30）翻转，避免点在右下角时菜单溢出窗口。
12. **组徽章对比度**：`.group-count` 背景 `--bg-secondary` → `--bg-hover`，文字 `--text-muted` → `--text-secondary`，浅色主题下不再与组头融为一体。
13. **弹窗加载态兜底**：groups/downloads store 加 `loading` 标志；三个弹窗空态前加 `v-else-if="loading"` 显示"加载中…"（本地查询毫秒级，骨架屏收益低，轻量兜底即可）。

### 26.7 验证结果

- cargo check / clippy --all-targets 0 警告 ✅
- cargo test 19 passed ✅
- npm run lint 0 错误 ✅ / npm run build（vue-tsc + vite）通过 ✅（CSS 瘦身 2.09KB）

### 26.8 遗留事项

- 无阻塞项；`set_bookmarks_bar_visible` 只更新 Rust 侧 top 状态，现有 child webview 的 GTK Fixed 位置在下次 Resize 时才会重算（前端书签栏显隐本身是 DOM 层，偏差可接受）

## 二十七、2026-09-13 第十八轮功能增强（快捷键/拖拽/右键菜单/历史面板/书签栏即时重算）

### 27.1 书签栏即时重算（第二十六章遗留）

1. `set_bookmarks_bar_visible` 现在拿到窗口后直接调整 GTK Fixed 的 `margin_top` 和所有 child webviews 尺寸（按新 top 计算 content_h），不再依赖下次窗口 Resize 才生效。

### 27.2 快捷键增强（浏览器标配）

2. Rust 新增 `reload_tab` 命令（WebKit 原生 `WebViewExt::reload`），已注册；前端 `tabs.ts` 加 `reloadTab()`。
3. Rust 新增 `reorder_tabs` 命令（拖拽排序用，缺失 id 兜底追加，锁分开不嵌套避免死锁），已注册；前端 `reorderTabs()` 乐观更新 + 失败回滚。
4. `AddressBar` 加 `inputEl` ref + `focusInput()`（聚焦并全选）并 `defineExpose`。
5. `App.vue` 全局快捷键：Ctrl+T 新建标签、Ctrl+W 关闭当前标签、Ctrl+R 刷新、Ctrl+L 聚焦地址栏、Ctrl+H 打开历史、Ctrl+Tab / Ctrl+Shift+Tab 切换前后标签；弹窗打开时屏蔽标签类快捷键防误触；Esc 关闭链补历史面板。

### 27.3 标签拖拽排序

6. 未分组 + 分组内两组标签加 `draggable` + dragstart/dragover/drop/dragend；拖拽中 `.tab.dragging` 半透明反馈；同层级排序（跨组移动暂不支持）。

### 27.4 右键菜单增强

7. TabBar 右键菜单顶部加：刷新 / 复制链接（clipboard）/ 新标签打开 / 关闭标签（danger 红色）；下方保留原分组移动功能。

### 27.5 历史管理面板（Ctrl+H）

8. 新组件 `HistoryDialog.vue`：搜索框（300ms 防抖，走 `search_history`）、最近历史列表（200 条，走 `get_recent_history`）、点击条目新标签打开、单条删除（hover 显示）、清空全部（confirm 二次确认）、时间格式化。
9. 复用全局弹窗公共样式（第二十六章抽取）；沿用隐藏/恢复 child webview 的竞态防护模式（seq 令牌 + finally）。

### 27.6 验证结果

- cargo clippy --all-targets 0 警告 ✅
- cargo test 19 passed ✅
- npm run lint 0 错误 ✅ / npm run build（vue-tsc + vite）通过 ✅
- 中途修复：TabBar `.tab` 样式编辑时括号配平出错（CssSyntaxError），已修复合并 transition

### 27.7 遗留事项

- 无阻塞项；跨组拖拽移动标签暂不支持（可后续做"拖到分组头归组"）
- 历史面板清空用原生 confirm（WebKitGTK 下可能无显示，后续可换自定义确认）

## 第二十八章（第十九轮）：搜索建议 + 书签管理 + 跨组拖拽归组

### 28.1 地址栏搜索建议下拉

- 输入防抖 200ms 并发调 `search_bookmarks`（前 5 条）+ `search_history`（前 8 条），合并显示
- 书签带 🔖 图标、历史带 🕘 图标；标题 + URL 两栏展示
- 键盘导航：↓/↑ 移动选中（循环）、Enter 打开选中项、Esc 关闭；无选中时 Enter 走原 URL/搜索逻辑
- 失焦 150ms 延迟关闭（保证 mousedown 点击建议生效）；`@mousedown.prevent` 避免抢焦点
- 下拉样式：`position: absolute` 挂在 .address-container 下，top 34px，max-height 320px 可滚动

### 28.2 书签管理面板

- BookmarksBar 右键菜单（`position: fixed` 贴边翻转，复用 TabBar 模式）：
  - 书签项右键：＋ 新标签打开 / 🔗 复制链接 / ✏ 编辑 / ✕ 删除（danger）
  - 栏空白右键：☆ 收藏当前页 / 📁 新建文件夹（默认名"日期 文件夹"）
- 编辑弹窗：复用全局 .dialog-overlay/.dialog 公共样式，改标题 + 网址，空值拦截
- bookmarks store 新增 `updateBookmark(id, title?, url?)`（走 `update_bookmark` 命令）与 `createFolder(name)`（走 `create_bookmark_folder` 命令）

### 28.3 跨组拖拽归组

- onDrop 判断来源/目标 tab 的 groupId：不同 → 先 `assignTabToGroup` 归组，再 `reorderTabs` 排序到目标位置；相同 → 原排序逻辑
- 分组头加 dragover/dragleave/drop：拖到分组头归组到该组（不强制排序）
- 分组头 drop-target 高亮（accent 外框 + bg-active），拖离恢复

### 28.4 验证

- npm run build ✅ / npm run lint ✅
- cargo clippy 0 警告 ✅ / cargo test 19 passed ✅
- 版本 5.5.0；CSS 产物 39.96KB

### 28.5 遗留事项

- 历史清空仍用原生 window.confirm（WebKitGTK 下可能无显示，后续可换自定义确认弹窗）→ ✅ 第二十一轮已解决
- 书签文件夹当前仅支持创建（无展开/嵌套 UI），书签栏只显示顶层书签 → 删除 UI ✅ 第二十一轮已补

---

## 第二十九章（第二十轮）：翻译面板 + 用户账号系统 + 新标签页增强 + 书签管理面板 + IPC 桥

> 补记（交接文档当时漏写，据 git `d1e3e1b` 归档）

- **翻译**：`translate.rs` 选中文本/整页翻译（`shift://__translate__` 协议），TranslatePanel 侧滑面板（Ctrl+Shift+T）
- **账号**：`users.rs` 注册/登录/资料/偏好（PBKDF2 10 万次迭代 SHA-512，与 Electron 原版一致），偏好登录时写入 settings 生效
- **新标签页**：Bing 每日壁纸（主进程拉取规避 CORS）、字号设置、动态背景开关
- **书签**：BookmarksDialog 管理面板 + 右键编辑/删除/新建文件夹
- **IPC 桥**：`ipc-bridge.js` 解决 shift:// 自定义协议页不执行 UserScript 的问题
- **验证**：cargo clippy 0 警告 / 19 测试全过 / npm lint 0 错误 / build 通过 / release 编译成功

---

## 第三十章（第二十一轮）：安全加固 + 功能补齐 + 性能优化 + 硬化（2026-09-18）

全修计划：P0 安全 5 项 + P1 功能 5 项 + P2 性能 4 项 + P3 硬化 3 项。提交 `0b9075a`。

### 30.1 P0 安全（5 项）

- **P0-1 恢复 CSP**：`tauri.conf.json` 恢复 `security.csp`（default 严格 + dev 宽松含 localhost:5173/ws）；因 `shift://` 是自定义协议、Tauri 不会自动注入 CSP，在 `lib.rs` 对 main index.html / newtab / worldnav 响应手动加 `Content-Security-Policy` 头（`SHIFT_CSP`）
- **P0-2 reader.rs 净化**：原 regex 方案可被 `<svg/onload=alert(1)>` 绕过 → 改用 `ammonia 4.2.0` 白名单（标签/属性白名单、url_schemes 仅 http/https、on* 全剥）；Cargo.toml 删 `regex`
- **P0-3 user_export 鉴权**：去掉可传的 `username` 参数，强制导出 `current_user_id` 指向的当前登录用户
- **P0-4 ai.rs SSRF**：新增 `validate_base_url`（仅 https、拒 userinfo、拒内网/本机字面地址与内网域名）+ `is_private_host`；注意本机 Rust 工具链 `IpAddr` 无 `is_private`，需 match 到 Ipv4Addr/Ipv6Addr
- **P0-5 下载文件名净化**：取 `url::Url::parse(u).path()` 解码 + `sanitize_filename`（剥路径穿越/控制字符/首尾点段、限长 200）

### 30.2 P1 功能（5 项）

- **P1-1/P1-2**：HistoryDialog 去 `window.confirm` 改自定义确认弹窗；BookmarksDialog 补文件夹删除 UI（含级联删除提示+确认弹窗），全项目不再有 window.confirm/alert
- **P1-3 会话管理**：新增 `SessionDialog.vue`（列表/恢复/删除/保存当前为命名会话），store 加 `restoreSession`/`saveNamedSession`，快捷键 **Ctrl+Shift+S**
- **P1-4 整页翻译**：新增 `whole_page_translate.js`（include_str! 注入）——收集文本节点/去重/分批 15 段调 `/batch`/轮询回写 DOM/右上角进度浮层；`/batch` 每批 5 并发保序
- **P1-5 静默吞错**：SettingsDialog / DownloadsDialog / TabBar / App.vue / tabs.ts / workspaces.ts 的容错分支改 `console.error` 或 toast（webview 显隐、剪贴板等辅助调用仍保留静默）

### 30.3 P2 性能（4 项）

- **P2-1 tabs 事件推送**：`tabs.rs` 四处 emit `"tabs-updated"`（页面加载后、标题变化时、新窗口建标签后、favicon 更新后）；前端 `startPolling` 改事件监听 + **8s 兜底**（`document.hidden` 时跳过），`stopPolling` 一并取消 listen
- **P2-2 newtab 去重**：加 `GEN_LOCK`（tokio Mutex）序列化 + 1.5s mtime 防抖；Bing 壁纸改 `wallpapers.json` 6h TTL 缓存（失败回退旧缓存）；删除 `lib.rs` 里与异步生成重复的同步 `block_on` 预生成块
- **P2-3 日志降级**：点击按下/抬起、滚轮、favicon、滚动诊断 `info → debug`（这几处每次点击/滚动都刷一条，会刷爆日志）
- **P2-4 历史查询**：迁移 **v6** 加 `idx_history_last_visit`（原表只有 url 唯一索引，排序/清理全表扫）；`record_history` 的 COUNT(*) 降频到每 64 次 + `MAX(id)` 快速短路

### 30.4 P3 硬化（3 项）

- **P3-1 PBKDF2 迭代版本化**：10 万 → **21 万**（OWASP 建议下限；60 万在本项目 `opt-level="s"` 下实测登录近 1 秒太卡）。参数写入 `hash_algo`（`pbkdf2-sha512-210000`），旧标记 `pbkdf2` 按 10 万兼容，**验证成功后自动透明升级**；账号导入读备份 `hashAlgo`，旧备份兜底
- **P3-2 密钥/数据库文件权限时序**：`std::fs::write` 会先按 umask 落 644 再 chmod → 改 `OpenOptions.mode(0o600)` 创建即收紧；数据库文件同样预创建为 600
- **P3-3 delete_setting 白名单**：抽 `check_deletable`，敏感键（master_key）+ 白名单双重校验（原可删任意键，包括切断密码解密链的 master_key）

### 30.5 验证结果

- `cargo clippy --all-targets` **0 警告 0 错误**；`cargo test` **34 passed**（原 27 + 新增 7）
- `npm run lint` / `npm run build` 通过（81 模块）
- release 编译通过；精确 PID 重启（旧 437457 → 新 544406），标签页/扩展注入正常
- **生产库实测**：迁移 v6 已应用、`idx_history_last_visit` 已建、gaoge 的 `hash_algo` 已在锁屏解锁时自动升级为 `pbkdf2-sha512-210000`、db 与 master.key 权限均 600

### 30.6 遗留事项

- **整页翻译受第三方页面 CSP 限制**：注入脚本用 `Image().src = 'shift://__translate__/batch?...'` 触发，受目标页自身 `img-src` 约束，严格 CSP 站点可能失效（与既有"翻译选中文本"机制同源限制），待实测确认
- **UI 层未人工实测**：Ctrl+Shift+S 会话管理、整页翻译、书签文件夹删除确认、历史清空确认弹窗——单测覆盖不到，需要手动点一遍
- **测试数据规范**：`.learnings/LEARNINGS.md` 新增"单测禁用真实密码"教训（本轮差点把真实锁屏密码提交进仓库，提交前扫描拦住）

## 第三十一章（第二十二轮）：战略规划立档 + 阶段 0 基座（2026-09-19）

### 战略纲领
- 新增 `docs/strategy-2026.md`：ShiftBrowser 战略规划（定位/五大支柱/阶段 0-4 路线图/架构演进/风险治理）
- 新增 `docs/spike-engine-capabilities-2026-09-19.md`：引擎能力实测报告
- 决策记录：**本地模型集成暂缓**（阶段 1.2 只做 provider 抽象 + 云端 provider）

### 引擎实测关键结论（Spike #1-#5）
- **引擎升级不可行**：Ubuntu 25.10 系统源冻结 WebKitGTK **2.52.3**（2.54.0 需发行版升级），→ 阶段 0 改为"版本冻结 + 跟踪机制"，不自行自编译
- **GPC 请求头可行**：`WebKitWebPage::send-request`（Web 进程扩展侧）+ `WebKitURIRequest` 可变请求头
- **WebExtensions 初始 API 存在**（2.52 的 `WebExtension` 类），但能力面窄，需专项 spike
- **WebAuthn**：引擎有实现（链接 libsecret），平台认证器待运行时实测
- **WebGPU**：引擎有、无公开运行时开关 → 不作为能力依赖
- **意外收获**：`WebKitUserContentFilterStore`（引擎级内容过滤）、`enable-developer-extras`（Web Inspector）、`enable-dns-prefetching`/`hardware-acceleration-policy`

### 本轮交付
1. **DevTools**：`Settings.enable-developer-extras` + GTK 窗口层拦截 F12 / Ctrl+Shift+I → `open_devtools` 命令（`WebInspectorExt::show`）
2. **广告拦截 2.0 v1**：
   - 新增 `engine/` 模块（架构解耦第一块砖）+ `engine/content_filter.rs`
   - **手写 FFI** 对接 WebKit 内容过滤器（gtk-rs 绑定缺失：`auto/` 无 `user_content_filter_store`，`add_filter` 被注释为 Ignored）
   - 75 条网络层拦截规则（国际广告网络/追踪统计/社交像素/国内广告统计）
   - 启动时异步编译（5231 字节规则）并缓存 GObject 指针，随注入流程挂到每个 child webview
   - 开关联动：关闭广告拦截时 `remove_filter_by_id` 摘除
   - 2 个单测（`rules_json_is_well_formed` / `rules_use_valid_regex_chars`），**抓出 JSON 非法转义 bug**
3. **批次 0.3 快捷键路由**：
   - 根因：前端快捷键绑在 HTML 层，**点击网页后焦点落在 child webview，按键到不了 HTML 层** → 所有浏览器级快捷键时灵时不灵
   - 方案：GTK 顶层窗口统一拦截（`gtk_win.connect_key_press_event`），用 `UI_FOCUSED`（AtomicBool）跟踪焦点归属
     - 页面持焦（`!UI_FOCUSED`）→ 匹配 Ctrl+T/W/R/L/H、Ctrl+Tab、Ctrl+Shift+T/S 后 `emit("global-shortcut", action)`，返回 `Propagation::Stop`
     - UI 层持焦 → 放行（前端 keydown 已处理，避免重复触发）
     - F12 / Ctrl+Shift+I 不依赖焦点，任何情况都响应
   - 焦点跟踪挂载点：`setup_gtk_layout` 里 `main_gtk.connect_focus_in_event`（UI 层）、`fix_gtk_layout` 里 `child_gtk.connect_focus_in_event`（页面）
   - 前端：抽出统一动作入口 `handleGlobalAction(action)`，keydown 与 `listen('global-shortcut')` 共用

### 编译铁律（务必遵守）
**Tauri 发布版把 `dist` 编译期嵌入二进制**：改前端必须 `npm run build && cargo build --release`（前端在前）。顺序反了会跑旧前端（已踩坑，见 `.learnings/ERRORS.md`）。

### 待验证（需人工确认）
- 各快捷键在页面持焦下是否全部生效（若某个被 WebKit 自身消费需换 GtkAccelGroup 方案）
- 内容过滤器的实际拦截效果（DevTools Network 面板观察）

### 第三十一章续：广告拦截 2.0 v2（2026-09-19 深夜）
- **自定义关键词进入网络层**：`build_rules_with_keywords()` 把用户关键词编译成规则（`keyword_to_pattern` 只保留字母/数字/汉字与 `.` `-`，剔除正则元字符；单个关键词 ≥2 字符；上限 200 条）
- **免重启生效链路**：`set_adblock_keywords` → `collect_enabled_items`（remember_rules 记录新规则指纹）→ `inject_extensions` 主线程闭包 `compile_pending()` → 异步编译 → `on_save_done` 缓存后 spawn `extensions::re_inject` → 挂到所有标签
- **时序修复**：`register_app`（注册缓存目录 + 应用句柄）必须排在 `inject_on_startup` **之前**；`compile_pending` 在未注册时把规则放回队列而不是丢弃
- **验证**：单测 3 个通过；实测加 1 个关键词后规则 5231 → 5304 字节，还原后回落 5231
- 全量测试：**62 个 Rust 单测通过**

### 待办：桥接已知问题
- 2026-09-19 15:53 高珩在钉钉发"继续"，桥接日志显示 `完成: dm:... 回复 0 字符` —— **消息被接走但回复为空**，未落到当前会话。需排查桥接会话路由/空回复。

### 第三十一章续二：桥接故障修复（2026-09-20 凌晨）
- **现象**：高珩在钉钉发的"在吗/继续/先1后2"全部无回复，日志只有 `回复 0 字符`
- **根因**：模型 429 限流被吞——opencode 接口报错时**仍返回 HTTP 200**，错误在 `result.info.error`，桥接只取 `parts` 拿到空串
- **修复**：桥接 `config.json` 模型 `opencode/big-pickle` → `opencode-go/deepseek-v4.1-flash`；`src/index.js` 加 `promptWithRetry`（识别 error、429 退避重试）+ `friendlyError`（中文提示）+ 空文本告警
- 详情见桥接侧 `.learnings/ERRORS.md`

### 第三十一章续三：架构解耦批次 0.2 首项 —— 布局几何收敛（2026-09-20 凌晨）
- **新增 `src-tauri/src/engine/layout.rs`**（引擎抽象层第二块砖）：
  - 布局常量唯一事实来源：`TAB_BAR_HEIGHT/ADDRESS_BAR_HEIGHT/BOOKMARKS_BAR_HEIGHT/STATUS_BAR_HEIGHT/AI_PANEL_WIDTH/CONTENT_TOP_OFFSET(_NO_BOOKMARKS)/MIN_*`
  - `ContentGeometry::compute(window, top, ai_active)`：窗口尺寸 + 缩放换算 + AI 分屏让位，一处算清
  - `apply_to_webview(window, tab_id, geo)`：定位/缩放承载 webview 的 GtkFixed + `queue_resize()`
  - `content_size()`：建标签用的浮点尺寸
  - 单测：常量关系校验
- **消除 6 处重复公式**（`tabs.rs` 原 `inner.width as f64 / scale`、`.max(100.0)`、`.max(400.0)` 等全部清空）：
  1. `TabManager::content_size`（建标签）
  2. `switch_visibility`（切标签）
  3. 窗口 resize 监听（闭包内用克隆的 `win_for_geo` 句柄）
  4. `set_webview_ai_panel`（AI 分屏）
  5. `set_bookmarks_bar_visible`（书签栏显隐）
  6. `apply_tab_geometry`（显示后重应用几何）
- **常量导出改为 `pub use crate::engine::layout::CONTENT_TOP_OFFSET`**，`lib.rs` 等外部引用不受影响
- **验证**：`cargo build --release` 零警告；**63 个单测全过**；重启后 `window resized: 1280x800 content=1280x662 top=116` 与重构前一致；书签栏切换日志正常；无 panic
- **意义**：这是战略"引擎抽象层"从「内容过滤」扩展到「布局」的第一步，后续分屏/垂直标签等新形态可直接复用 `ContentGeometry`，不再各自手写公式


### 第三十一章续四：可观测性 —— 自检命令（2026-09-20 凌晨）
- **新增 `src-tauri/src/selfcheck.rs`** + `selfcheck_cmd` 命令 + 前端设置页「🩺 自检」分区
- **17 项检查分四组**：
  1. 版本与环境：ShiftBrowser 版本、**WebKitGTK 运行时版本**（`webkit_get_*_version`）、系统、数据目录存在性
  2. 数据存储：DB 连通性、表数量、书签/历史/密码行数、**密码主密钥权限（必须 600）**
  3. 运行时：标签数+活动标签、主窗口尺寸、AI 分屏状态
  4. 能力状态：内容过滤器就绪/开关、广告拦截开关、隐私档位、启用扩展数
- **设计原则**：只读不改；单项失败不影响整体；版本/路径等环境事实必须报（避免用户描述问题时说不清环境）
- **启动自动自检**：`App.vue` onReady 后调一次，日志输出「环境」与「能力」两行快照
- **封装**：`TabManager` 新增 `runtime_snapshot()`（自检读私有字段改为走公开方法）
- **验证**：编译零警告、**64 个单测通过**、实测 `[selfcheck] 健康=true`、日志能看到 `WebKitGTK=2.52.3` 与数据目录
- **顺带发现**：自检报出「广告拦截=关闭」，核对 DB `adblock_enabled=false` —— 功能正常，是真实状态（需与高珩确认是否要开）

### 第三十一章续五：架构解耦批次 0.2 第二项 —— WebView 基础操作收敛（2026-09-20 凌晨）
- **新增 `src-tauri/src/engine/webview.rs`**（引擎抽象层第三块砖）：
  - `apply_default_settings(wv)`：页面默认设置（JS 自动开窗 / 开发者工具 / **UA 常量 `DEFAULT_USER_AGENT`**）统一施加
  - `eval_js(wv, script)`：执行 JS 忽略结果（失败仅 debug——页面未就绪时失败属正常）
  - `eval_js_with(wv, script, cb)`：执行 JS 并回调结果字符串（`Ok(String)/Err(String)`）
  - 单测：UA 格式校验
- **收敛调用点**：`evaluate_javascript` 原先散落 **tabs.rs 3 处 + extensions.rs 5 处**，现全部走引擎层；`settings.set_*` 在 tabs.rs 归零
- **耦合度变化**（同口径 grep）：
  | 文件 | WebKit 调用点 |
  |---|---|
  | tabs.rs | 25 → **8** |
  | lib.rs | 15 → **6** |
  | extensions.rs | 10 → **6** |
  | newtab.rs | 6 → **0** |
- **验证**：编译零警告、**65 个单测通过**、重启后实测——延迟探针 `probe: tr=boolean`、注入探针 `__shiftTranslateMenu=boolean`、页面加载后即时注入成功（21730 字节）、隐私防护注入正常、无 panic
- **坑（已记经验本）**：`wv.settings()` 在同时引入 gtk/webkit prelude 时有歧义，须显式 `webkit2gtk::WebViewExt::settings(wv)`

### 第三十一章续六：架构解耦批次 0.2 第三项 —— WebView 基础操作补全（2026-09-20 凌晨）
- **`engine/webview.rs` 补 4 个 API**（公开 API 达 7 个）：
  - `enable_console_output(wv)`：页面 console 输出到 stdout（诊断 JS 错误）
  - `reload(wv)`：刷新页面
  - `navigation_state(wv) -> (bool,bool)`：能否后退/前进
  - `open_inspector(wv)`：打开开发者工具（内部显式 `WebInspectorExt::show` 避免与 WidgetExt::show 歧义）
- **tabs.rs 4 处调用收敛**：console 输出 / reload / can_go_back+forward / inspector show 全走引擎层
- **耦合度**：tabs.rs webkit 依赖 **25 → 1**（仅剩 use 行）；lib.rs 5；extensions.rs 1
- **边界结论（重要）**：`connect_*` 信号连接方法（connect_load_failed / connect_title_notify / context()）**不能**从 tabs.rs 移除——它们是业务回调载体，依赖 `WebViewExt` trait 在作用域；引擎层只收敛"基础操作"，**信号回调留在业务层**（封装会把业务逻辑拖进引擎，得不偿失）。曾误删 use 导致 4 个 E0599，已恢复。
- **验证**：编译零警告、65 单测通过、重启实测健康、隐私注入正常、无 panic

### 第三十一章续七：架构解耦批次 0.2 第四项 —— GTK 布局操作收敛（2026-09-20 凌晨）
- **`engine/layout.rs` 补 3 个 API**（公开 API 达 5 个）：
  - `parent_vbox(wv)`：从 webview 向上找父级 GtkBox（不调 window 方法避免主线程死锁）
  - `find_or_create_fixed(vbox)`：从 GtkOverlay 找 GtkFixed，不存在则建（从 tabs.rs 原函数迁移，含 set_halign/set_valign）
  - `set_fixed_visible(wv, visible)`：GtkFixed 显隐（弹窗打开时 Fixed 会拦截点击，必须一起隐藏）
- **tabs.rs 4 处收敛**：fix_gtk_layout 的 vbox 查找、setup 的 main vbox 查找、原 find_or_create_fixed 函数删除、弹窗 Fixed 显隐
- **验证**：编译零警告、65 单测通过、重启实测 `GtkOverlay 布局初始化完成 top=116` + `child_webview reparent 到 Fixed: pos=(0,116) size=(1280,662)` 正常、无 panic
- **坑（已记经验本）**：python 批量替换脚本里断言失败被静默跳过（缩进不匹配），导致 2 处替换没生效还编译通过了（因为 parent_vbox 报 never used 才发现）——脚本必须断言失败即退出

### 第三十一章续八：测试网（E2E 冒烟）—— scripts/smoke.sh（2026-09-20）
- **新增 `scripts/smoke.sh`**：E2E 冒烟测试，把解耦各批次关键链路钉死
  - `scripts/smoke.sh`：完整模式（重启浏览器 → 等启动 → 逐项检查）
  - `scripts/smoke.sh --no-restart`：只检查当前实例
- **12 项检查**：进程存活 / 数据库存在 / 自检健康 / 环境快照 / WebKitGTK 版本 / 快捷键挂载 / GtkOverlay 布局初始化 / child reparent 到 Fixed / 隐私防护注入 / 翻译菜单探针 / 扩展即时注入 / 无 panic
- **退出码**：0=全过，1=有失败，2=用法错误
- **实测**：完整模式 12/12 通过、--no-restart 12/12 通过、失败路径正确报 FAIL
- **使用**：以后每次改完 Rust/前端，跑 `scripts/smoke.sh` 即可确认无回归

### 第三十一章续九：技术债清算 1+2 —— 静音真实现 + 阅读列表死表删除（2026-09-20）
- **标签静音真实现**（原为假实现：只翻转内存 is_muted 字段，注释误称"WebKitGTK 无 per-tab 音量 API"）：
  - 事实：WebKitGTK 2.36+ 有 `webkit_web_view_set_is_muted`，gtk-rs 绑定 `WebViewExt::set_is_muted` 存在（web_view.rs 1950 行）——当初调研不仔细
  - `engine/webview.rs` 新增 `set_muted(wv, muted)` API
  - `toggle_mute` 改为：翻转状态（作用域内取 cloned 避免借用冲突）+ 异步派发到主线程调 `set_muted`（参考 open_devtools_for_active 模式，避免主线程重入）
- **阅读列表死表删除**：`reading_list` 表 v5.0.0 建表但功能从未实现、代码零引用、数据零行
  - 新增迁移 `20260920000000_drop_reading_list.sql`（DROP TABLE IF EXISTS，注释保留重建线索）
  - 迁移 10 注册 `drop_reading_list_dead_table`，实测执行成功、表已删、迁移记录正确
- **验证**：编译零警告、65 单测通过、冒烟 12/12 通过

### 第三十一章续十：技术债清算 3 —— 固定标签排序（2026-09-20）
- **问题**：固定标签（📌）与普通标签混排，按创建顺序，无"固定永远最前"语义
- **修复**（纯前端，两处）：
  - `TabBar.vue`：`groupedTabs` 计算属性加 `pinnedFirst()` 排序——未分组区与分组内都固定优先（固定之间保持原顺序）
  - `stores/tabs.ts`：`reorderTabs` 加拖拽保护——普通标签不能拖到固定标签前面、固定标签不能拖到普通标签后面（store 权威顺序与 Rust 一致）
- **验证**：vue-tsc 类型检查通过、npm build + cargo build 零警告、65 单测通过、冒烟 12/12

### 第四十章：阶段 1 启动 —— 1.1 页面理解层（2026-09-20）
- **新增 `engine/content.rs`**（页面理解层核心）：
  - `Snapshot` 结构体：标题/URL/语言/正文文本/可交互元素统计（链接·按钮·输入框·文本域·下拉·iframe）/表单字段/链接列表
  - `SNAPSHOT_JS`：内嵌 DOM 提取脚本（JSON.stringify 返回，文本截断 4000/表单 20/链接 30）
  - `extract_snapshot(wv)`：eval_js_with + channel 同步等待（5s 超时），全程本地不出网
  - `summarize_text()`：本地规则摘要（供 AI 上下文）
  - 单测 ×2：JS 结构校验 + 摘要截断
- **新增 `content_cmd.rs`**：
  - `content_snapshot`：取活动标签 webview → 提取语义快照（用 `TabManager::runtime_snapshot().1` 拿 active_id，避免私有字段）
  - `content_extract`：正文提取（复用 `reader::scrape_article_public`，出网操作由调用方授权）
- **reader.rs**：加 `scrape_article_public` 公共包装（复用 curl+readability 管线）
- **前端接入**（stores/ai.ts）：`sendMessage` 前先 `content_snapshot`，成功则把正文摘要+交互元素附进 system prompt（AI 真正"理解页面"）；失败降级为标题+地址，不阻断对话
- **类型**：新增 `src/types/content.ts`（Snapshot 前端类型）
- **验证**：编译零警告、**67 单测通过**（+2）、冒烟 12/12、vue-tsc 通过
- **待高哥实测**：AI 面板发消息 → 后端日志应出现 `[content] 语义快照: tab=... title=...`

### 第四十章补：content_snapshot 死锁修复（2026-09-20）
- **bug**：`extract_snapshot` 的 `recv_timeout(5s)` 在 `with_webview` 闭包内（主线程）执行 → 主线程阻塞 → evaluate 回调无法执行 → 死锁 5 秒卡 UI → 高哥"发送不了消息"
- **修复**：拆成 `extract_snapshot_async(wv, tx)`（只发起不等待），`content_cmd.rs` 的 `recv_timeout` 移到闭包外（async 线程）等待
- **铁律**：with_webview 闭包内绝不阻塞等待（channel/锁/睡眠）
- **验证**：67 单测、冒烟 12/12、新二进制运行中

## 第三十二章（阶段 1.2）：推理层 Provider 抽象（2026-09-20）

### 目标
阶段 1（AI 原生内核）的 1.2 推理层：把"AI 对话"从单一 AGNES 硬编码升级为**多 provider 抽象**，为将来接本地模型/其他云端留接口（本地模型集成按 2026-09-19 决策暂缓，只留 stub）。

### 交付内容
1. **`engine/ai_provider.rs`**（新文件，引擎层第三块业务砖）：
   - `AiProvider` trait（`#[async_trait]`，`Send + Sync`）：`id`/`name`/`is_configured`/`chat`/`chat_stream`（on_delta 用 `Box<dyn Fn(String) -> Result<(), ()> + Send + 'static>`——dyn trait 不能用泛型方法）
   - `AgnesProvider`：持有 base_url + api_key，委托 `crate::ai::agnes_chat_impl` / `agnes_chat_stream_impl`
   - `LocalProvider`：stub，`chat` 返回"本地模型集成暂缓"错误（2026-09-19 决策）
   - `resolve_provider(id, base_url, api_key)`：空/未知 id 回退 agnes；key 掩码（****）过滤 + 环境变量回退
   - `list_providers()` → `ProviderInfo`（设置页展示）
2. **`ai.rs` 重构**：
   - 抽离 `agnes_chat_impl(base_url, api_key, req)`：SSRF 校验（`validate_base_url` + `validate_resolved_host`）+ 非流式请求 + 429 友好提示 + 错误体截断（按字符非字节）
   - 抽离 `agnes_chat_stream_impl<F>(base_url, api_key, req, cancel, on_delta)`：SSE 解析循环 + cancel 中断（返回"已停止生成"）+ on_delta 回调 Err 时提前返回（前端关通道）
   - `ai_chat`/`ai_chat_stream` 命令：新增 `provider: Option<String>` 参数，经 `resolve_provider` 路由；cancel 标志注册/移除、Channel 推送留在命令层（impl 只回调）
3. **前端**：设置页 AI 区新增"推理服务（Provider）"下拉（`ai_list_providers` 加载，local 置灰标"暂缓"）；`stores/ai.ts` sendMessage 透传 `ai_provider` 设置；`ai_provider` 设置键（默认 'agnes'）

### 技术要点（踩坑记录）
- **dyn trait 不能有泛型方法**：`chat_stream` 的 `impl Fn` 参数导致 E0038 → 改 `Box<dyn Fn + Send + 'static>`（Box 本身也是 dyn 兼容的）
- **async fn in trait 的 dyn 兼容**：Rust 1.97 原生支持 async fn in trait，但 dyn 场景受限 → 引入 `async-trait` crate（Cargo.toml 新增依赖）
- **tauri Channel 是 Clone 的**：`Box::new(move |delta| on_event.send(...))` move 了 on_event，后续 Done/Error 还要用 → 闭包前 `on_event.clone()`
- **测试环境无关性**：`resolve_agnes_default` 原假设"无 key 必 Err"，但测试机设置了 AGNES_API_KEY → 改测"掩码 key 被过滤"语义（Ok 则回退环境变量成功 / Err 则未配置，两分支都合法）

### 验证
- 71 单测通过（70+1，新增 4 个 provider 测试、修正 1 个环境依赖测试）
- 编译零警告（`req.stream` 字段被 impl 实际使用消除 1 个；trait 元数据方法 `#[allow(dead_code)]` 注明设置页接入后移除）
- 冒烟 12/12
- 待高哥实测：设置页选 Provider + AI 对话走新路由（日志应显示 provider=agnes）

### 下一步
- 阶段 1.3 工具层（MCP 客户端）——按 strategy-2026.md 路线图
- 或先做设置页 provider 多端点配置完善（当前 agnes 端点可配，local 置灰）

## 第三十三章（阶段 1.3）：工具层 MCP 客户端（2026-09-20）

### 目标
阶段 1（AI 原生内核）的 1.3 工具层：AI 面板能调用外部工具。落地为 **MCP 客户端**（Model Context Protocol）——接本地/远端 MCP server，工具注册表 + 权限白名单 + 调用审计日志。

### 交付内容
1. **迁移 11**（`20260920000001_mcp_tools.sql`）：
   - `mcp_servers`：name（唯一）/command/args（JSON 数组）/enabled/allowed_tools（白名单 JSON，空=全部）
   - `tool_audit_log`：server/tool/arguments/result_summary/success/created_at + 索引
2. **`engine/mcp.rs`**（引擎层）：
   - `McpConnection`：子进程（stdlib）+ JSON-RPC 2.0 请求/通知
   - **协议细节**：`initialize` 握手（协议版本 2025-03-26）→ `notifications/initialized`（**无 id 通知，server 不应答**）→ `tools/list` → `tools/call`
   - **30s 请求超时**：server 卡死不响应时返回错误，不挂起调用方（端到端测试抓出死等 bug）
   - 连接缓存：server_id → 长驻进程（删除/停用时断开）
   - 工具注册表：`list_tools` 汇总启用 server 工具，白名单过滤，全局名 `{server}__{tool}`
   - `call_tool`：统一执行入口（白名单校验 + 审计写库 + 结果拼接）
   - `to_openai_tools`：转 OpenAI function calling 格式
3. **`mcp_cmd.rs`**：7 个命令（`mcp_list_servers`/`mcp_add_server`/`mcp_remove_server`/`mcp_set_server_enabled`/`mcp_list_tools`/`mcp_call_tool`/`mcp_get_audit_log`）
4. **`ai.rs` function calling 支持**：
   - `ChatMessage` 加 `tool_call_id`/`tool_calls`（OpenAI 兼容）；新增 `ToolCall`/`ToolCallFunction`
   - `ChatRequest` 加 `tools`
   - `agnes_chat_impl` 返回结构化 `ChatMessage`（含 tool_calls）
   - **`ai_chat` 工具执行循环**：≤6 轮（模型请求工具 → 逐个 `call_tool` → 结果回传 → 再请求）；自动携带启用工具（前端调用签名不变）
5. **前端**：
   - `stores/ai.ts`：`tools`/`toolsLoaded` 状态 + `loadTools()`；**有工具走非流式 `ai_chat`（function calling）**，无工具保持流式
   - 设置页 MCP 分区：server 列表（启用开关/删除/工具数）、添加表单（name/command/args/白名单）、可用工具列表、手动调用测试、审计日志
6. **端到端测试**（`mcp_end_to_end_with_fake_server`）：node 假 server 脚本 + 临时 DB，验证 spawn→握手→tools/list→tools/call→审计写库→白名单过滤全链路

### 踩坑记录（重要）
- **MCP notification 无 id 不应答**：`notifications/initialized` 是 JSON-RPC 通知（无 id），server 不响应。用 `request()` 发会带 id 死等 → 新增 `notify()` 方法（只写不读）。**这是端到端测试卡死抓出来的**
- **MCP 请求必须有超时**：server 卡死会挂起调用方 → 30s 超时保护
- **Vue 模板单引号属性值内不能含裸双引号**：`placeholder='参数 JSON（如 ["-y",...]）'` 编译报错 → 换 `&quot;` 实体
- **python 写 Rust 源码的转义**：`"["other"]"` 在 python 里写进文件会变 `["other"]`（裸引号断 Rust 字符串）→ 用 `r#"..."#` 原始字符串

### 验证
- 74 单测全过（+1 端到端）、编译零警告、冒烟 12/12
- 端到端实测：假 server 握手/工具列表/调用/审计/白名单全链路 0.08s 跑通
- 待高哥实测：设置页 MCP 分区加一个真实 server（如 `npx -y @modelcontextprotocol/server-filesystem /tmp`），AI 面板对话让其调工具

### 下一步
- 阶段 1.4 执行层（Agent：语义快照 → LLM function calling → 动作执行，安全铁律：动作白名单/每步人工确认/仅当前标签）
- 或 MCP 增强：streamable HTTP 远端 server、工具热更新

## 第三十四章（阶段 1.4）：执行层 Agent（2026-09-20）

### 目标
里程碑 M1 的最后一块：AI 助手能在授权下执行多步任务（点击/填写/滚动/读取/导航）。
安全铁律（strategy 1.4）：动作白名单、每步人工确认、仅当前标签、凭据永不外泄、注入防护。

### 交付内容
1. **`engine/agent.rs`**：
   - 动作白名单 9 项：click/fill/scroll/read/wait/navigate/back/snapshot/submit
   - JS 模板生成：selector/value 等参数 **JSON 序列化后嵌入**（引号/反斜杠逃逸防护，单测覆盖）
   - **密码框拒绝**：fill/read 对 `input[type=password]` 直接拒绝（凭据永不外泄）
   - `browser_action` OpenAI 工具定义（enum 约束动作）
   - `describe_action` 人类可读描述（确认卡片展示）
2. **`agent_cmd.rs`（每步人工确认）**：
   - `request_agent_action`：生成 request_id → emit `agent-action-request` 事件（前端弹卡片）→ oneshot 等待确认（60s 超时）
   - `agent_confirm_action(request_id, approved)`：前端回调，发送确认结果
   - `execute_agent_action`：仅当前活动标签（TabManager.runtime_snapshot 取 active_id）；wait/navigate/back/snapshot 后端动作 + click/fill/scroll/read/submit 页面 JS
   - `agent_debug_execute`：设置页调试入口（绕过确认，仅调试用）
3. **`ai.rs` 工具循环接入**：tools 恒带 `browser_action`（与 MCP 工具合并）；循环里 `call.function.name == "browser_action"` 走确认流程（允许→执行→结果回传 / 拒绝→"用户拒绝了该动作"），否则走 MCP
4. **前端 AISidebar**：`listen('agent-action-request')` 弹确认卡片（允许/拒绝按钮）；footer 显示工具数

### 安全铁律落地对照
| 铁律 | 落地 |
|---|---|
| 动作白名单 | 固定 9 动作，白名单外（如 eval）直接拒绝（单测覆盖） |
| 每步人工确认 | 所有动作执行前 emit 事件 + oneshot 等用户确认，60s 超时 |
| 仅当前标签 | 动作只作用于 runtime_snapshot 的活动标签 webview |
| 凭据永不外泄 | fill/read 拒绝密码框；快照提取排除密码 |
| 注入防护 | 参数 JSON 序列化嵌入 JS 模板，单测验证选择器含引号不逃逸 |

### 踩坑记录
- **tauri 2 取窗口**：`app.get_window("main")` 返回 `Window`，与 `WebviewWindow` 不匹配 → 用 `get_webview_window("main")`
- **`wv.navigate` 要 `Url`**：`url::Url::parse` 转换
- **evaluate 回调嵌套 Result**：`recv_timeout` 解一层后 raw 仍是 `Result<String,String>`（回调错误），需再 `?` 解一层

### 验证
- 80 单测全过（+6 agent：转义/密码框/白名单/后端动作/解析/描述）、编译零警告、冒烟 12/12
- 待高哥实测：AI 面板说"往下滚动一点"→ 弹确认卡片 → 允许后页面滚动

### 下一步
- 阶段 1.5 标准跟踪（WebMCP/Agentic Browsing 审计）
- 或 Agent 增强：多步任务编排提示词、动作回放/重试、跨标签任务（需放松"仅当前标签"）
## 第三十五章 标准跟踪（WebMCP + Agentic Browsing 审计）

> 阶段 1.5 · 2026-09-20 · 交付⑯ · 单测 81（+1）· 零警告 · 冒烟 12/12

### 背景与结论一句话
WebMCP 已从原型进入 Chrome 149–156 公开 origin trial（2026-05-18 批准，Google I/O 2026 宣布），
Lighthouse 13.3 加了 Agentic Browsing 类别（2026-05-07 入默认）；但 ShiftBrowser 引擎
**WebKitGTK 2.52.3（冻结）不支持 `navigator.modelContext` 且无法升级** → 走**客户端 polyfill 试验**
（捕获网站 WebMCP 工具声明，不冒充能力）。

### 交付内容
1. **调研报告** `docs/standard-tracking-2026-09-20.md`：WebMCP 现状（origin trial+字符预算+W3C Web ML CG 孵化）、
   Agentic Browsing 四信号（WebMCP 注册/schema、无障碍树、CLS、llms.txt·pass-fail 比率制）、差距分析、
   落地路径四项（polyfill 试验>llms.txt 检测>无障碍增强>agentic 自测）、**标准跟踪机制**（季度复核，下次 2026-12-20）。
2. **WebMCP polyfill 试验（最小可行·默认关）**：
   - 新增 `engine/webmcp.rs`：`WEBMCP_POLYFILL_JS`（document_start 幂等注入，捕获 `navigator.modelContext.registerTool`
     到 `window.__webmcp_tools`；**只捕获声明、不伪造执行能力**，其他 modelContext API 不实现）；结构 + 单测。
   - `engine/content.rs`：`Snapshot` 加 `webmcp_tools`（`#[serde(default)]` 向后兼容）+ `SNAPSHOT_JS` 附带提取内联。
   - `extensions.rs`：`collect_enabled_items` 返回 `(items, webmcp_on)`；`inject_extensions` 加 `webmcp_on` 参数
     （循环内 Start 注入 polyfill + 即时 eval，**8 处调用点全适配**）；`get/set_webmcp_polyfill` 命令 + lib.rs 注册。
   - 前端：`content.ts` Snapshot 加 `webmcpTools`（可选）、`ai.ts` 快照描述附带页面暴露的 WebMCP 工具、
     `SettingsDialog.vue` WebMCP polyfill 试验开关（默认关 + "试验中"徽标 + 说明文字）。

### 踩坑
- **`#[serde(default)]` 必须加在字段上**：Snapshot 新字段若不加，旧前端（TypeScript 无此字段）反序列化会失败——
  语义快照是跨端契约，加字段必须向后兼容。
- **WebMCP polyfill 必须 Start 注入**（网站脚本 feature-detect 前）；End 注入（UserScript 默认）会让网站
  feature-detect 落空、不注册工具 → 单独加 Start UserScript + 即时 eval。
- **改函数签名要全文搜**：`collect_enabled_items`/`inject_extensions` 有 8 处调用点分散在 extensions.rs，
  改返回值类型/参数必须一次搜全（编译错误 E0308/E0061 逐点暴露）。

### 标准跟踪机制（strategy 承诺兑现）
跟踪对象：WebMCP（origin trial 进度/正式化）、MCP 协议版本、llms.txt、Lighthouse Agentic Browsing、agent 流量治理。
节奏：**每季度复核**（下次 2026-12-20），任一对象重大进展即时复核。王二狗每季度主动提醒高哥。

### 验证
- 81 单测（+1 webmcp polyfill 幂等/只捕获不伪造），零警告；冒烟 12/12。
- 前端生产构建零错误，Cargo 全量 release 零警告。

---

## 第三十六章 ~ 第四十三章（阶段 1.5–2 补记 · 2026-09-20）

> 补记说明：这几章逐章记录此前遗漏（HANDOFF 一度停在第三十五章），2026-09-21 依 git 历史按主题补记。

### 阶段 1.5 收口：llms.txt 引擎 + WebMCP polyfill 试验 + 快照无障碍语义（da360ba）
llms.txt 引擎竣工；WebMCP polyfill 试验（默认关，只捕获声明不伪造执行）；内容快照加无障碍语义。
**踩坑**：引擎"幽灵符号"（凭空编的函数名）+ 碎片化打补丁 → 编译错误成堆时先通读整段、一次重写。

### 阶段 1.6–1.9：Agent 执行层增强
- **1.6 多步任务编排**（af60328）：`browser_action_plan` 整体确认 + 按序执行。
- **1.7 AI 友好度检测**（590a9fd）：对齐 Lighthouse Agentic Browsing 四信号。
- **1.8 动作回放/重试**（d77ab54）：多步任务可信审计 + 断点续跑地基。
- **1.9 整批重试**（953f6e0）：多步编排的批量失败恢复。
- 修复：补注册 `browser_action_plan` 工具（f28e94c，多步编排断点）。

### 阶段 2 隐私主权（骨架）
- **隐私防护自测面板**（48de317 + 9365af3）："信任需要证明"；非网页标签容错。
- **GPC 站点声明感知**（3bb7890）：`/.well-known/gpc.json`，从"发了信号"到"可验证站点认不认"。
- **零遥测审计**（999ba21）：全量网络出口清单 + 可验证"无外联"。
- **反指纹 2.0 双模式**（d475535）：加噪（现）/ 统一化（Tor/Mullvad 派）。

---

## 第四十四章（阶段 2）：密码库主密码（数据主权底线）— 6e152fe

**背景一句话**：密码库主密钥原先靠机器自动生成的 `master.key`（同机拿到即可解密）；本批补上**用户主密码**：
主密码经 PBKDF2-HMAC-SHA256（随机 salt，200k 轮）派生包裹密钥，用它 AES-256-GCM 加密"真正的主密钥"存
`vault.key`（600）。设主密码后密码库启动即**锁定**，须解锁后主密钥才在内存可用——同机拿到文件也解不开。

**交付**：`db/vault.rs`（`is_protected`/`is_unlocked`/`unlocked_key`/`lock`/`derive_key`/`wrap_key`/`unwrap_key`
+ 5 条命令）；前端隐私分区"密码库主密码"块。

**验证**：108 测试（+5 vault）；零警告；冒烟 12/12。

---

## 第四十五章（阶段 2）：上云 E2E 加密同步（服务器零知识）— 4bb2e04

**背景一句话**：密码库加密推到自建 Nextcloud，**服务器只存密文**。复用第44章主密码闸门：未设/未解锁一律拒绝上云。

**交付**：`db/sync.rs`（WebDAV 客户端 + 主钥整包 AES-256-GCM 的 E2E 包；凭据主钥包裹落盘 `sync.key` 600；
拉取按 domain+username UPSERT 合并、不删本地独有）；5 条命令 + 前端卡片。

**踩坑**：**凭记忆写"幽灵 API"复发**（`Db::app_data_dir` 不存在、自造 `write_atomic_060` → 14 个 E0433）+
目录幻觉 + 长文件 write 截断。铁律：写跨模块调用前先 grep 真身签名；长文件分块写。

**验证**：cargo 0/0；108 测试；前端构建；冒烟 12/12；**真 Nextcloud WebDAV** MKCOL/PUT/GET/DELETE 实测通过。

---

## 第四十六章（阶段 3.1）：命令栏 Ctrl+K 万能入口 — 83f4661

**背景一句话**：一个输入框统一入口（命令 / 切标签 / 搜历史 / 开网址 / 问 AI）。

**交付**：`CommandPalette.vue`（五类候选 + 键盘导航 + 打开时隐藏 webview）；**Ctrl+K 双通道**
（前端 keydown + GTK 层转发 0x6B）；「问 AI」走 `aiStore.pendingPrompt`。

**验证**：cargo 0/0；108 测试；前端构建；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第四十七章（阶段 3.1）：垂直标签栏 — 1fb9fd7 + 修复 91de7b0

**背景一句话**：标签栏可切到左侧竖排（默认关）；`ContentGeometry` 加 `left`，`top_offset()` 随模式变。

**踩坑（已记 learnings）**：①开关放"外观"而默认开"通用"→找不到（735d770）；②**新建标签没应用几何定位**
（初始化同步时 0 标签空转）→ webview 落在 (0,116) 盖住左栏；③**`margin_start` 把 Fixed 压成 1×1** →
改用 GtkFixed 内部 `move_(left,0)`，实测 `wv_toplevel_pos=(220,72)` 对齐；④窗口按钮随 TabBar 隐藏 → 补到竖栏。

**验证**：cargo 0/0；112 测试（+4 几何单测）；前端构建；冒烟 12/12。

---

## 第四十八章（阶段 3.1）：分屏浏览 — 2d990b8 + 修复 2b20200

**背景一句话**：标签可"在右侧分屏"，内容区左右对半，两个 webview 同时可见可交互。

**交付**：`ContentGeometry::split()`（+2 单测）；`split_tab` 状态 + `apply_split_layout` + 3 条命令；
switch_tab/resize 感知分屏；TabBar 右键菜单「在右侧分屏」。

**踩坑**：**右键菜单关闭时"恢复全部 webview"覆盖分屏** → 分屏时改走 `apply_split_layout`。

**验证**：cargo 0/0；114 测试；前端构建；冒烟 12/12；用户实测可用。

---

## 第四十九章（阶段 3.2）：空间 Spaces — 282ee20

**背景一句话**：工作区升级为"上下文隔离容器"（每空间独立标签集 + 主题色）。

**交付**：迁移 v12 `space_color`；`Workspace.color`；`SpaceSwitcher.vue`（切换/新建/重命名/删除，下拉隐藏 webview）；
App.vue 监听空间色 → 全局 `--accent`。

**验证**：cargo 0/0；114 测试；前端构建；vitest 2/2；eslint 干净；冒烟 12/12；迁移 v12 完成。

---

## 第五十章（阶段 3.2）：扩展体系能力 spike — 3e27564

**背景一句话**：真扩展体系前置——引擎能否**网络层拦截**？只读实证（GIR + 系统库符号）。

**结论**：**可行**，走「Web 进程扩展」机制（独立 .so + C FFI），**非 chrome.\***：
`WebKitWebPage::send-request`（请求出网前可改 `WebKitURIRequest`、返回 TRUE 取消，不含响应体改写）；
加载靠 `.so` 导出 `webkit_web_extension_initialize` + UI 侧 `set_web_extensions_directory`；
2.52 新增 `WebExtension` 类只是消息通道骨架；gtk-rs 未覆盖 → 需裸 FFI。
报告：`docs/spike-extension-capabilities-2026-09-21.md`。

---

## 第五十一章（阶段 3.2）：Web 进程扩展最小原型（阶段 A）— 53e4160

**背景一句话**：把 spike 落成能跑的东西——独立扩展 .so 端到端证明网络层拦截可用。

**交付**：`src-tauri/web-extension/`（Rust `cdylib` + 裸 FFI，无第三方依赖）；`scripts/build-extension.sh`；
复用 `lib.rs` 早已设置的 `set_web_extensions_directory`（指向 exe 目录）。

**运行实证**：`Web 进程扩展已加载` → `page-created` → `send-request: https://…`（每个请求）→
`拦截（取消）: …`（返回 TRUE 生效）。

---

## 第五十二章（阶段 3.2）：网络层按规则文件拦截（阶段 B 最小版）— 821c2e9

**背景一句话**：把硬编码标记升级为可配置规则：扩展读
`~/.local/share/com.shiftbrowser.app/shift-ext-rules.txt`（每行 URL 子串，`#` 注释），命中即取消。

**交付**：`web-extension/src/lib.rs` 用 `OnceLock` 缓存规则；`should_block()`；未命中不再逐条打日志。

**运行实证**：`已加载 1 条规则（…/shift-ext-rules.txt）` + `拦截（取消）: …/ds.js`。

**遗留**：规则文件由主程序写入（同步广告词库）；请求头改写（需 SoupHeaders FFI）；.so 生产打包。

---

## 第五十三章（工程）：HANDOFF 补记 36–52 章 — bad2518

**背景一句话**：HANDOFF 一度停在第三十五章（欠 17 章），本批依 git 历史补齐。

**交付**：36–43 章按主题补记（阶段 1.5 收口 / 阶段 1.6–1.9 Agent / 阶段 2 骨架）；
44–52 章逐章记录（背景/交付/踩坑/验证）；头部「最后更新」「当前进度」同步真实状态。

---

## 第五十四章（阶段 3.2）：打通「主程序 → 扩展规则文件」— 9f01737

**背景一句话**：设置里的网络层拦截规则自动落盘到扩展规则文件（DB → 文件 → 扩展 → 拦截）。

**交付**：`net_block_rules` 白名单键 + 命令 `sync_ext_rules`（读设置写 `shift-ext-rules.txt`）；
设置隐私分区加「网络层拦截规则」文本框；启动时同步。

**运行实证**：DB 设 `baidustatic` → `已写网络层拦截规则文件` → 扩展 `已加载 1 条规则` →
`拦截（取消）: …/ds.js`。

---

## 第五十五章（阶段 3.2）：规则热更新 — ae3323f

**背景一句话**：改规则免重启——扩展后台守护线程每 2 秒查规则文件 mtime 重载。

**交付**：`RULES` 由 `OnceLock<Vec>` 改 `OnceLock<RwLock<Vec>>`；`file_mtime_ns` / `reload_rules` /
`spawn_rules_watcher`；文案由"重启生效"改"约 2 秒自动生效"。

**运行实证**：改文件后不重启，日志从"已加载 1 条规则"变"已加载 2 条规则"。

---

## 第五十六章（阶段 3.2）：网络层 GPC 请求头 — 4407303

**背景一句话**：GPC 原先只注入 JS 属性、**缺 `Sec-GPC` 请求头**；本批在网络层补上（W3C GPC 里请求头才是站点认不认的关键）。

**交付**：FFI `webkit_uri_request_get_http_headers` + `soup_message_headers_replace/get_one`；
`apply_gpc_header()` 给每个出网请求注入 `Sec-GPC: 1`（首次读回校验）。

**运行实证**：`已注入 Sec-GPC: 1（网络层 GPC）`。

---

## 第五十七章（阶段 3.2）：可配置请求头注入 — a23ff82

**背景一句话**：把"头改写"做成可配置（每行 `Name: Value`，如 DNT / 自定义 UA）。

**交付**：`net_headers` 白名单键；`sync_ext_rules` 同时写 `shift-ext-headers.txt`；
扩展 `reload_headers`（热更新）+ `apply_custom_headers`；设置加「自定义请求头」文本框。

**运行实证**：DB 设 `DNT: 1` → `已写自定义请求头文件` → 扩展 `已加载 1 条自定义请求头`。

---

## 第五十八章（阶段 3.2）：用户脚本生态（Tampermonkey 风格）— c64b6bd

**背景一句话**：用户可添加/编辑/启停/删除用户脚本，按 matches 匹配页面注入 JS。

**交付**：迁移 v13 `userscripts`；`db/userscripts.rs`（CRUD + 注入缓存）；`inject_extensions`
搭车注入（按 matches 作 allow_list）；前端 `UserscriptsDialog.vue` + 地址栏 📜 按钮 + 命令栏命令。

**踩坑（重要）**：首版在注入路径里 `block_on` 查 DB → **panic**（inject 跑在 tokio 上下文，
"cannot start a runtime from within a runtime"）。**改成同步注入缓存**（CRUD 后 + 启动时刷新）。

**运行实证**：插入 `console.log('[userscript-test] …')` 后重启，日志出现 `CONSOLE LOG [userscript-test] …`。

---

## 第五十九章（渲染修复）：禁用 WebKitGTK DMA-BUF 渲染器 — b844b08

**背景一句话**：看视频时画面闪烁（历史遗留，用户一直没说）。

**根因**：WebKitGTK 2.42+ 默认走 **DMA-BUF 渲染路径**，在部分核显（本机 Intel HD 630 / i915）
上与驱动配合不良 → 网页/视频闪烁（社区公认问题）。

**修复**：`run()` 最前面（WebKit 初始化前）设 `WEBKIT_DISABLE_DMABUF_RENDERER=1`，回退共享内存渲染；
可用环境变量覆盖（设 0 保留 DMA-BUF）。

**验证**：WebKit 子进程环境确含该变量（注意父进程 `/proc/<pid>/environ` 是启动快照，要查子进程）；
用户实测**不再闪烁**。

**诊断方法论**：先隔离最近改动（临时停扩展/脚本）→ 确认历史遗留 → 查渲染路径/驱动（env 开关）。

---

## 第六十章（阶段 2 收尾）：Passkeys 运行时实测 — 结论：不可用

**背景一句话**：阶段 2 最后一块——WebKitGTK 能否用 Passkeys？用用户脚本探针在真页面实测。

**方法**：借第 58 章的用户脚本能力，注入探针查 `PublicKeyCredential` 与平台认证器。

**结论**：`PublicKeyCredential=false`（JS 层**未暴露** WebAuthn）→ **Passkeys 不可用**，
按战略预案**降级为「密码管理器增强」**（strategy 风险登记第 2 项已更新为 ✅ 实测完成）。

---

## 第六十一章（阶段 3 自动化）：标签批量操作 — 40cd41a

**背景一句话**：批量管理标签——关同域名、关重复、全部静音。

**交付**：tabs store 加 `closeSameDomain` / `closeDuplicates` / `toggleMuteAll`；
TabBar 右键菜单 + 命令栏（Ctrl+K）各加三项，App.vue 统一处理并 toast 反馈。

**验证**：前端 98 模块构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第六十二章（阶段 3 自动化）：定时任务 — c8d1cdb

**背景一句话**：设置驱动的后台调度（定时清理历史 / 定时刷新订阅）。

**交付**：`auto_tasks.rs`（启动起 tokio 任务，每 60s 检查到期；到期判定用进程内 `last_run`，
重启后重新计时）；设置白名单 + 通用分区两个数字输入框（0=关）。

**运行实证**：设 `auto_refresh_feeds_minutes=1` → 日志 `[auto] 定时刷新订阅：1014 个源`。

**验证**：cargo 0错0警告；114 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第六十三章（阶段 3 自动化）：Agent 编排 — f40bfdc

**背景一句话**：把调度器与 AI 打通——到点自动跑 Agent 提示词（阶段 3 收官）。

**交付**：`auto_tasks.rs` 加第三类任务（`auto_agent_minutes` + `auto_agent_prompt`，到点
`emit("auto-agent", prompt)`）；App.vue 监听 → 打开 AI 侧栏 + 发提示词；
AI 的多步动作仍走**原有确认流程**（不绕过安全闸门）。

**运行实证**：设 1 分钟 + 提示词 → `[auto] 触发 Agent 提示词（每 1 分钟）` +
`set_webview_ai_panel: active=true` + AI 调用发出（provider 429 限流属外部）。

**验证**：cargo 0错0警告；114 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 🎯 阶段 3 收官总结（2026-09-21）

| 板块 | 交付 |
|---|---|
| 3.1 交互形态 | 命令栏 Ctrl+K、垂直标签栏、分屏浏览 |
| 3.2 平台化 | 空间 Spaces、真扩展体系（网络层拦截/头改写/热更新）、用户脚本生态 |
| 3.3 自动化/工作流 | 标签批量操作、定时任务、Agent 编排 |

**至此战略路线图 阶段 0/1/2/3 全部完成**（阶段 4「开放生态」为长期项）。

- 期间另修：视频/网页画面闪烁（WebKitGTK DMA-BUF 渲染器 + Intel 核显，第 59 章）。
- 工程资产：`docs/spike-extension-capabilities-2026-09-21.md`（扩展体系能力实证 + 网络层原型）。

---

## 第六十四章（阶段 3 工程收尾）：扩展 .so 进生产打包 — e7525e0

**背景一句话**：让正式版也带扩展——打包流程纳入扩展 .so。

**交付**：`scripts/build-extension.sh` 同时拷 `target/release/`（开发运行，web extensions 目录）
与 `src-tauri/resources/`（生产打包）；`tauri.conf.json` 加 `bundle.resources` +
`beforeBuildCommand`（打包时先构建扩展）；`lib.rs` 加打包兜底（exe 目录没 .so 时从资源目录复制）；
`.gitignore` 只排除生成的 .so（保留已有 feeds.json / worldnav.json）。

**验证**：脚本生成两处 .so；JSON 合法；cargo 0错0警告；114 测试；冒烟 12/12；
dev 下兜底未误触发、扩展仍正常加载。
**说明**：未跑完整 `tauri build`（bundler 依赖/耗时）——逻辑成立但未实包验证。

---

## 第六十五章（隐私）：GPC 请求头开关化 — 9c25ba3

**背景一句话**：把常开的 `Sec-GPC: 1` 做成可配置。

**交付**：GPC 从扩展硬编码改为「主程序按设置写进自定义请求头文件」——一处配置、单一来源。
扩展移除硬编码；`settings.rs` 白名单加 `gpc_header`，写文件时非 `false` 才注入；
设置隐私分区加开关。

**运行实证**：开 → 文件含 `Sec-GPC: 1` + `DNT: 1`；关 → 只剩 `DNT: 1`。

**验证**：扩展 0警告；cargo 0错0警告；114 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第六十六章（阶段 4 开放生态）：本地开放 API — 7768d74

**背景一句话**：让外部脚本/工具驱动浏览器（阶段 4 起步）。

**交付**：`src-tauri/src/api.rs`（Unix 域套接字 + 行式 JSON；套接字权限 0600 只限本用户）；
命令 `status` / `list_tabs` / `open_url`；`tabs.rs::all_tabs` 提为 pub；`docs/local-api.md`。

**运行实证**：套接字 `srw-------`；三命令均 `ok:true`，`open_url` 真开 example.com。

**验证**：cargo 0错0警告；114 测试；冒烟 12/12。

---

## 第六十七章（阶段 4 开放生态）：开放 API 扩命令 — 71e6cd6

**交付**：新增 `active_tab` / `close_tab` / `switch_tab` / `reload_tab` / `navigate`；
文档补命令表 + 用法（先 list_tabs/active_tab 拿 id 再传写命令）。

**运行实证**：全流程真调——open_url → active_tab → switch_tab → reload_tab → navigate →
list_tabs（3 标签）→ close_tab 均 `ok:true`；未知命令返回 `ok:false` + 错误。

**验证**：cargo 0错0警告；114 测试；冒烟 12/12。

---

## 第六十八章（修复）：翻译在带 CSP 站点失效 — 081ce40 + f34d681

**背景一句话**：用户报"翻译功能不能用了"（带 CSP 的站点上整页/划词翻译没反应）。

**根因**：翻译脚本用 `img.src = 'shift://__translate__/…'` 调后端，**受站点 CSP 的 img-src 约束**；
主流站点普遍带 CSP → 浏览器在页面层直接拒绝，请求没到后端（日志里连一次翻译 API 调用都没有）。

**修复**：改用 **WebKit 原生脚本消息处理器**（CSP 免疫）：
- `lib.rs` 新增 `native_translate_handle`（复用 translate::* + write_translate_result，
  支持 /text /batch /whole-page /progress，保留限长/限频/callback 白名单）。
- `extensions.rs` 在 child webview 注册处理器 + 连回调（按 label 去重）。
- 两个翻译 JS 加 `__sbTrSend`：**原生桥优先、无桥回退 img**（双通道）。

**二次坑**：换桥后仍"没反应"——**path 规范化**（旧通道前缀自带斜杠，JS 传的 path 不带，
新 handler 按 `/whole-page` 匹配 → 全落"未知路径"静默丢弃）。f34d681 修：缺斜杠就补 + 加链路日志。

**运行实证**：`原生桥已注册`（每 child webview）；端到端 `result={"text":"你好，世界"}`；
整页链路 `整页翻译请求` → `批量翻译请求 bytes=482`；用户确认修复。

**验证**：cargo 0错0警告；114 测试；冒烟 12/12。

---

## 第六十九章（阶段 4 多端协同）：E2E 同步扩展——书签跨端 — 6b2cf92

**背景一句话**：E2E 加密同步从"只同步密码库"扩展到"密码库 + 书签"。

**交付**：sync.rs 导出 payload 升 version 2（加 `bookmarks`，folder 名经 LEFT JOIN 解析）；
导入书签按 **url 去重合并**（本地已有同 url 则跳过、不覆盖本地），文件夹按名字 **find-or-create**；
文案/前端卡片更新；新增单测（合并 + find-or-create + 重复导入幂等 + 旧版包兼容）。

**验证**：cargo 0错0警告；115 测试（+1）；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第七十章（阶段 4 多端协同）：E2E 同步再加会话（标签集）— 179fe25

**背景一句话**：会话（标签集）也跨端——"接着上次的标签继续"。

**交付**：payload 升 version 3（加 `sessions`：name/data/created_at，最多 50 条）；
导入按 **(name, created_at)** 去重（workspace 不跨端，置 NULL）；单测扩展。

**验证**：cargo 0错0警告；115 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第七十一章（阶段 4 多端协同）：E2E 同步再加历史 — fc0d415

**背景一句话**：历史也跨端（最近 300 条）。

**交付**：payload 升 version 4（加 `history`：url/title/visit_count/last_visit_at）；
导入按 **url UPSERT**——visit_count 取最大、last_visit_at 取最新、标题不覆盖本地；单测扩展。

**至此多端协同数据层齐了**：密码库 / 书签 / 会话 / 历史（同一把主密钥、同一零知识通道）。

**验证**：cargo 0错0警告；115 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第七十二章（阶段 4 自托管服务端套件）：本地 HTTP API — 1e64d88

**背景一句话**：开放 API 除 Unix 套接字外，再提供 HTTP 形态（自托管套件最小版）。

**交付**：`POST http://127.0.0.1:19701/api`，body 为同款 JSON 命令，**复用同一套 `handle()`**；
**只绑 127.0.0.1**（不对外）；鉴权 `Authorization: Bearer <token>`（随机生成、落 `api-token` 0600）。
文档补「两种形态」表 + curl 示例 + 局域网暴露提醒。

**运行实证**：`HTTP API 已启动`；token 文件 `-rw-------`；不带 token → 未授权；
带 token → `{"ok":true,"data":{"tabs":3,…}}`；`open_url` 成功。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

---

## 第七十三章（阶段 4）：开放 API 再加 search / new_tab — 69e8f63

**交付**：`search {query}`（读 search_engine 设置拼 URL，与 newtab/AddressBar 同映射）、
`new_tab`（打开内置新标签页）；附最小 percent-encode（不引额外依赖）。

**踩坑**：Rust 侧 `create_tab` 需绝对 URL——传 `"newtab"` 报
`URL 解析失败: relative URL without a base`，改用 `shift://newtab`。

**运行实证**：`search "山西 永济 天气"` → `create_tab: url=https://www.bing.com/search?q=%E5%B1%B1…`；
`new_tab` → `create_tab: url=shift://newtab`。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

**开放 API 现状**：10 命令（status/list_tabs/active_tab/open_url/close_tab/switch_tab/reload_tab/
navigate/search/new_tab），**Unix 套接字 + HTTP（带 token）双形态**。

---

## 第七十四章（阶段 4）：开放 API 再加 history / bookmarks 只读命令 — bfff839

**交付**：`history {limit}`（默认 50、上限 500）、`bookmarks`（LEFT JOIN 出文件夹名）；
文档命令表补两行。

**运行实证**：`history {"limit":3}` → 3 条（含 cn.bing.com 搜索页）；`bookmarks` → 列表（含 folder 名）。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

**开放 API 现状（12 命令）**：读 `status`/`list_tabs`/`active_tab`/`history`/`bookmarks`；
写 `open_url`/`close_tab`/`switch_tab`/`reload_tab`/`navigate`/`search`/`new_tab`；
Unix 套接字 + HTTP（带 token）双形态。

---

## 第七十五章（阶段 4）：开放 API 加 screenshot 截图命令 — b369697

**交付**：`screenshot` —— 截当前活动标签可见区 → PNG 落 `<app_data>/screenshots/`，返回路径。
Cargo.toml 加 `cairo-rs`（features=["png"]）；GTK 对象非 Send → GTK 主上下文 `spawn_local`
跑快照、结果经 tokio oneshot 回传。

**踩坑**：`write_to_png` 在 `impl Surface` 但**受 cairo-rs 的 png feature 门控**（不开报 "no method"）；
且它收 **Write 流**（不是路径）。

**运行实证**：`screenshot` → `{"ok":true,"data":{"path":"…/shot-….png"}}`；
`file` 确认 `PNG image data, 1280 x 662, 8-bit/color RGB`（18.6KB）。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

**开放 API 现状（13 命令）**：读 status/list_tabs/active_tab/history/bookmarks；
写 open_url/close_tab/switch_tab/reload_tab/navigate/search/new_tab；媒体 screenshot。

---

## 第七十六章（阶段 4）：开放 API 加「执行 JS」（带安全闸门）— 9a0e454

**背景一句话**：开放 API 的最强能力——在活动标签执行任意 JS（读页面/驱动交互）。

**安全闸门**（关键）：设置项 `api_allow_eval`（**默认关**）未开时命令直接拒绝；
设置 → 隐私 → 「允许 API 执行 JS（危险）」显式开启；另有限长（≤100KB）；
双形态鉴权兜底（Unix 0600 / HTTP Bearer token）。

**实现**：api.rs `eval {code}` → 取活动标签 webview → `engine::webview::eval_js_with`
（带结果回调）→ tokio oneshot 回传。

**运行实证**：禁用态 → `{"ok":false,"error":"执行 JS 已禁用…"}`；
开闸 → `{"ok":true,"data":{"result":"Example Domain | example.com"}}`。

**验证**：cargo 0错0警告；115 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

**开放 API 现状（14 命令）**：读 status/list_tabs/active_tab/history/bookmarks；
写 open_url/close_tab/switch_tab/reload_tab/navigate/search/new_tab；
媒体 screenshot；执行 eval（默认关）。

---

## 第七十七章（阶段 4）：开放 API HTTP 端口可配 + 可选局域网绑定 — fa00395

**背景一句话**：让开放 API 能按需换端口、按需（显式）对局域网开放。

**交付**：设置项 `api_http_port`（默认 19701）、`api_bind_lan`（默认关，只绑 127.0.0.1）；
api.rs 启动时读两设置决定 bind 地址（开局域网则 0.0.0.0 + `log::warn` 提醒保管 token）；
设置 → 隐私加端口数字框 + 局域网开关；文档补说明。改完需重启生效。

**运行实证**：端口设 19702 → 重启后 `19702` 通（`{"ok":true,"data":{"tabs":2,…}}`）、`19701` 拒连。

**验证**：cargo 0错0警告；115 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

---

## 第七十八章（阶段 4 自托管套件）：工具网关 — dcc81ea

**交付**：API 加 `tools`（列可用 MCP 工具）、`tool_call {name, arguments}`（调工具）；
直接复用 `engine::mcp::{list_tools, call_tool}`，调用受 MCP server 的 `allowed_tools` 白名单约束。

**运行实证**：`tools` → `{"ok":true,"data":[]}`（未配 MCP server）；未知工具 → 明确报错。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

---

## 第七十九章（阶段 4 自托管套件）：本地状态面板 — 77ed555

**交付**：浏览器打开 `http://127.0.0.1:19701/?token=<t>` → 状态面板（版本/标签数/活动标签/
命令清单/用法）。HTTP 非 /api 路径改返回 HTML；面板需 token（查询串，浏览器不便发 Bearer 头）。

**运行实证**：不带 token → 提示页；带 token → 面板（`版本：5.6.0`、`标签数：2`）。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

---

## 第八十章（阶段 4 自托管套件）：模型网关 — abb0a02

**背景一句话**：本地 OpenAI 兼容端点，让其它程序用同一模型（套件最后一块）。

**交付**：`POST /v1/chat/completions`（Bearer token）→ 转发到设置里的 provider
（base_url/key/model；未指定 model 则注入设置里的；key 空回退 `AGNES_API_KEY`）。
**默认关**（`api_model_gateway`，消耗额度）；设置加开关；文档补小节 + curl 示例。

**运行实证**：禁用态 → 明确报错；无 token → 未授权；开闸 → OpenAI 兼容响应
（`{"choices":[{"message":{"content":"…收到"}}]}`）。

**验证**：cargo 0错0警告；115 测试；前端构建过；vitest 2/2；eslint 干净；冒烟 12/12。

**🎯 自托管服务端套件四块齐**：同步（Nextcloud E2E）/ 工具网关（MCP）/ 管理页（状态面板）/ 模型网关。

---

## 第八十一章（实战检验）：下载落盘目录修复 — 16c8f10

**背景一句话**：高哥实测报「下载不能开始 / 找不到文件」。

**排查结论**（两个问题叠加）：
1. APK 链接被当网页加载 → 导航失败（`帧加载已中断`）。已修（3bc91ab）：接 `decide-policy`，
   遇到不可显示的 MIME 转下载、不再导航；目的地 URI 改用 `url::Url::from_file_path`
   （中文/空格路径手拼 `file://…` 非法会被取消）。
2. **文件其实下成功了，只是落错地方**：应用硬编码 `~/Downloads`，而中文系统 XDG 下载目录
   是 `~/下载`（`XDG_DOWNLOAD_DIR="$HOME/下载"`）→ 文件落 `~/下载`，应用在 `~/Downloads` 找，
   用户找不到。

**交付**：`tabs.rs` 下载目的地改用 `glib::user_special_dir(UserDirectory::Downloads)` 读系统配置，
缺失时回退 `~/Downloads`。

**运行实证**：测试 APK 落 `~/下载/dogverify.apk`（43 字节 completed），DB `path` 一致；
旧 `ToSAF.apk` 记录路径一并修正。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

---

## 第八十二章（实战检验）：系统托盘 — f69d6ab / c583b24

**背景一句话**：高哥问「系统托盘怎么没有图标？」——排查确认应用**从未实现托盘**（非故障）。

**实现**（ksni 0.3.6，纯 Rust D-Bus SNI，零 C 依赖；宿主 = GNOME `ubuntu-appindicators` 扩展）：
- **关闭窗口 = 缩托盘**（拦截主窗口 `delete-event`，`hide` + 阻止关闭，不退出）
- 托盘左键单击 = 显示/隐藏主窗口
- 菜单四项：显示/隐藏窗口 · 下载管理 · 密码库 · 分隔线 · 退出
- 真退出走前端退出按钮或托盘「退出」（置 `REAL_QUIT` 放行 delete-event）
- 托盘图标 = 应用自己的图标（编译期嵌入 **512 主源图 `icon.png`**，gdk-pixbuf 解码缩放出
  16/22/32 三尺寸，转 ARGB32 network byte order）

**线程模型**（关键）：ksni 菜单闭包跑在 D-Bus 线程，只往 mpsc 投动作；
GTK 主线程 glib 定时器（120ms）排空执行——**绝不跨线程碰 GTK**。

**接线**：`lib.rs` setup 起托盘服务 + delete-event 拦截 + 动作排空；
`tabs.rs` `close_window` 置 `REAL_QUIT`；`App.vue` 监听 `tray-open-downloads/passwords`。

**运行实证**：D-Bus 实测 SNI 注册成功；`IconPixmap` 返回 16/22/32 三尺寸；
菜单布局读回四项正确（显示/隐藏窗口 · 下载管理 · 密码库 · separator · 退出）。

**人工实测（高哥，2026-09-21）**：六项全部正常 —— 左键点击显隐窗口 / 菜单「下载管理」
「密码库」弹出面板 / 点窗口 X 缩托盘不退出 / 托盘「退出」真退出。托盘功能验收通过。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。

---

## 第八十三章（阶段 4）：开放 API 扩展 — 更多命令 + 鉴权细化

**背景一句话**：给本地开放 API 补常用命令，并加"只读模式 / IP 白名单"两层鉴权。

**交付 · 更多命令**（12 个，命令总数 16 → 28）：
- **只读 8 个**：`list_downloads` · `list_sessions` · `list_groups` · `list_spaces` ·
  `list_userscripts` · `search_history`（query 必填）· `get_setting`（敏感键返回 null）·
  `list_settings`（复用 `get_all_settings`，已过滤敏感键）
- **操作 4 个**：`pin_tab` · `mute_tab` · `minimize_window` · `maximize_window`

**交付 · 鉴权细化**：
- **只读模式**（设置 `api_readonly`）：`is_write_cmd()` 列出 13 个写命令，命中即查设置，
  为 true 时统一拒绝（`ok:false`）——给"只读脚本"场景兜底。
- **IP 白名单**（设置 `api_allowed_ips`）：HTTP 形态绑 `0.0.0.0` 时按来源 IP 过滤
  （逗号分隔、前缀匹配，空=不限）；非白名单连接直接拒 + 日志留痕。接入点是
  `listener.accept()` 的 peer 地址（此前被丢弃）。

**接线**：`api.rs`（命令分发 + `is_write_cmd` + `ip_allowed` + 状态面板命令清单）；
`db/settings.rs` 的 `ALLOWED_KEYS` 登记 `api_readonly` / `api_allowed_ips`。

**运行实证**：12 个新命令全部 `ok:true`（下载/会话/空间/设置/搜索均取真实数据；
pin/mute/maximize 生效）；只读模式下 `open_url` 被拒、`status` 正常；
IP 白名单设 `1.2.3.4` 时本机被拒（日志 `拒绝非白名单 IP: 127.0.0.1`），设 `127.0.0.1` 正常。

**验证**：cargo 0错0警告；115 测试；冒烟 12/12。文档 `docs/local-api.md` 同步（命令表 + 安全边界）。

---

## 下一步（截至第八十三章）

- **阶段 4 继续**：多端形态（移动/平板协同）——路线图剩的最后一块大件。
- **实战检验（进行中）**：下载目录、系统托盘已修并验收；开放 API 扩展（C2）已交付。
- **工程**：HANDOFF 逐章记录机制（已补至 83 章，后续每章随提交更新）。


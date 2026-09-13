# ShiftBrowser v5.0.0（Tauri 重构）交接文档

> 最后更新：2026-09-12（广告拦截自定义关键词 ✅：设置里可手工加词 + CSS 规则扩充，误杀选择器已收紧）
> 交接人：王二狗 → 下任 agent

---

## 一、项目概况

**目标**：将 Electron 版 ShiftBrowser（v4.5.0，`/home/gao/ShiftBrowser/`）重构为 **Tauri 2.0 + Rust + Vue 3 + Tailwind 4 + Pinia + TypeScript + SQLite(sqlx)**。

**重构项目位置**：`/home/gao/shift-browser-tauri/`（独立目录，和 Electron 版分开）

**路线图**：`/home/gao/ShiftBrowser/doc/UPGRADE-ROADMAP.md`（v2.0，含 Phase 0-4 完整规划）

**当前进度**：**Phase 0 完成**（环境搭建+核心骨架+数据库链路验证通过），进入 Phase 1（核心浏览器框架）。

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

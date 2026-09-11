# ShiftBrowser v5.0.0（Tauri 重构）交接文档

> 最后更新：2026-09-11（Phase 1 多标签核心功能已跑通 ✅）
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
5. **sudo 密码**：`***REMOVED***`（仅 Electron 版部署用，Tauri 开发不需要）

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

### 8.2 待办（下一步）

1. **P1**：密码管理 UI（保存/查看/删除密码的界面）
2. **P1**：下载管理 UI（下载记录列表/进度展示）
3. **P1**：标签分组/工作区 UI（对应 Rust 命令已就绪）
4. **P2**：窗口 resize 时同步 Fixed 内 webview 尺寸（当前硬编码 1280x800）
5. **P2**：初始化 git 仓库

### 8.3 测试环境注意事项

- **Wayland**：应用必须 `GDK_BACKEND=x11` 启动才有窗口；`ffmpeg x11grab` 截屏全黑，要用 `import -window <wid>`；窗口位置会漂移，xdotool 用 `mousemove --window <wid>` 相对坐标
- **启动**：先 vite（5173）再 `(setsid env GDK_BACKEND=x11 ./src-tauri/target/debug/shift-browser > /tmp/shift-browser.log 2>&1 < /dev/null &)`
- **日志**：Rust 命令有 `[tabs]` 前缀 println（create/close/switch/navigate 都打日志），验证交互看 `/tmp/shift-browser.log`
- **⚠️ IME 干扰**：系统有中文输入法时，`xdotool type` 输入的 ASCII 会被 IME 当拼音转换（如 "baidu" → "豆包"），导致地址栏输入测试不可靠。绕过方式：`xclip` 设置剪贴板 + `ctrl+v` 粘贴，或直接改前端默认 URL 验证
- **OCR 验证**：`import -window <wid>` 截图 + `tesseract` OCR 可识别界面文字（标签标题、输入框内容）；裁剪区域放大 3-4 倍 + `ImageOps.autocontrast` 提高识别率

---

## 九、注意事项

- **禁用语**：永远不要说"中不中"（高哥明确禁止）
- **语言**：所有输出用中文（代码/变量名除外）
- **安全**：sudo 密码、API key、数据库密码不向输出透露
- **测试**：Wayland 下截图受限，验证窗口效果主要靠日志 + 进程状态
- **Git**：Tauri 项目目前没有 git 仓库（Electron 版有），可以考虑初始化

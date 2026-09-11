# Errors

Command failures and integration errors.

---

## 2026-09-11 18:20 — sqlx 新建 SQLite 数据库报 CANTOPEN (code 14)

**类别**: correction
**场景**: ShiftBrowser Tauri 重构，`Db::init()` 用 sqlx 0.8 连接文件数据库
**错误**: `SqlitePoolOptions::connect("sqlite:///path/db")` 报 `(code: 14) unable to open database file`，数据库文件未创建
**根因**: sqlx 的 `SqliteConnectOptions::create_if_missing` **默认是 false**！默认 flags 只有 `SQLITE_OPEN_READWRITE`（没有 `SQLITE_OPEN_CREATE`），打开不存在的文件时 SQLite 直接返回 CANTOPEN。内存库（`sqlite::memory:`）不受影响，所以"内存库能开、文件库全挂"是典型特征。
**正确**: 
```rust
let options = SqliteConnectOptions::new()
  .filename(&db_path)
  .create_if_missing(true)   // 关键！
  .foreign_keys(true)
  .journal_mode(SqliteJournalMode::Wal);
let pool = SqlitePoolOptions::new().max_connections(5).connect_with(options).await?;
```
或者 URL 带 `?mode=rwc`。
**预防**: 用 sqlx 新建数据库文件时，先确认 `create_if_missing`；排查 CANTOPEN 时先查 sqlx 默认值，不要想当然"默认会创建文件"。

---

## 2026-09-11 18:20 — pkill/grep 杀进程误杀自己

**类别**: correction
**场景**: 清理 shift-browser / vite / cargo 旧进程
**错误**: `pkill -f "cargo"`、`grep "shift-browser" | xargs kill` 把**自己的 shell 进程**也杀了（命令行字符串里含关键字），导致命令被中止、工具超时
**正确**: 
- 用 `ps -eo pid,comm | grep -i 关键字`（只看进程名 comm，不看完整命令行）再取 PID kill
- 或 `pgrep -x 精确进程名`（精确匹配进程名）
- 避免用 `pkill -f` / `pgrep -f` 匹配宽泛关键字
**预防**: 任何"按关键字杀进程"的操作，先确认关键字不会出现在自己的命令行里；优先用进程名精确匹配。

---

## 2026-09-11 18:20 — 后台启动进程挂住 shell 工具

**类别**: correction
**场景**: 用 `setsid nohup npm run dev &` 后台启动 vite / cargo run
**错误**: bash 工具等待后台进程导致 120 秒超时被中止
**正确**: 
```bash
(setsid npm run dev > /tmp/vite-dev.log 2>&1 < /dev/null &)
```
用子 shell `( ... & )` 包裹 + 重定向 stdin/stdout/stderr + `setsid`，父 shell 立即返回；或分两步：先 `cargo build`（正常结束），再单独启动二进制。
**预防**: 后台启动服务类进程，永远用子 shell 包裹 + 三重重定向。

---

## 2026-09-11 19:10 — Rust 自写 UUID 生成切片越界 panic

**类别**: correction
**场景**: ShiftBrowser Tauri 重构，`tabs.rs` 里自写简单 UUID（避免引入 uuid crate）
**错误**: `format!("{:x}{:x}{:x}{:x}", r1, r2, r3, r4)` 生成的十六进制字符串**不补零**，`0x1234` 只输出 `1234`（4 字符），导致总长度不足 32，后面 `&s[20..32]` 切片越界 panic：`end byte index 32 is out of bounds for string of length 31`。panic 发生在 tokio worker 线程，命令返回 Err，前端静默失败。
**正确**: 必须用 `{:08x}` 强制补零，保证每段 8 字符、s 恰好 32 字符：
```rust
let s = format!("{:08x}{:08x}{:08x}{:08x}", r1, r2, r3, r4);
```
另外注意 UUID 第 4 段切片是 `s[15..19]`（不是 `s[16..20]`），第 3 段 `4{}` 是 1+3=4。
**预防**: 自写格式化拼接时，先确认每个片段的**最小宽度**；涉及字符串切片前先断言长度；`{:x}` 系列默认不补零，要补零必须显式 `{:08x}`。
---

## 2026-09-11 20:30 — Tauri Linux 嵌入式 webview 白色空白（GTK vbox 平分空间）

**类别**: correction
**场景**: ShiftBrowser Tauri 重构，`Window::add_child` 创建嵌入式 webview，页面从 y=400 开始，标签栏/地址栏与页面之间有 310px 白色空白
**错误**: 以为 add_child 传的 `LogicalPosition(0, 82)` / `LogicalSize(1280, 696)` 会生效。实际 Linux/GTK 后端走 `WebviewKind::WindowChild => webview_builder.build_gtk(vbox)`，webview 被 `pack_start(webview, true, true, 0)` 塞进窗口 vbox（GtkBox）。主 webview + child webview 都 expand=true → GTK 垂直平分窗口，child 落到 y=400（=800/2），中间 310px 是主 webview 残留内容区。`set_bounds()` 也无效（GTK 布局强制覆盖）。
**正确**: 把主 webview 和所有 child webview 移到 **GtkFixed 容器**（绝对定位叠层）：
- 结构：窗口 vbox → [GtkFixed] → 主 webview(0,0 全屏) + child webview(0,82 内容区)
- gtk-rs 0.18 **没有 `reparent` 方法**（被移除），用 `vbox.remove(&webview)` + `fixed.put(&webview, x, y)` 实现
- gtk 对象**非 Send**，不能存 Tauri State、不能跨线程捕获进 `with_webview` 闭包 → 所有 gtk 对象在 `with_webview` 闭包内创建/查找；vbox 从 webview 的 `parent()` 拿（`window.default_vbox()` 会发消息回主线程，在闭包内调用会死锁）
- `setup_gtk_layout()` 在 setup 阶段初始化（主线程），`fix_gtk_layout()` 处理每个 child
**预防**: Linux 上 add_child 的位置/大小参数不可信；调试 GUI 布局用 `child.position()/size()` + 截图采样验证实际渲染位置。

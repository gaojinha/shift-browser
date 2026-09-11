# Learnings

Corrections, insights, and knowledge gaps captured during development.

**Categories**: correction | insight | knowledge_gap | best_practice

---

## 2026-09-11 18:20 — ShiftBrowser Tauri 骨架全链路打通（insight）

**类别**: insight
**场景**: ShiftBrowser v5.0.0 Tauri 重构 Phase 0
**要点**: 
- 数据库 code 14 根因是 sqlx `create_if_missing` 默认 false（详见 ERRORS.md）
- 修复后 `Db::init()` 用 `SqliteConnectOptions::new().filename().create_if_missing(true).foreign_keys(true).journal_mode(Wal)` 连接池方式，绕开 URL 解析
- 前端 App.vue 的 invoke 测试全通：set_setting → get_setting → create_bookmark → get_bookmarks
- 数据库文件在 `~/.local/share/com.shiftbrowser.app/shift-browser.db`，迁移 v1 自动执行
- 启动顺序：先 vite（5173）再 shift-browser，否则 WebView 连 localhost 被拒

## 2026-09-11 18:20 — sqlx 排查方法论（best_practice）

**类别**: best_practice
**要点**: 
- 排查依赖库问题时，对比测试必须**逐项核对默认值**（flags、features、配置项），不能想当然
- 原生 libsqlite3-sys 测试成功 ≠ sqlx 成功，两者默认配置可能不同（如 create_if_missing）
- 内存库成功 + 文件库失败 = 典型"创建文件权限/flag 缺失"特征
- 改 cargo registry 源码加 DEBUG 日志排查时，touch 文件 + cargo build 可能不触发重编译（fingerprint 用内容 hash），必要时 `cargo clean -p 包名`

---

## 2026-09-11 19:30 — Tauri 2 多标签页 Webview 架构验证（insight）

**类别**: insight
**场景**: ShiftBrowser v5.0.0 Phase 1，同一窗口内多嵌入式 webview
**要点**:
- **方案确定**：混合方案 — Vue UI（TabBar/AddressBar）渲染在主 WebviewWindow，标签页用 `Window::add_child(WebviewBuilder)` 创建嵌入式 webview
- **需 `unstable` feature**：`Window::add_child` 在 tauri 2.11.5 需要 `features = ["unstable"]`（`#[cfg(all(desktop, feature = "unstable"))]`）
- **切换用 hide/show**：`webview.hide()` + `webview.show()` + `webview.set_focus()`
- **布局偏移**：webview 从 `LogicalPosition(0, TAB_BAR_HEIGHT + ADDRESS_BAR_HEIGHT)` 开始，底部扣 `STATUS_BAR_HEIGHT`
- **`close()` 销毁 webview**：Webview 没有 `destroy()` 方法，正确 API 是 `webview.close()`
- **标题同步暂缓**：Phase 1 MVP 先用 URL 作标题，后续加 `on_page_load` 监听
- **`update_tab` 异步命令必须返回 Result**：Tauri 2 的 async 命令如果参数带引用（如 `State<'_, TabManager>`）必须返回 `Result<T, E>`，否则报 `AsyncCommandMustReturnResult`

---

## 2026-09-11 19:30 — Wayland 环境下 Tauri/WebKitGTK 窗口创建（knowledge_gap）

**类别**: knowledge_gap
**场景**: Ubuntu 24.04 Wayland + XWayland 混合环境
**要点**:
- **必须 `GDK_BACKEND=x11`**：WebKitGTK 在 Wayland 原生模式下可能无法正常创建窗口（进程活着但 xdotool 找不到窗口），加 `GDK_BACKEND=x11` 环境变量强制走 XWayland 即可
- **`x11grab` 截图抓不到 Wayland 窗口**：`ffmpeg -f x11grab` 和 `import -window root` 在 Wayland 下截取的屏幕全黑，需要用 `import -window <window-id>` 直接抓特定窗口才有效
- **xdotool 相对坐标**：Wayland 窗口位置不固定（可能被合成器重定位），用 `xdotool mousemove --window <wid> <x> <y>` 指定窗口内相对坐标，不依赖屏幕绝对位置
- **启动命令**：`(setsid env GDK_BACKEND=x11 ./shift-browser > /tmp/shift-browser.log 2>&1 < /dev/null &)`

---

## 2026-09-11 19:30 — vue-tsc 与 typescript 7 不兼容（correction）

**类别**: correction
**场景**: ShiftBrowser 前端构建
**要点**:
- `vue-tsc 3.3.11` 不兼容 `typescript ^7.0.2`（TS7 是原生 tsgo，`vue-tsc` 内部依赖 `typescript/lib/tsc` 子路径，TS7 未导出该路径），构建报 `ERR_PACKAGE_PATH_NOT_EXPORTED`
- **修复**：降 typescript 到 `^5.9.3`，`npm run build`（vue-tsc + vite build）正常通过
- 也可用 `npx vite build` 跳过类型检查作为临时方案
---

## 2026-09-11 20:30 — Tauri Linux 多 webview 布局与测试技巧（knowledge_gap）

**类别**: knowledge_gap
**场景**: ShiftBrowser Tauri 重构，解决嵌入式 webview 布局 + GUI 自动化测试
**要点**:
- **Linux GTK 布局真相**：`add_child` 的 webview 走 `build_gtk(vbox)`，位置/大小参数被忽略，vbox 平分空间。绝对定位必须用 GtkFixed 容器（见 ERRORS.md）
- **`with_webview` 闭包在主线程执行**：`tauri::Webview::with_webview(|pwv| ...)` 通过 `send_user_message` 发到主线程事件循环，闭包内可直接操作 gtk 对象（安全）；但闭包要求 `Send + 'static`，不能捕获 gtk 对象，gtk 对象要在闭包内创建/查找
- **`PlatformWebview::inner()` 拿底层句柄**：Linux 上 `pwv.inner()` 返回 `webkit2gtk::WebView`（gtk::Widget 子类），可做 remove/put/set_size_request/show 等 GTK 操作
- **gtk-rs 0.18 无 `reparent`**：用 `old_container.remove(&widget)` + `new_container.put(&widget, x, y)` 模拟
- **Cargo 依赖 gtk**：`gtk = { version = "0.18", features = ["v3_24"] }`（与 tauri-runtime-wry 同版本，避免类型不匹配）
- **标题同步用轮询**：Tauri 2 内置无 `plugin:event|listen` 命令，`tauri-plugin-event` 不存在于 crates.io（事件方案走不通）；用 Rust `on_page_load` 更新 State + 前端 `setInterval` 调 `get_tabs` 轮询
- **⚠️ xdotool 输入被 IME 干扰**：系统中文输入法会把 `xdotool type` 的 ASCII 当拼音转换（"baidu" → "豆包"），地址栏输入测试不可靠。绕过：`xclip` 剪贴板 + `ctrl+v`，或直接改前端默认 URL
- **OCR 验证 GUI**：`import -window <wid>` 截图 + `tesseract` OCR 识别界面文字；裁剪区域放大 3-4 倍 + `ImageOps.autocontrast` 提高识别率（识别 "cn.bing.com" 可能被误读为 "2iIng.com"）

---

## 2026-09-11 21:30 — WebKitGTK 合成键盘事件与 GUI 测试方法（knowledge_gap）

**类别**: knowledge_gap
**场景**: ShiftBrowser Tauri 后退/前进按钮验证，需要 GUI 输入 URL 造历史记录
**要点**:
- **WebKitGTK 忽略合成键盘字符事件**：`xdotool type`（普通字符）无法输入到 webview 的 input 元素（输入框无反应），但 **ctrl 组合键有效**（`ctrl+a` 全选、`ctrl+v` 粘贴能触发）。高哥手动真实键盘输入正常 → 是 WebKitGTK 对合成键盘事件的兼容性限制，不是应用 bug
- **xdotool 坐标模式**：绝对坐标（`mousemove X Y`）点击有时无效（窗口位置漂移/XWayland 偏移）；**`--window <wid>` 相对坐标（`mousemove --window <wid> x y`）最可靠**，点击按钮/元素都用这个
- **造浏览历史测试后退/前进**：GUI 输入不可靠时，**临时改前端默认 URL**（如 `createTab(url = 'https://www.baidu.com')`）→ 重启 → 默认 baidu 标签 → 点主页按钮 navigateTab 到 bing → 历史 baidu→bing → 点后退应回 baidu，点前进应回 bing。测完改回默认 URL
- **tauri Webview 无 back/forward API**：只有 `navigate(url)` 和 `eval(js)`；后退/前进用 `eval("history.back()")`/`eval("history.forward()")`，页面加载完成会触发 `on_page_load` 回调（`page_load_finished` 日志可验证）
- **页面滚动正常但看似"不能滚"**：Bing 首页内容短（一屏装下）不需要滚动，百度首页内容多能滚动——是内容量差异，不是滚动机制问题

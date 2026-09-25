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

---

## 2026-09-11 22:30 — 窗口 resize 同步与弹窗遮挡（knowledge_gap）

**类别**: knowledge_gap
**场景**: ShiftBrowser Tauri，高哥最大化窗口后发现 UI 不跟随 + 密码管理弹窗被 child webview 遮挡
**要点**:
- **窗口 resize 后 webview 尺寸不同步**：主 webview 和 child webview 的 GTK 尺寸是启动时硬编码的（`set_size_request(1280, 800)`），窗口最大化到 1920x1011 后 Vue UI 仍只占 1280 宽（右侧露出 GTK 空白），child webview 也停在 1280x666。**解法**：`setup_gtk_layout` 里注册 `window.on_window_event(|e| if let WindowEvent::Resized(size) = e { ... })`，在回调里 `with_webview` 更新 Fixed 内所有 webview：主 webview `fixed.move_(&wv, 0, 0)` + `set_size_request(w, h)`；child webview `move_(&wv, 0, CONTENT_TOP_OFFSET)` + `set_size_request(w, h-134)`。验证：最大化后日志 `window resized: 1920x1011 content_h=877`，截图确认 UI 铺满
- **`on_window_event` 回调捕获 AppHandle**：回调是 `Fn(&WindowEvent) + Send + 'static`，不能直接捕获 gtk 对象；捕获 `app.handle().clone()`（AppHandle）即可在回调里 `get_webview` + `with_webview` 操作 GTK
- **`window.webviews()` 只返回 main webview**：child webview（tab-xxx）不在列表里！遍历必须从 `TabManager` State 拿标签 id，逐个 `window.get_webview(&format!("tab-{}", id))`
- **Vue 弹窗被 child webview 遮挡**：child webview 是独立 GTK 部件，GtkFixed 绝对定位叠在主 webview（Vue UI）之上。Vue 弹窗（fixed 定位）渲染在主 webview 里，被 child 盖住看不见。**解法**：弹窗打开时 `set_webview_visible(false)` 隐藏所有 child webview → 弹窗可见 → 关闭时 `set_webview_visible(true)` 恢复
- **窗口最小尺寸约束**：Tauri 窗口默认最小 1280x800，`xdotool windowsize` 无法缩小；最大化用 `wmctrl -i -r <WID> -b add,maximized_vert,maximized_horz`，还原 remove 可能不生效（窗口位置跑偏），直接杀进程重启恢复默认
- **xdotool 点击坐标基于窗口内容区**：窗口被 WM 移动/resize 后，绝对坐标失效；`mousemove --window <wid>` 相对坐标也要注意窗口实际尺寸（`getwindowgeometry` 为准），弹窗/表单元素位置会随窗口大小变化

---

## 2026-09-11 22:30 — 密码管理 UI 测试（insight）

**类别**: insight
**场景**: ShiftBrowser Tauri 密码管理弹窗 GUI 验证
**要点**:
- **弹窗验证用像素差异对比**：OCR 识别不了深色弹窗小字（遮罩叠加后对比度低），改用"点击前 vs 点击后"截图采样对比（`getpixel` 差异点数量 + 分布）判断表单是否展开——6057 个差异点、y=500-700 密集 = 输入框+按钮新增
- **表单输入无法自动化**：WebKitGTK 忽略合成键盘字符（xdotool type 无效），粘贴（ctrl+v）可填输入框但点击保存按钮位置难定位（表单展开后元素坐标变化）；密码保存的完整验证只能由高哥手动操作
- **`set_webview_visible` 命令**：`tabs.rs` 里遍历 TabManager 的标签 id 逐个 `get_webview` 隐藏/显示（不能用 `window.webviews()`，它不含 child）

---

## 2026-09-12 00:30 — GtkOverlay 上层 Fixed 拦截点击事件（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，GtkOverlay 双层布局后地址栏/弹窗按钮点击全部失效
**要点**:
- **根因**：GtkOverlay 的 `add_overlay` 子部件（GtkFixed）默认填满整个 overlay——即使 Fixed 里没有子部件覆盖地址栏区域，**Fixed 本身作为顶层容器拦截所有点击事件**（GTK 事件路由：上层子部件优先），Vue UI（主 webview 主层）收不到任何点击
- **修复 1（定位）**：Fixed 用 `set_valign(gtk::Align::Start)` + `set_margin_top(CONTENT_TOP_OFFSET=112)` 定位到内容区，不覆盖地址栏（y<112 的点击能到达 Vue UI）；resize 时 `fixed.set_size_request(w, content_h)` 跟随窗口
- **修复 2（弹窗交互）**：Fixed 仍覆盖内容区（y=112-778），弹窗面板在内容区里 → 点击弹窗按钮被 Fixed 拦截。`set_webview_visible(false)` 时**连 Fixed 一起 `hide()`**（弹窗可交互），`visible=true` 时 `show()` 恢复
- **child 坐标变化**：Fixed 定位到 (0,112) 后，child 在 Fixed 内位置改为 (0,0)（不再 (0,112)）；resize 监听里 `fixed.move_(&gtk_wv, 0, 0)` + `fixed.set_size_request(w, content_h)`
- **验证**：点后退按钮有 `go_back` 日志（点击到达 Vue UI）；弹窗遮罩 + 面板渲染；"+ 添加密码"点击展开表单
- **教训**：GtkOverlay 上层子部件不只是"叠层显示"，还会**独立拦截事件**——任何需要主层交互的弹窗/覆盖层都要考虑上层容器的遮挡

---

## 2026-09-12 00:30 — Vue 组件始终挂载时 onMounted 陷阱（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，PasswordsDialog 在 App.vue 里无 v-if 始终挂载，⋮ 点击后弹窗不显示
**要点**:
- **根因**：`<PasswordsDialog :show="showPasswords" />` 无 v-if → 组件在 App.vue 启动时挂载一次，`onMounted` 只在启动时执行（此时 show=false，弹窗不渲染）；⋮ 点击 → showPasswords=true → 组件 props 变化**不重新挂载** → onMounted 不执行 → child webviews 不隐藏 → 弹窗被盖住
- **解法**：`watch(() => props.show, async (v) => { ... })`——show 变 true 时隐藏 child + 加载列表，false 时恢复；**不要用 onMounted/onUnmounted 做显示副作用**（组件生命周期 ≠ 弹窗显示周期）
- **验证**：⋮ 点击 → `visible=false count=3` → 弹窗渲染 ✓
- **教训**：Vue 组件"始终挂载 + props 控制显示"时，副作用必须放 watch(props)，不能放生命周期钩子

---

## 2026-09-11 23:30 — GtkOverlay 布局与窗口 natural size（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，GtkFixed 方案下窗口最大化后无法还原/调整大小
**要点**:
- **根因**：GtkFixed 里主 webview 用 `set_size_request(窗口尺寸)` 跟随窗口 → 主 webview 的 size_request 撑大窗口 **natural size**（1920x1011）→ GTK 窗口无法缩小到小于 natural size → WM 还原失败（`wmctrl remove` 不生效、`wmctrl -e` 也拒绝）
- **GtkFixed 的特性**：子部件实际分配尺寸 = size request（fixed 的 allocate 按 size_request 分配），`size_allocate` 会被下一次布局循环覆盖；要改变子部件尺寸必须 `set_size_request`
- **解法：GtkOverlay 双层布局**：窗口 vbox → [GtkOverlay] → 主 webview（**主层**，GTK 自动填满 overlay，不需要手动 size_request）+ GtkFixed（**上层**，child webviews 绝对定位）。主 webview 和 Fixed 的 natural size 都固定 1280x800（初始 set_size_request），窗口可正常缩小；最大化时主 webview 自动铺满、child webviews 由 resize 监听更新 size_request
- **GtkOverlay API**：`overlay.add(&main)` 主层（自动填满）；`overlay.add_overlay(&fixed)` 上层（覆盖在主层之上）；`overlay.children()` 包含主层+上层所有子部件
- **resize 监听只更新 child webviews**：主 webview 在 overlay 主层自动跟随窗口（GTK 管理），不需要也不应该手动 set_size_request
- **验证**：最大化 1920x1011（地址栏铺满 1920 宽）→ 还原 1280x800 ✓ 循环多次正常

---

## 2026-09-11 23:30 — target=_blank 新窗口处理（knowledge_gap）

**类别**: knowledge_gap
**场景**: ShiftBrowser Tauri，点击 target=_blank 链接无反应
**要点**:
- **Tauri 2 WebviewBuilder::on_new_window**：回调 `Fn(Url, NewWindowFeatures) -> NewWindowResponse`，返回 `Deny` 拒绝新窗口 / `Allow` 默认创建 / `Create{window}` 指定窗口。MVP 方案：回调里 `wv.navigate(url)` 在**当前 webview** 打开链接 + 返回 `Deny`
- **关键前置**：webkit2gtk 的 `javascript_can_open_windows_automatically` **默认 false**——JS 的 window.open / target=_blank 不触发 create 信号，on_new_window 回调根本不会执行！必须开启：`webkit2gtk::Settings::builder().javascript_can_open_windows_automatically(true).build()` + `WebViewExt::set_settings(&settings)`
- **webkit2gtk Settings 无 setter**：`impl Settings` 只有 new/builder/font_size 方法，所有配置在 **SettingsBuilder** 上（`settings.set_xxx()` 不存在！），必须 builder 创建新 Settings 再 `set_settings` 替换
- **方法遮蔽**：`webkit2gtk::WebView` 继承 gtk::Widget，`webview.settings()` 解析到 `gtk::WidgetExt::settings()`（返回 gtk::Settings），不是 WebKitSettings！必须显式 `webkit2gtk::WebViewExt::settings(&webview)` 或 `webview.set_settings(&settings)`（WebViewExt 的 set_settings）
- **Cargo 依赖**：`webkit2gtk = { version = "=2.0", features = ["v2_40"] }`（与 tauri-runtime-wry 同版本）
- **验证**：window.open 触发 `on_new_window: url=https://www.qq.com/` → 当前 webview 导航成功 → `page_load_finished: title=www.qq.com`
- **调试技巧**：eval 注入 JS 构造 target=_blank 链接 + window.open 验证 create 信号；但 eval 的 `a.click()` 合成点击可能不算"用户手势"不触发 create，window.open 更可靠

---

## 2026-09-11 23:30 — 合成事件与滚动测试（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，xdotool 滚轮/PageDown 测试"滚动失效"
**要点**:
- **xdotool 合成滚轮/键盘事件不被 WebKitGTK 处理**（同合成键盘字符）：`xdotool click 5`（滚轮下）、`xdotool key Page_Down` 对 child webview 无效，页面零变化——**不是 bug，是自动化测试限制**，真实用户滚轮/键盘正常（高哥实测网易能滚动）
- **判断页面是否需要滚动**：用 eval 检查 `document.documentElement.scrollHeight vs window.innerHeight`——Bing 首页 `SH:666x666`（内容刚好一屏，无滚动条正常）；网易等长页面 scrollHeight 远超视口，有滚动条
- **验证事件是否到达 webview**：eval 注入 `document.addEventListener('click', e => document.title = 'CLICK:'+e.clientX+','+e.clientY)`，配合 `on_document_title_changed` 回调打印日志——点击 (640,400) → `CLICK:640,288`（y 差 112 = CONTENT_TOP_OFFSET，坐标转换正确，事件路由正常）

---

## 2026-09-12 12:00 — 前端加载失败 Connection refused（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，release 版启动后前端白屏，日志 "Connection refused"
**要点**:
- **根因 1：缺 custom-protocol feature**：Cargo.toml `tauri = { features = ["unstable"] }` 没有 `"custom-protocol"` → 应用进入 **dev 模式**（`tauri::is_dev()` 为 true）→ 主 webview 连 `http://localhost:5173`（无 vite dev server）→ 前端加载失败。**修复**：features 加 `"custom-protocol"`，release 版走 `tauri://localhost` 协议从 dist 加载
- **根因 2：strip=true 删符号**：`strip = true`（Cargo profile）会删除 cdylib（web extension）的符号表 → WebKit 找不到入口函数（`generate_web_extension`）→ `tauri://localhost` 协议失效。**修复**：改 `strip = "debuginfo"`（保留符号，体积略大）
- **辅助**：Linux 上手动设置 web extensions 目录：`WebContext::default()` + `set_web_extensions_directory(exe 所在目录)`（Tauri 2 不自动设置 extensions_path）
- **教训**：release 版前端白屏先查两件事——① tauri features 有没有 custom-protocol；② profile 有没有 strip=true。这两项都影响 web extension 加载

## 2026-09-12 12:30 — child webview reparent 后链接点击失效（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，child webview 移入 GtkFixed 后网页内点击链接不导航
**要点**:
- **根因**：child webview 被 reparent（vbox → GtkFixed）后，**WebKit 输入窗口不处理 DOM 点击**——点击事件到达 `WebKitWebView` 的 `button_press_event` 信号（GTK 层面），但 WebKit 内部不把它当作 DOM 点击（链接不导航、按钮不响应）
- **无效方案**：`grab_focus()`（只解决键盘输入焦点）、标准 `gtk_widget_reparent` FFI（布局正常但点击仍不处理）、unrealize+realize（页面黑屏）、不 reparent（窗口被撑到 2798px 高）
- **有效方案**：`connect_button_press_event` 里用 `run_javascript` 执行：`elementFromPoint(x,y)` 找点击位置的元素 → `closest('a[href]')` 找链接 → `window.open(a.href)` 打开（触发 on_new_window 创建新标签页）。**这是"手动补导航"，绕过 WebKit 输入窗口问题**
- **注意**：`window.open` 需要 `javascript_can_open_windows_automatically(true)`（在现有 Settings 上修改，不能新建替换——新建会把 Tauri 默认设置弄丢，链接点击直接失效）
- **窗口坐标 vs child 坐标**：GTK 事件坐标是 child webview 内的（y 从 0 起），页面 DOM 坐标一致，直接传给 elementFromPoint 即可；窗口坐标 = child 坐标 + CONTENT_TOP_OFFSET(112)

## 2026-09-12 13:00 — on_new_window 创建新标签页（insight）

**类别**: insight
**场景**: ShiftBrowser Tauri，点击链接应打开新标签页（统一行为）
**要点**:
- **on_new_window 回调里创建新标签**：`tauri::async_runtime::spawn` + `create_tab(app.clone(), app.state::<TabManager>(), url_str)` + 返回 `NewWindowResponse::Deny`（拒绝 WebKit 原生新窗口）——比"当前 webview 导航"更符合浏览器习惯
- **前端刷新靠轮询**：Tauri 2 无 event listen 插件（tauri-plugin-event 不在 crates.io），后端 emit 事件前端收不到——但前端 `startPolling()` 每 2s 拉 `get_tabs`，新标签自动出现（延迟 ≤2s，可接受）
- **JS 手动导航统一用 window.open**：`window.location.href` 是当前页导航（行为不一致：target=_blank 走 on_new_window、普通链接走当前页）；统一 `window.open(a.href)` 让所有链接都走 on_new_window → 新标签页，行为一致
- **验证链路**：button_press → `on_new_window: url=...` → `create_tab` → `create_webview` → 新标签创建成功 → `page_load_finished` → 前端轮询显示新标签 ✓

## 2026-09-12 13:30 — 前端改动必须重建 dist（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，改了 BookmarksBar.vue 但书签删除按钮不显示
**要点**:
- **生产模式从 dist 加载**：`tauri.conf.json` frontendDist 指向 `../dist`，release 版加载的是构建产物，**不是 src/**——改 .vue 后必须 `npm run build`（vue-tsc + vite build）重建 dist，否则改动不生效
- **验证**：hover 书签 → 出现 × 删除按钮（OCR：`www.qg.com * cn.bing.com`）✓
- **教训**：改前端代码后先问自己"这是 dev 还是 prod？prod 必须 npm run build"

---

## 2026-09-12 14:00 — 弹窗位置漂移与 xdotool 点击（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，分组管理弹窗改色/折叠/删除按钮点击
**要点**:
- **弹窗位置不稳定**：520px 宽居中弹窗打开后，位置可能漂移（截图分析 y=394-490，点击时可能已移动）——xdotool 点击前**必须重新截图确认**，否则点击穿透到错误元素（曾误改生活分组颜色）
- **改色验证成功**：点颜色点 → `update_tab_group` color 生效（#4a9eff → #e53935）
- **删除验证成功**：点删除按钮 → `delete_tab_group` 生效（分组删除）
- **折叠/重命名自动化受限**：WebKitGTK 合成点击位置漂移 + 合成键盘不可靠（重命名需输入文字）——这类操作留给高哥手动验证
- **验证技巧**：破坏性操作（删除）前先备份数据库（`cp shift-browser.db /tmp/xxx.bak`）；点击后用数据库查询确认命令是否生效
- **xdotool 点击验证链路**：截图 → python 分析按钮位置（#3d3d3d 按钮色 + 列表项 y 段）→ 立即点击 → 查数据库——截图和分析与点击间隔越短越可靠

## 2026-09-12 14:30 — 标签分组持久化与脏数据防护（insight）

**类别**: insight
**场景**: ShiftBrowser Tauri，标签分组重启后丢失 → sessions JSON 持久化
**要点**:
- **方案**：`SessionTab` 加 `groupId` 字段，`saveCurrentSession` 存 groupId，`restoreLastSession` 恢复时按 session 顺序 `assignTabToGroup`（分组 id 是数据库持久自增，重启后不变）
- **验证**：改 session JSON 给标签设 groupId → 重启 → 会话恢复 → TabBar 组头颜色条显示（border-left 3px 竖条，高 30px，区别于 favicon 的 10px 圆）
- **脏数据防护**：删除分组后 sessions 残留旧 groupId → 恢复时校验分组存在性（`useGroupsStore().groups.some(g => g.id === groupId)`），不存在则回未分组
- **TabBar 组头显示逻辑**：`filter((g) => g.tabs.length > 0 || true)` —— `|| true` 导致**所有分组都显示组头**（即使无标签）——验证时不能靠"组头数量"判断分组是否有标签

## 2026-09-12 15:30 — 设置页面：⚙ 按钮位置与 flex 布局（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，设置页面 ⚙ 按钮放地址栏左侧导航区点击无效
**要点**:
- **⚙ 按钮放地址栏左侧（第 5 个 nav-btn 位置）点击无效**：xdotool 点按钮中心无反应，弹窗不打开；但同一区域的刷新按钮正常、右侧按钮（密码/分组/下载）正常
- **排查过程**：像素分析找"齿轮图标"→ 实际是地址栏 URL 文字（x=162-187 是 https:// 的灰色像素）；flex 布局中 ⚙ 按钮被地址栏输入框挤压（地址栏输入框从 x=144 开始，⚙ 按钮位置被占）
- **解决**：把 ⚙ 移到右侧（替换无功能的 ☆ 收藏按钮）→ 点击正常
- **教训**：地址栏按钮太多时优先放右侧（右侧 4 个 action-btn 点击验证正常）；左侧 nav-btn 区域 flex 布局挤压问题难排查，不如直接换位置
- **验证技巧**：emoji 按钮（⚙🔑🗂⬇）OCR 识别不了，用像素分析（灰色/彩色图标段）定位；"齿轮图标"和"URL 文字"都是灰色像素，容易混淆——用上下文（前后段间隔）区分

## 2026-09-12 15:40 — 多标签并发加载 WebKit 崩溃（insight）

**类别**: insight
**场景**: ShiftBrowser Tauri，恢复会话（5 标签并发加载）时崩溃
**要点**:
- **症状**：启动后 WebKit 大量 `WebLoaderStrategy::internallyFailedLoadTimerFired` 错误 → 进程崩溃
- **排查**：网络正常（ping 通）→ 怀疑历史记录逻辑（新增 page_load_finished → record_history）→ 清空 sessions（只加载 1 个标签）→ 正常不崩 → **结论：多标签并发加载触发 WebKit 网络层 bug，与历史记录无关**
- **应对**：清空 sessions 后应用正常（1 标签）；当前 4 标签 session 也正常——崩溃是偶发（WebKit bug），非必然
- **教训**：排查崩溃先做对照实验（清空数据 vs 保留数据）；WebKit 网络层错误（WebLoaderStrategy）是已知 bug 类，别急着怀疑自己的代码

## 2026-09-12 16:20 — WebKitGTK 浮层菜单点击 bug（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，工作区下拉菜单（position absolute/fixed）渲染可见但点击无效
**要点**:
- **症状**：下拉菜单（position: absolute/fixed 的浮层）渲染正常（截图可见文字/背景），但点击菜单项完全无效（xdotool 点击无效、hover 无效）；菜单外点击关闭逻辑也不触发
- **排查**：坐标转换没问题（mousemove --window 正确）；TabBar 其他元素点击正常（标签 switch_tab、书签 create_tab）；AddressBar 无 stacking context；z-index 2000 正常
- **解决**：**改用弹窗模式**（dialog-overlay fixed 全屏 + dialog 居中，与设置弹窗/分组弹窗一致）——点击立即可靠
- **教训**：WebKitGTK 对 position absolute/fixed 的浮层菜单点击事件处理有 bug（渲染可见但事件不响应）——**以后所有下拉菜单/浮层交互一律用弹窗模式**（overlay + dialog），不要用 position absolute 的菜单
- **补充**：弹窗打开时必须 `set_webview_visible(false)` 隐藏 GTK 层 webview（HTML overlay 盖不住 GTK webview）；关闭按钮字符用 ✕（U+2715）不用 ×（U+00D7）

## 2026-09-12 16:30 — release 编译必须 cargo build --release（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，代码改动后运行不生效
**要点**:
- **症状**：改了 Rust + 前端代码，cargo build（dev profile）通过，但运行 target/release/shift-browser 时 UI 没有新功能
- **原因**：**一直用 cargo build（dev），但运行的是 release 二进制**——dev 编译的 target/debug 不是运行目标；release 二进制嵌入的是编译时的 dist（Tauri 编译期嵌入前端资源）
- **解决**：Rust 改动必须 `cargo build --release`（6 分钟左右）；前端改动 npm run build 后也必须重新 cargo build --release（dist 是编译期嵌入）
- **教训**：**Tauri 项目改代码后必须 build --release**；排查"改动不生效"先看运行的是哪个二进制（ps aux 确认 target/release）

## 2026-09-12 18:20 — Vue 事件名连续大写陷阱（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，AI 按钮点击无响应（hover 正常、其他按钮正常）
**要点**:
- **症状**：AddressBar 的 AI 按钮 hover 变色正常，但点击完全无反应；其他按钮（openPasswords/openGroups 等）正常
- **根因**：`emit('openAI')` 的事件名含连续大写 "AI"——Vue 3 的 hyphenate 转换（`/\B([A-Z])/g`）把 `openAI` 转成 `open-a-i`，而模板写的是 `@open-ai`——**不匹配**——emit 无效
- **解决**：事件名改用 `openAi`（避免连续大写）——`openAi` → `open-ai` 匹配
- **教训**：**Vue 事件名避免连续大写**（openAI/openURL 等都不行），用 openAi/openUrl；排查"按钮点击无反应"先检查 emit 事件名和模板 @事件名的匹配

## 2026-09-12 18:30 — WebKitGTK 面板高度/emoji 按钮/overlay 关闭（correction）

**类别**: correction
**场景**: ShiftBrowser Tauri，AI 侧边栏面板开发
**要点**:
- **面板高度**：flex 子元素 `height: 100%` 在 WebKitGTK 不生效（面板只有内容高度）→ 用 `position: fixed; right: 0; top: 0; height: 100vh` 解决
- **emoji 按钮**：🤖（U+1F916）在 WebKitGTK 渲染异常（hover 正常但 click 不触发）→ 按钮内容改用文字
- **overlay 关闭**：`@click.self` 会因 WebKitGTK 点击判定（release 元素触发 click？）导致弹窗开了又关 → 去掉 overlay 关闭，只保留 ✕ 按钮
- **xdotool 自动化**：`xdotool click`（屏幕坐标）在窗口失焦时无效 → 用 `xdotool click --window $WID`（窗口相对坐标）可靠；窗口激活用 `windowactivate --sync`
- **教训**：WebKitGTK 的 CSS/事件行为与标准浏览器有差异，遇到"渲染正常但交互异常"优先怀疑 WebKitGTK 特性（浮层菜单点击、emoji 渲染、flex 高度、click 判定）

## 2026-09-12 20:15 — AGNES 模型实测（correction）

**类别**: correction
**场景**: ShiftBrowser AI 侧边栏，聊天功能"没实现"
**要点**:
- **症状**：AI 聊天点发送后无回复（前端一直转圈）
- **根因**：默认模型 agnes-2.5-flash 在 AGNES API 上 POST /chat/completions 超时（60s+ 无响应，HTTP 000）；agnes-2.0-flash 正常（~5s 返回）
- **排查**：curl 直测 API——GET /v1/models 正常（模型列表含 agnes-2.5-flash），但 POST 2.5-flash 超时；注意 curl 单引号嵌套会破坏 JSON（返回 400 或挂起）
- **解决**：默认模型改 agnes-2.0-flash；Rust reqwest 加 90s 超时；设置对话框加模型选择让用户可切换
- **教训**：AI 功能"没反应"先直测 API（curl），判断是 API 问题还是前端问题；模型可用性要实测（列表里有 ≠ 能用）

## 2026-09-12 21:30 — WebKitGTK 下载信号接入（correction）

**类别**: correction
**场景**: ShiftBrowser 下载管理真实下载流程
**要点**:
- **WebView 无 download 信号**（webkit2gtk-rs 2.0 编译报 E0599）→ 用 `WebContext::connect_download_started`（需 `use WebKitContextExt`）
- **Download 对象 API**：无 get_suggested_filename / get_uri / get_total_content_length → 用 `request().uri()`（需 URIRequestExt）推断文件名、`response().content_length()`（需 URIResponseExt）拿总字节
- **重复注册**：每个 webview 创建时都会拿 context → 用 `static AtomicBool` 全局标志防重复注册（同一 context 重复注册会 N 次触发）
- **id 竞态**：下载开始记录用 `tauri::async_runtime::block_on` 同步插入拿 id，避免进度回调拿不到 id（下载很快时 received_data 可能在异步插入完成前触发）
- **文件名推断**：URL 最后一段去 query（`u.split('?').next()` + `rsplit('/').next()`）
- **自动化测试受阻**：WebKitGTK 合成键盘输入（xdotool type/key）在地址栏不可靠（导航不触发）——真人操作正常；下载功能需真人验证
- **教训**：WebKitGTK 的 API 与文档有差异（信号位置、方法名），编译报错先 grep ~/.cargo/registry 里的 webkit2gtk 源码确认 API

## 2026-09-12 22:10 — GTK 主线程 block_on 死锁（correction，重要）

**类别**: correction
**场景**: ShiftBrowser 下载管理——点击下载链接后浏览器整个卡死（高哥实测反馈）
**要点**:
- **根因**：`connect_download_started` 回调跑在 **GTK 主线程**，回调里 `tauri::async_runtime::block_on` 等 sqlx 插入 → 主线程被阻塞，WebKit 下载引擎/界面事件全卡 → **UI 死机**
- **铁律**：**GTK/WebKit 回调里绝对不要 block_on**（无论等待多短）——GTK 主循环需要持续处理事件，任何阻塞都是死锁
- **正确方案**：回调里 `tauri::async_runtime::spawn` 异步插入；记录 id 用 `Arc<Mutex<Option<i64>>>` 共享；进度/完成/失败回调里 spawn 任务，先读共享 id，没就绪就 `tokio::time::sleep` 轮询等（最多 5 秒）——全程不碰主线程
- **MutexGuard 跨 await**：`match *lock.lock().unwrap() { ... None => await ... }` 编译报 "future cannot be sent between threads safely"——先 `let id_now = { *lock.lock().unwrap() };` 让 guard 在块尾 drop，再 await
- **SQLite 写锁竞争**：下载快时 received_data 每秒触发多次，每次都 UPDATE → 写锁排队（UPDATE 卡 1-5 秒，INSERT 直接 database is locked 失败）→ 两个修复：① `SqliteConnectOptions::busy_timeout(10s)` ② 进度节流（`Arc<Mutex<Instant>>` 500ms 内最多写一次）
- **total 未知**：下载开始时 `response().content_length()` 可能为 0（如百度网盘）→ 进度回调时再取一次，`UPDATE ... total_bytes = CASE WHEN total_bytes = 0 THEN ? ELSE total_bytes END`
- **教训**：异步回调链里"读共享状态再 await"的写法，先提取值再 await，guard 别跨 await 边界

---

## 2026-09-12 14:40 — 杀进程绝不用 pkill -f 匹配项目名（correction，高哥明确批评）

**类别**: correction
**场景**: 清缓存重启浏览器时 `pkill -9 -f "shift-browser"` 把自己（执行命令的 shell）也杀了，命令中断，重复犯过至少两次
**要点**:
- `pkill -f "xxx"` / `ps aux | grep "xxx" | grep -v grep | awk '{print $2}' | xargs kill` 会匹配**执行命令的 bash -c 自身命令行**（命令行里含该字符串）→ 把自己杀死、命令中断
- **正确做法（记死）**：
  - `ps -C shift-browser -o pid= | xargs -r kill -9`（按进程名 comm 精确匹配，bash 不匹配）
  - 或 `pgrep -x shift-browser | xargs -r kill -9`
- 杀 WebKit 子进程同理：`ps -C WebKitWebProcess -o pid=` 等，绝不 `pkill -f WebKit`
- 清缓存目录用 rm -rf 时也要先确认路径，别把 ~ 打错

---

## 2026-09-12 15:30 — 扩展注入四大坑（insight，高哥实测通过）

**类别**: insight
**场景**: ShiftBrowser v5.4.0 扩展管理（WebKitGTK UserContentManager 用户脚本/样式注入）
**要点**:
- **前端改动必须重新 cargo build**：Tauri 2 的 `generate_context!` 编译时把 dist 嵌入二进制，光 `npm run build` 不重新 `cargo build`，二进制里还是旧前端（高哥看不到扩展节、输入框白底的根因）
- **取窗口用 `app.get_window("main")` 不是 `get_webview_window`**：本项目主窗口是 Window 类型，`get_webview_window("main")` 返回 None 导致注入被静默跳过（日志"主窗口不可用"）
- **`Window::webviews()` 返回 `Vec<Webview>`**（无 label 元组），`WebviewWindow::webviews()` 才返回 `Vec<(&String, Webview)>`，遍历写法不同
- **注入必须跳过主 webview**（label=="main"，Vue UI）：`remove_all_scripts()` 会误删 Tauri 主 webview 的 UserScript（Vue UI 桥接），导致界面白屏崩溃
- **allow_list 留空**：WebKitGTK 的 UserScript/UserStyleSheet allow_list 不识别 `"http://*"` 通配符格式，脚本不注入；跳过主 webview 后 child 都是外部网页，allow_list 留空安全
- **注入只对刷新后的页面生效**：UserScript 在页面加载时执行，已打开的页面需刷新

## 2026-09-12 广告拦截 CSS 规则块尾逗号坑
- **错误**: 扩充 ADBLOCK_CSS 时，在中间选择器组后面用了 `{ display: none !important; },`（逗号结尾再接新组），CSS 语法不允许这种写法，导致后面整段规则失效。
- **纠正**: 所有选择器合并到一个 `{ display: none !important; }` 块里，块与块之间不需要分隔符。
- **教训**: 往 CSS 常量里加规则，先想清楚是"追加选择器到已有块"还是"新开一个块"，别混。

## 2026-09-12 23:00 — 广告拦截误杀选择器（correction）

**类别**: correction
**场景**: ShiftBrowser 广告拦截扩充 CSS/关键词
**要点**:
- `a[href*="tmall.com"]` 会把天猫购物页所有链接藏掉，联盟追踪才该拦（`uland.taobao` / `s.click.taobao` / `click.tb`）
- `div[style*="position:fixed"][style*="z-index"]` 会误杀登录框、cookie 条、播放器，只拦 `z-index:2147483647` 这种广告弹层常用值
- 文本关键词匹配必须限制容器大小（高度 ≤520、字数 4-400），否则「推广」「赞助」会把整页正文藏掉
- 自定义关键词走 settings.`adblock_keywords` JSON，注入时拼进 JS 数组；UserScript 只在页面加载时执行，改词后要刷新页面

## 2026-09-12 Tauri show/hide 不能包在 run_on_main_thread 里（重入失效）
- **错误**: 为"减少往返"把 `wv.show()/wv.hide()` 包进 `app.run_on_main_thread(closure)`，结果切换标签页内容不切换，多点几次才偶尔生效。
- **根因**: tauri-runtime-wry 的 `send_user_message` 在**主线程时直接重入 `handle_user_message`**（非主线程才 proxy.send_event 排队）。在事件循环处理 Task 闭包时重入，可见性变更静默失效。
- **纠正**: webview 的 show/hide 直接在 async 命令线程调用——它们内部走 proxy 事件队列，入队顺序=执行顺序，天然保证"先 show 新再 hide 旧"。
- **通用教训**: Tauri 的窗口/webview 操作 API 本身就是线程安全的（内部投递到主线程），**绝不要**再手动包 run_on_main_thread，那是套娃。

## 2026-09-12 flexbox min-width:auto 挤出兄弟元素坑
- **症状**: 状态栏"只剩 URL"，右侧"X 标签 · v5.0.0"看不到。
- **根因**: flex 子项默认 `min-width:auto`——长 URL 的 .status-left 即使设了 overflow:hidden+ellipsis 也撑到内容全宽，把 .status-right 挤出容器。
- **纠正**: 给可收缩的 flex 子项加 `min-width: 0`，ellipsis 才真正生效。
- **教训**: flex 布局里"文本截断不生效/兄弟元素消失"，第一个查 min-width:0。

## 2026-09-13 GNOME/Xwayland 桥接挂死：窗口永远不上屏
- **症状**: 浏览器进程活着、日志正常、X11 窗口存在但 Map State=IsUnMapped/WM_STATE=Iconic；
  Activities 概览、Alt+Tab、Dock 里都找不到窗口；unminimize/windowmap/wmctrl 全部无效。
- **误判**: 先怀疑代码（newtab block_on）、库升级（apt webkit）、数据（homepage），对照实验全排除。
- **根因**: Xwayland 与 mutter 的窗口桥接挂了——Xwayland 不转建新 X11 窗口的 surface。
  旁证：同期后台启动的 zenity 也起不来，但前台跑能显示（时间点错开的假象）。
- **纠正**: `kill -9 $(pgrep Xwayland)`，gnome-shell 会在下个 X11 客户端出现时自动重建。
  重启前用 `xlsclients` 确认没有别的要紧 X11 GUI 应用在跑。
- **教训**: GUI 应用"进程活着窗口不出现"，先测**任意**新 X11 窗口能否显示（对照），
  是环境病就别在代码里找原因。

---

## 2026-09-17 18:20 — opencode 全面自检：钉钉通道断裂根源

**类别**: insight
**场景**: opencode 系统全面自检
**发现**:
1. **opencode-serve.service 已死**（9月15日 20:34:59 被停止后未重启）→ 4096 端口无服务 → dingtalk-bridge 每2秒 "fetch failed" 疯狂重连 → 钉钉消息处理全部失败（已断 2 天）
2. **opencode.db 15.9GB 严重膨胀**（仅 28 会话/13739 消息/107142 parts）→ compacted 版本仅 631MB（可缩 96%）→ 数据库过大是 9月15日 OOM kill（systemd-oomd 杀 11 进程）的根源之一
3. **备份文件占 40GB**：opencode.db.bak_20260914（15G）+ before_vacuum.bak（8.7G）+ compacted（631M）
4. **atomcode datalog 16GB**：Toonflow-app 11G + clipyai 2.6G + ShiftBrowser 2.2G
5. **opencode.log 85MB** 无轮转；bridge.log 15MB 每2秒刷一条
6. **atomcode 市场更新失败**：atomgit.com DNS 解析失败 + 418
7. **架构**：opencode-serve(4096) 是钉钉桥接后端；opencode-fix.py(14096) 是绕过 opencode API 直接读库的临时补丁，仅提供 /session 查询
**预防**: 自检时先看 systemd 服务状态和端口监听，再查日志；数据库膨胀要定期 vacuum，备份别堆在数据目录

---

## 2026-09-17 19:58 — opencode 自检修复实战经验

**类别**: best_practice
**场景**: 修复 opencode 系统问题
**关键经验**:
1. **数据库膨胀根源是 event 表孤儿数据**：event 表 37 万行 14.1GB，其中 16.5 万行 12.6GB 属于已删除会话（session 表无记录）。DELETE 大表数据写 WAL 极慢（12.6GB 写入超时回滚 2 次）
2. **正确姿势**：CREATE TABLE event_new AS SELECT ... JOIN session（只复制有效数据 1.7GB）→ 但 DROP TABLE 需要独占锁，opencode 运行中拿不到
3. **终极方案**：VACUUM INTO 生成新库副本（无需独占锁）→ 在新库上 DROP/RENAME/重建索引 → 替换主库。VACUUM INTO 目标必须放 /home（/tmp 只有 12G，库 16G 放不下会报 disk full）
4. **WAL 陷阱**：checkpoint 返回 (0,0,0) 不一定代表 WAL 已清空；DELETE 事务未提交时 WAL 会涨到 13GB，checkpoint 会丢弃未提交帧（WAL 缩到 32 字节）
5. **替换后验证**：检查所有进程 fd 确认都用新库（避免 serve/CLI 还握着旧 inode 导致数据分叉）
6. **sudo + heredoc 冲突**：`echo '密码' | sudo -S tee ... << EOF` 中 heredoc 会覆盖管道 stdin，密码传不进去 → 先写临时文件再 sudo cp
7. **logrotate 配置**：copytruncate 模式适合被进程持续打开的日志；用户文件用 `su gao gao`
**结果**: 数据库 15.9G→1.8G、磁盘释放 53G、日志轮转生效、钉钉通道恢复

---

## 2026-09-17 20:30 — pkill -f 误杀自己 + 误杀 Electron 原版

**类别**: error
**场景**: 启动 Tauri 版验证前想清理进程，用了 `pkill -f "shift-browser"`
**发现**:
1. **`pkill -f` 匹配完整命令行**：我自己的 bash 命令命令行里含 "shift-browser" 字符串（要启动的二进制路径），pkill 把执行命令的 shell 也杀了 → 命令中断无输出
2. **连带误杀 Electron 原版**：/opt/Shift Browser/shift-browser 进程名和命令行都含 shift-browser，被一起杀掉 → 高哥正在用的原版浏览器被误杀，需要重新拉起
3. HANDOFF 7.5 早记过此坑（"命令行里含该关键字的进程会误杀自己"，当时是 grep 误杀），这次 pkill 又踩了一遍
**预防**:
1. 杀进程用精确方式：`ps -eo pid,args | grep 精确路径 | awk '{print $1}'` 拿 PID 再 `kill <PID>`；或 `pgrep -x`（精确进程名）；**不要用 `pkill -f 宽泛关键字`**
2. 启动新实例前先确认要杀的目标到底是谁（Electron 原版 vs Tauri 版进程名相同，用命令行路径区分）
3. 涉及"清理进程再启动"的操作，先 `ps -eo pid,args` 看清再动手
**结果**: Electron 原版已重新拉起（/usr/share/applications/shift-browser.desktop 的 Exec）；Tauri 版用精确 PID 安全关闭

---

## 2026-09-18 07:20 — 单测里写了真实密码，提交前扫描拦住（error）

**类别**: error
**场景**: 第二十一轮 P3-1 给 PBKDF2 写单测，顺手用了高哥的真实锁屏账号密码当测试数据
**发现**:
- 测试代码（`insert_user(&db, "gaoge", "206694", ...)`）会随 git 提交永久留在仓库历史里，等于把密码写进版本库
- 提交前做 diff 敏感信息扫描时才发现，已全部替换为假数据（`testuser` / `test-pass-42`）
**预防**:
1. **测试数据一律用假值**：写单测时不要图省事复制真实账号/密码/token，一律用明显假的占位值
2. **提交前必扫**：`git diff | grep -inE "(password|secret|token|api[_-]?key)"`，再单独 grep 已知的真实口令（如锁屏密码、sudo 密码）
3. 真实凭据只允许出现在运行时文件（600 权限的配置/密钥文件），绝不能进源码和测试

---

## 2026-09-18 07:20 — PBKDF2 迭代次数要与交互延迟一起权衡（insight）

**类别**: insight
**场景**: P3-1 把密码哈希迭代次数从 10 万提到 OWASP 建议值
**要点**:
- 一开始按"越高越安全"选了 60 万，实测 debug 下单测跑 55 秒、推算 release（本项目 `opt-level="s"`，体积优化比速度优化更慢）登录接近 1 秒——本地锁屏解锁会明显卡顿
- 最终取 **21 万**（OWASP 2023 对 PBKDF2-SHA512 的建议下限）：约 0.2-0.4 秒，安全性与体验平衡
- 关键设计：把参数写进 `hash_algo` 字段（如 `pbkdf2-sha512-210000`），验证成功后自动用新参数重算并落库 → **日后想调次数不用锁死老用户**
- 判断依据要落到实测/推算，别只凭"更安全"就往上加
**结果**: 34 个测试全过，生产库 gaoge 的 hash_algo 已在锁屏解锁时自动升级为 `pbkdf2-sha512-210000`

---

## 2026-09-18 — P0-P2 三批安全修复心得（best_practice）

**类别**: best_practice
**场景**: ShiftBrowser 安全加固（P0 XSS/竞态 → P1 假成功链/SSRF → P2 核心模块/CI）
**要点**:
1. **reqwest 流式读取要开 `stream` feature**：`resp.bytes_stream()` 在 `features = ["json", "rustls-tls"]` 下不存在（E0599），必须加 `"stream"`；配合 `futures-util` 的 `StreamExt` 才能边读边计数防内存炸弹
2. **url crate `host_str()` 对 IPv6 返回带方括号**：`parse::<IpAddr>("[::1]")` 会失败，必须先 `trim_start_matches('[')`/`trim_end_matches(']')`——SSRF 校验 IPv6 内网段必踩的坑
3. **可测性模式**：Tauri 命令带 `State<'_, Db>` 不便单测，把核心逻辑抽成 `xxx_inner(pool, ...)` pool 级函数、命令薄包装（downloads.rs 先例），测试直接调 inner
4. **测试模块 import 二选一**：要么 `use super::*` 后裸调函数名，要么全限定 `super::xxx`——混用会触发 `unused_imports`（clippy 警告）
5. **CI 接线 fmt 的坑**：项目从没跑过 rustfmt，`cargo fmt --check` 一接就爆几百处 diff；正确顺序是先本地 `cargo fmt` 全量格式化（rustfmt 不改语义），再 CI 加 `--check`
6. **rust-toolchain.toml 固定版本**：放仓库根，rustup 自动读取，本地/CI 完全一致（dtolnay/rust-toolchain action 会跟随）
7. **特权页面（带 IPC 桥/invoke_key）的 URL scheme 白名单**：newtab 是特权页，`javascript:` 渲染成 `<a href>` 点击即任意命令执行——保存（save_quick_sites）和渲染（generate）两头都拦，防历史遗留恶意数据
8. **翻译端点 callback 白名单**：callback 拼进 JS 变量名 eval 到 webview，必须严格 `[A-Za-z0-9_]{1,64}` 白名单（比转义更干净），入口+写入双保险
9. **shift:// 自定义协议 handler 是远程页面可控入口**：on_new_window 必须拒绝非 http/https（防 window.open('file:///etc/passwd')），create_tab/navigate_tab 走白名单保留内部页面能力

---

## 2026-09-18 10:25 — 三个踩坑：Vue 编辑重复钩子、GTK3+Wayland 窗口圆角、stdout 缓冲吞日志（correction + insight）

**类别**: correction / insight
**场景**: ShiftBrowser v5.5.1（加载失败提示 + 解锁自动开启动页 + 窗体圆角）
**要点**:
1. **correction：edit 大段替换 Vue script 时别漏删旧块**。给 StatusBar.vue 加 `loadFailed` 监听时，newString 里带了 `onMounted`/`onUnmounted`，而原文件里也有一份 —— 结果出现两个 `onMounted`、`appVersion` 在定义前被引用（const 不 hoist 会 ReferenceError）。`grep -n "onMounted\|onUnmounted"` 自检能立刻发现。改完 Vue 必须数一遍生命周期钩子数量。
2. **insight：GTK3 + Wayland 下真正的整窗边框圆角做不了**。Wayland 没有 XShape 扩展；GTK3 的 CSS 不支持 `window` 选择器圆角；X11 XShape（`gtk_window_set_shape_from_region`）**只接受矩形区域、不含弧线**，也做不出圆角。唯一可行是**内容圆角**：给各页面 body 加 `border-radius:12px; overflow-x:hidden`（启动页/世界导航页在 newtab.rs 的 format! CSS 里、主界面在根 index.html 模板里）。且**改 dist/ 无效**——`npm run build` 会覆盖，必须改源码模板。
3. **insight：Rust log 走 stdout 被全缓冲，日志"卡住"不是没输出**。GUI 进程 stdout 重定向到文件时是块缓冲（4-8KB），不 flush 就看不到。启动命令加 `stdbuf -oL`（行缓冲）后日志实时可见——排查"启动页加载失败"这类问题时，这一步比读代码更关键。用 `setsid nohup ... < /dev/null &` 还能让进程完全脱离、启动命令不再挂住 shell 工具。
4. **insight：定位"加载失败"根因的正确姿势**：Tauri `PageLoadEvent` 只有 Started/Finished（没有 Failed），必须下沉到 GTK 层 `connect_load_failed`（webkit2gtk-2.0 签名是 `Fn(&WebView, LoadEvent, &str, &glib::Error) -> bool`，**返回 bool** 而不是 `glib::Propagation`，返回 true 才阻止 WebKit 默认错误页）。CLI 输出会误导：本地 systemd/沙箱里"创建页面失败"是 WebKit 的 `WEBKIT_ERROR_CANNOT_CREATE_PAGE`（错误码 104），系统 `WebKitGTK-4.1.mo` 里搜不到这个中文串，别死磕翻译文件。
**结果**: v5.5.1 提交 eb51f88；59 测试全过、clippy 零警告；日志行缓冲后能实时观察解锁→建标签→page_load_finished 全链路

---

## 2026-09-18 10:36 — 前端初始化撞上 Rust setup 阻塞：要用"可查询状态"而非"猜失败"（best_practice）

**类别**: best_practice
**场景**: ShiftBrowser 启动锁屏期间前端反复弹"创建/加载标签页失败"toast
**要点**:
1. **症状与根因**：Tauri 的 `setup()` 里同步弹 GTK modal 密码对话框（`dialog.run()` 阻塞事件循环），此时 `app.manage(Db/TabManager)` 都还没执行；而主窗口 webview 早已加载，Vue `onMounted` 照常跑 `initBrowser()` 并对失败做"每秒重试 10 次"兜底 → 每次命令失败都 `useToastStore().error(...)`，用户看到的是**锁屏界面上不停弹同一个失败提示**。
2. **错误做法**：只在前端"捕获异常后静默"或"减少重试次数"——治标；前端无法区分"锁屏未就绪"和"真故障"，静默会吞掉真实错误。
3. **正确做法**：**Rust 暴露一个不依赖 Db/State 的状态查询命令**。锁屏标志用 `static LOCKED: AtomicBool`（不能放进 `State<Db>`，锁屏时 Db 还没 manage，命令参数取 State 会 panic/失败），锁屏前置 true、验证通过置 false；`#[tauri::command] fn is_locked() -> bool` 只读原子变量，**在 Db 就绪前也能成功返回**。
4. **前端配合**：`onMounted` 第一件事查 `is_locked`；true 则只 `listen('app-unlocked')` 静默等待，不初始化/不轮询；解锁后再跑初始化 + 建启动页。另在 store 里缓存该状态（`setLockedState`/`isLockedNow`）作为双保险，命令失败时若锁屏中则静默——既防时序错开，也保留非锁屏时的真实报错。
5. **验证手法**：对比"日志安静度"最直观——修复前锁屏 13 秒内 `get_all_settings 被调用` 每秒刷屏且伴随 create_tab 失败；修复后同样 13 秒**日志无任何业务调用**，解锁瞬间才出现 `app-unlocked` → `create_tab` → `page_load_finished`。
6. **顺带**：`initBrowser()` 加 `skipStartupTab` 参数，把"建标签"职责交给调用方，避免"恢复上次会话"和"解锁后开启动页"两个分支各建一个标签。
**结果**: 提交 3a13d3b；59 测试全过、clippy 零警告、前端 build+lint 通过；锁屏期间日志完全安静

---

## 2026-09-18 11:15 — Tauri 前端事件监听必须在 IPC 就绪后再注册 + 兜底轮询（correction / best_practice）

**类别**: correction / best_practice
**场景**: ShiftBrowser 解锁后不自动打开启动页（既不建标签、也不报错，纯静默死等）
**踩的坑（真实事故）**:
- 为实现"锁屏期间静默等待解锁"，把 `listen('app-unlocked', ...)` 挪到 `onMounted` 最前面（在 `await invoke('is_locked')` 之前），想着"先注册防漏事件"。
- 结果：注册请求发出时主窗口 webview 的 IPC 桥还没就绪，`listen` **静默失败**，而调用处是 `listen(...).catch(() => {})` —— 错误被完全吞掉，前端表面无异常。
- 后果：解锁事件永远收不到，前端卡在"等待解锁"，用户看到**没有标签、页面空白**；手动点 + 号不经过该分支所以正常 —— 症状极具误导性。
**正确做法（三重保障，缺一不可）**:
1. **先用 `await invoke(...)` 探一次 IPC**（哪怕只是查状态），确认桥就绪后再 `await listen(...)`；`listen` 要 `await`，不要 fire-and-forget。
2. **关键状态加兜底轮询**：锁屏期间每秒 `invoke('is_locked')`，一旦变 false 立即初始化。**本次就是兜底先发现解锁的**，事件压根没到。
3. **初始化函数防重入**（`if (ready) return`），让"事件 / 兜底轮询 / 未锁屏直达"三条路径只跑一次。
**配套排查利器**:
- 加一个 `front_log(msg)` Rust 命令把前端关键节点写进服务端日志（`log::info!("[front] {}", msg)`）——**webview 的 console 在服务端完全看不见**，没有这个只能瞎猜。本次正是靠 `[front] onMounted 开始 / is_locked=true / 已注册监听` 才定位到"注册成功但事件没到"。
- 高频同步命令（get_tabs）加 `info` 级条数日志，能一眼看出前端列表是否为空、同步是否在跑。
**教训**: 在 Tauri/Electron 这类"前端 + 原生"架构里，**前端首个 IPC 调用之前的状态都是不可信的**；`catch(() => {})` 吞掉注册类错误极其危险，至少要 `console.error` + front_log。
**结果**: 提交 e2b6d5e；解锁后 1 秒内自动建启动页；59 测试全过、clippy 零警告

---

## 2026-09-18 12:20 — 浏览器反指纹在 WebKitGTK 上的工程边界与落地要点（best_practice）

**类别**: best_practice
**场景**: ShiftBrowser 隐私防护三连（P0 基础防护 → P1/P2 指纹面收敛 → 三档分级）
**核心结论**:
1. **反指纹只有两条路，必须先选路线**：
   - **随机化**（Brave/Safari）：同站点内稳定、跨站点/跨会话变化 → 不破坏功能，适合日常浏览器；
   - **统一化**（Tor/Mullvad/Firefox RFP）：所有用户长得一样（含 letterboxing）→ 最强但毁功能。
   ShiftBrowser 选**随机化为主的混合**：基础特征归一化（UA/硬件/GPU 报通用值）+ 高熵特征加噪。
2. **"每源都变"是新手最易犯的错**：UCSD IMC25 论文指出，商业指纹方正是靠"同一设备每次渲染都不同"来判定"这人在加噪"。正确做法是 **per-origin 确定性伪随机**：FNV-1a 哈希 `location.origin` 得种子 → xorshift32 生成噪声 → 同源稳定、跨源变化。
3. **WebKitGTK 的 API 边界（实测）**：
   - ✅ 有 `SettingsExt::set_user_agent` / `set_enable_webrtc` / `set_javascript_can_open_windows_automatically`
   - ✅ 有 `CookieManagerExt::set_accept_policy(CookieAcceptPolicy::NoThirdParty)`（第三方 Cookie 阻断）
   - ❌ **没有 `WebView::connect_send_request`** → 无法改请求头，所以 **Sec-GPC 头发不出去**，只能走 W3C 规范里的 `navigator.globalPrivacyControl` JS 属性（这也是规范允许的表达方式）
   - ❌ 没找到 ITP 显式开关（WebKit 内建行为）
4. **反指纹脚本的唯一注入点**：`extensions.rs::inject_extensions`（UserContentManager，webview 级）。**关键坑**：它先 `remove_all_scripts()` 再注入 items —— 所以所有脚本（用户扩展 + 广告拦截 + 隐私防护）必须**一起拼在 items 里**，不能单独二次注入，否则会被下一次注入清掉。
5. **分档设计的价值**：加噪/裁剪 API 必然有兼容性代价，**给用户三档（标准/严格/关）比给一个总开关更好**——日常标准、敏感站点严格、坏站临时关。状态存 `settings` 表并**同步旧布尔键**避免两处状态打架。
6. **WebRTC 的正确防护姿势**：不要整体 `set_enable_webrtc(false)`（会废掉视频会议），而是**包装 RTCPeerConnection 过滤 `icecandidate` 里 `typ host` 的候选**（内网 IP 泄露主因），保留公网候选。
7. **必备自测工具**：`creepjs.org`（最严，看熵值）、`coveryourtracks.eff.org`（EFF 唯一性评分）、`browserleaks.com/webrtc`（IP 泄露）、`privacytests.org`（逐项能力对比）。
**结果**: 提交 696bf7f(调研报告) / d5a0fde(P0) / 0d4d4ea(P1-P2) / a06ada0(三档)；59 测试全过、clippy 零警告

---

## 2026-09-18 13:20 — 两起"防护误伤"事故复盘：字体探针 + 广告拦截子串匹配（correction）

**类别**: correction
**背景**: 隐私防护与广告拦截上线当天，连续两起"页面能开、内容空白"的用户反馈
**事故一：字体探针防护导致邮箱正文空白**
- 做法：把 `document.fonts.check` 改写为"非通用字体一律返回 false"（想收敛系统字体指纹）
- 后果：部分站点用该接口判断"字体是否就绪"来决定是否渲染内容，收到 false 便卡住不渲染
- 教训：**凡是"改写返回值"的防护（而非加噪），都有被站点当成业务逻辑信号的风险**；标准档只该保留加噪与无副作用的归一化，"改写返回值"类应放严格档
**事故二：广告拦截宽泛规则导致邮箱正文空白**
- 做法：CSS 用了 `iframe[src*="ad"]`、`img[src*="ad."]`、`[class*="banner"]`、`[id*="banner"]`、`[class*="advert"]` 等**子串匹配**；文本关键词含宽泛词「推广」
- 后果：邮箱正文的 iframe / 图片 URL 或 class 里带 "ad"/"banner" 就被整块隐藏；正文含"产品推广"字样同样被隐藏
- 关键线索：用户一句"关掉广告拦截就正常了"直接定位——**排查时要主动请用户做功能二分（关掉某模块试试），比读代码猜快得多**
- 教训：**子串匹配（`[src*="x"]`、`[class*="x"]`）是内容拦截的头号危险动作**，"ad" 两字母出现在正常资源名里的概率极高；只认完整标识符（`adsbygoogle`、`div-gpt-ad`、`doubleclick.net` 这类高置信度值）
**事故三（隐性）：隐私设置无法保存**
- 根因：后端 `set_setting` 有 `ALLOWED_KEYS` 白名单校验，新增的三个隐私键忘了同步加入 → 写入被拒
- 教训：**"前端可写设置键"与"后端白名单"是耦合清单，没有测试覆盖就会漏**；新增设置项时必须检查后端白名单
**通用的三条护栏（写进后续开发的肌肉记忆）**:
1. 任何"拦截/隐藏/改写"类防护，先在**真实业务页面**（邮箱、网盘、在线文档）验证，别只在搜索引擎上测
2. 防护项按"是否有副作用"分档：无副作用（GPC/参数清洗/加噪）→ 默认开；有副作用（改写返回值/全局 Cookie 策略/TLS 底层）→ 默认关或严格档
3. 用户报"内容空白"类问题时，**先请用户关闭对应模块做二分**，拿到方向再改代码
**结果**: 提交 cbae257(字体探针移档) / 69a50de(广告拦截收紧) / 36b90d6(设置白名单补键) / e2bfc90(回退 TLS+三方Cookie+UA)

---

## 2026-09-18 14:10 — WebKitGTK 性能优化：测量方法论 + 两个有效杠杆（best_practice）

**类别**: best_practice
**场景**: ShiftBrowser 性能优化（CPU / 内存）
**测量基线（1 标签空闲 / 9 标签使用中）**:
- 启动到可用：**1 秒**（解锁后），无需优化
- CPU：主进程 13.7% + 渲染进程 17-18%（单标签空闲）
- 内存：单标签 ~780MB；**9 标签合计 3.75GB**（每标签一个渲染进程，240-500MB）
**踩过的测量坑**:
- 想验证"壁纸动画吃多少 CPU"，直接改磁盘 CSS 让用户刷新后测 —— 结果用户当时开着 9 个标签在正常浏览，**变量完全不可比**，数据作废。
- 教训：**性能测量必须受控**（固定标签数、无用户操作、静默期采样）。在用户正在使用的实例上测 CPU/内存，只能得到"氛围数据"，不能用来判断某项改动的效果。要么造受控场景（新起实例、只开一个启动页），要么用**同版本前后对比 + 多次采样取稳态**。
**两个真正有效的杠杆**:
1. **CSS 动画提升合成层**：`.bg-layer` 的 45s 无限缩放动画缺 `will-change: transform` / `translateZ(0)` 时，WebKit **每帧在 CPU 上重绘**110% 视口的高清壁纸位图。加上后主进程 13.7%→11%、渲染进程 17%→13%。**规律：任何持续 CSS 动画都要检查是否被提升为合成层**（transform/opacity 类动画才可能上 GPU）。
2. **MemoryPressureSettings（WebKitGTK v2_34+）**：`WebsiteDataManager::set_memory_pressure_settings()` 是**静态全局** API，必须在创建任何 WebContext **之前**调用；参数 `memory_limit`(MB) + `conservative_threshold`(0.33) + `strict_threshold`(0.5) + `kill_threshold`(0.9) + `poll_interval`(1s)。字段名与阈值语义见 webkit2gtk 的 `auto/memory_pressure_settings.rs`。**实践取舍**：kill_threshold 设高（0.9），宁可多回收缓存也不杀渲染进程，否则用户标签会突然崩溃重载。
**架构性限制（不要硬碰）**:
- WebKitGTK 默认 `MultipleSecondaryProcesses`（每标签一进程）。改 `SharedSecondaryProcess` 曾在实测中触发 WebKit bug（WebLoaderStrategy internallyFailedLoad）导致整个应用崩溃 —— 且主 webview 在 setup 前已创建，进程池已固化，运行期切换会被静默忽略。
- 因此多标签内存的**正经解法**是"休眠/丢弃久未使用的标签"（应用层），而不是改进程模型。
**结果**: 提交 360d387(合成层) / 3b279c4(内存压力治理)；59 测试全过、clippy 零警告

---

## 2026-09-18 14:35 — WebKitGTK 标签休眠（内存回收）实现要点（best_practice）

**类别**: best_practice
**场景**: ShiftBrowser 多标签内存治理（9 标签 3.75GB → 应用层回收）
**背景**：WebKitGTK 默认每标签一个渲染进程、**不主动回收**；改进程模型会触发 WebKit bug（实测崩），
所以正解是应用层"休眠"：把久未激活标签的 webview **销毁**，点回来再按 URL 重建。
**实现骨架（可直接复用）**:
1. **状态**：`Tab` 加 `hibernated: bool`（`#[serde(default)]` 传给前端展示）；
   `TabManager` 加 `last_active: Mutex<HashMap<String, i64>>` 记录最后激活的 Unix 秒。
2. **打点**：`create_tab` / `switch_tab` 里调 `touch_active(id)`。
3. **休眠命令** `hibernate_idle_tabs`：读设置阈值（默认 30 分钟，0=关闭）→ 筛出
   「非当前激活 + 未休眠 + `last_active < now-阈值`」→ 对每个 `wv.hide()` 后 `wv.close()`
   → 置 `hibernated=true` → `emit("tabs-updated")`。返回休眠数量便于日志/统计。
4. **唤醒**：在 `switch_tab` **校验标签存在之后、切换可见性之前**插入恢复分支 ——
   若 `hibernated`，用保存的 url 调 `create_webview(app, id, url)` 重建（id 复用即可，
   label 仍是 `tab-{id}`），清标记，并 `extensions::re_inject(&app).await` 重新注入
   用户脚本/广告拦截/隐私防护（重建的 webview 是干净的，不注入就没有防护）。
   之后沿用原有激活与可见性逻辑，不必重复实现。
5. **前端**：复用已有 8s 轮询，每 8 轮（≈64s）`invoke('hibernate_idle_tabs')` 一次；
   新设置键 `tab_hibernate_minutes` 记得加进后端 `ALLOWED_KEYS`（这个坑刚踩过）。
**验证方法论（值得复用）**:
- 默认阈值 30 分钟无法即时验证 → **临时把 settings 里的阈值改成 1 分钟**，
  请用户在受控场景下操作（开 2-3 个标签、晾着），观察日志闭环：
  `标签已休眠` → 用户点回 → `唤醒休眠标签: url=...` → `page_load_finished: title=...`。
- 没有 sqlite3 CLI 时可用 `python3 -c "import sqlite3..."` 直接改运行时设置（WAL 支持并发读写）。
- **注意**：用户在自然使用中开/关标签会让"内存净效果"不可测（新开的比休眠的多），
  功能正确性靠日志链路验证，净收益要在受控场景才谈得上。
**结果**: 提交 966120c；实测休眠 3 个标签 → 点回全部按原 URL 正常恢复；59 测试全过、clippy 零警告

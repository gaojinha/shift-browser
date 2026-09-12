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

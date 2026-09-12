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

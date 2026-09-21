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

## 2026-09-19 10:45 — Tauri 发布版改了前端却不生效（dist 编译期嵌入，构建顺序反了）

**类别**: correction
**场景**: ShiftBrowser，改完前端（AISidebar 改成调用 set_webview_ai_panel）后，依次跑 `cargo build --release`（后端）→ `npm run build`（前端）→ 重启。结果应用里跑的还是**旧前端**（日志仍见 `set_webview_visible source=ai-sidebar`），白折腾一轮。
**错误**: `tauri.conf.json` 里 `frontendDist: "../dist"`，发布构建时 Tauri 用 `generate_context!` 在**编译期**把 dist 目录**嵌进二进制**。所以必须**先构建前端、再编译 Rust**；顺序反了，二进制里嵌的是上一次的旧 dist。
**正确**: 铁律——改前端必按此顺序：
```bash
npm run build && cargo build --release   # 前端在前，Rust 在后
```
(改纯 Rust 时前端可省，但只要碰过前端就必须重打。)
**排查手法**: 日志里的行为特征能反推跑的是哪版前端（如旧版仍调用已删除的命令/带旧 source 标识 `ai-sidebar`）。
**预防**: 怀疑"改了没生效"时，先确认二进制里的 dist 是不是最新——看日志行为特征，或对比 dist 产物 hash 与编译时间。
---

## 2026-09-19 11:40 — gtk-rs 方法名歧义：WebInspector::show 被 gtk 的 show 抢（E0599）

**类别**: correction
**场景**: ShiftBrowser 启用 Web Inspector（`open_devtools` 命令），`inspector.show()`
**错误**: 编译报 `E0599: the method 'show' exists for struct 'WebInspector', but its trait bounds were not satisfied` —— 因为 `WebInspector` 同时受 `webkit2gtk::WebInspectorExt` 和 `gtk::prelude::WidgetExt/NativeDialogExt` 影响，同名 `show()` 解析到了 gtk 的 trait，而 `WebInspector` 并不实现 `IsA<gtk::Widget>`。
**正确**: 显式走 trait 全路径调用（tabs.rs 里已 `use gtk::prelude` 等，add import 会引入歧义）：
```rust
webkit2gtk::WebInspectorExt::show(&inspector);
```
**预防**: gtk-rs 生态里 `show/hide/set_size_request` 等通用名在多个 trait 里重名，遇到 E0599 优先用 `TraitName::method(&obj)` 全路径形式，别盲目加 use。
---

## 2026-09-19 15:10 — 内容过滤器规则 JSON 非法转义 + glib::Object 非 Send/Sync（FFI 两坑）

**类别**: correction
**场景**: ShiftBrowser 广告拦截 2.0，手写 FFI 对接 WebKit 内容过滤器（`WebKitUserContentFilterStore`）
**错误 1（JSON 转义）**: 规则常量写成普通字符串 `".*adservice\\.google.*"`，Rust 解码后是**单个**反斜杠 `\`，嵌进 JSON 成为 `"\.` —— **JSON 不认 `\.` 转义**，`serde_json::from_str` 解析失败（单测抓到）。要 JSON 里出现一个反斜杠必须写 `\\`，即 Rust 侧需**两个**反斜杠（raw 串同理）。
→ 最终改用**不含反斜杠**的规则（`.*adservice.*`，正则 `.` 匹配任意字符含字面点），彻底绕开转义歧义。
**错误 2（Send/Sync）**: `static OnceLock<Mutex<Option<glib::Object>>>` 编译失败——`glib::Object` 内含裸指针，**不满足 Send/Sync**。改为存 `usize`（指针数值）并注释说明"自己持有 GObject 引用、故意不释放，生命周期覆盖进程"。
**正确**: 跨线程共享 GObject 指针的正确姿势：存 `usize`/裸指针 + 明确所有权与释放策略；不要试图把 glib 对象塞进 static。
**预防**: 给引擎写 FFI 时①任何"字符串嵌 JSON"的场景先写单测验证可解析；②跨线程共享 GObject 一律用裸指针+显式所有权，glib 对象默认不是 Send/Sync。
---

## 2026-09-20 01:40 — 批量替换代码时误伤同名调用点（Python 全局 replace 的坑）

**类别**: correction
**场景**: ShiftBrowser 架构解耦，用 Python 脚本把 tabs.rs 里 6 处重复的布局公式替换成引擎层调用
**错误**: 用 `s.replace(old, new)` 做**全局**替换时，同一句代码在多个上下文出现（如 `apply_to_webview(&window, ...)` 在 resize 闭包、AI 面板命令、书签栏命令里都有），结果把**不需要改的位置也改了**——resize 闭包里引入了只在该作用域存在的 `win_for_geo`，AI 面板命令里被替换成未定义变量，编译报 3 个 `E0425/E0505`。
**正确**: 批量替换前先 `grep -c` 确认目标片段出现次数；多次出现时用 `replace(old, new, 1)` 限定次数，或按行号定位修改；替换后立刻 `grep -n 新标识符` 检查所有落点是否都在预期作用域内。
**预防**: 代码改写脚本必须"替换前数出现次数 + 替换后查落点"，尤其在**闭包/不同作用域**并存的文件里。全局替换只适用于确定唯一的片段。

## 2026-09-20：删 WebViewExt use 导致 4 个 E0599（tabs.rs）
- **现象**：把 `use webkit2gtk::WebViewExt;` 删掉后，`connect_load_failed`/`connect_title_notify`/`context()` 全部报 E0599 "trait bounds were not satisfied"。
- **根因**：GTK 信号连接方法（connect_*）是 trait 方法，必须 trait 在作用域才能调用；它们不是引擎能封装的"基础操作"（回调体内是大量业务逻辑）。
- **纠正**：恢复 use。**边界原则**：引擎层收敛"基础操作"（设置/JS 执行/刷新/导航/检查器），信号回调留在业务层；删 use 前先 grep 确认所有 trait 方法调用点。

## 2026-09-20：python 批量替换断言失败被静默跳过（tabs.rs 两处未生效）
- **现象**：批量替换 2 处 vbox 查找，脚本里 `assert old in s` 失败但没退出（python 默认 assert 失败抛异常，但当时用了 `if old in s` 分支且只 print 不退出），编译通过（因为新函数 parent_vbox 报 never used 才暴露）。
- **根因**：实际文件缩进与脚本假设不一致（8/16 空格 vs 脚本里的不同缩进），替换被跳过。
- **纠正**：批量替换脚本必须 `assert old in s, "标识"` 失败即抛异常退出；替换后 grep 验证目标函数零残留、新函数有调用点。

## 2026-09-20：标签静音"假实现"的教训（toggle_mute 只翻转内存字段）
- **现象**：前端点"静音"只有图标变化，声音照常出。代码注释写着"WebKitGTK 无 per-tab 音量 API"。
- **真相**：WebKitGTK 2.36+ 有 `webkit_web_view_set_is_muted`（系统库符号确认），gtk-rs 绑定 `WebViewExt::set_is_muted` 完整存在（auto/web_view.rs 1950 行）。
- **纠正**：新功能/修复前先查引擎能力（系统库 nm + 绑定源码），不要轻信旧注释；"无 API"类结论要留证据。

## 2026-09-20：content_snapshot 死锁 bug（recv_timeout 在 with_webview 闭包内）
- **现象**：AI 面板发消息"发送不了"——实际是主线程被卡 5 秒（UI 冻结），快照超时降级后才发出。
- **根因**：`extract_snapshot` 内部 `recv_timeout(5s)` 被调在 `with_webview` 闭包里（主线程执行），阻塞主线程 → evaluate_javascript 回调也要主线程 → 互相等待死锁 → 5 秒超时。
- **纠正**：拆成 `extract_snapshot_async(wv, tx)`（只发起不等待），`recv_timeout` 移到 with_webview 闭包外（async 线程）等待。**铁律：with_webview 闭包内绝不能做同步阻塞等待**（channel/锁/睡眠），只能发起异步操作。

## 2026-09-20 — llms.txt 引擎幽灵符号 + 碎片化打补丁（Rust 7 错修到 0）

**类别**: correction
**场景**: ShiftBrowser 阶段 1.5 llms.txt 引擎收口，cargo build 报 7 个错，其中 4 个是"幽灵符号"
**错误 1（幽灵符号）**: 引擎里引用了**根本不存在**的东西——`fetch_llmstxt_raw`（凭空编的出网函数名）、`LlmsCacheEntry`（真名是 `CacheEntry`）、`read_llmstxt` 漏标 `#[tauri::command]`、`content.rs` 用了 `WebmcpToolDef` 却没 `use`。E0425/E0422/E0433 全指向这些。
**错误 2（碎片化·最严重）**: 修的时候"改一行→编译→再看"，结果在 `Option` 嵌套上反复改了五六遍——`fetch` 块返回 `Option<LlmsSummary>`、`CacheEntry.summary` 是 `Option<LlmsSummary>`、函数返回 `Option<LlmsSummary>`，三层没一次理清，白白耗掉一下午。用户点破："一下午了？？？"
**错误 3（Send）**: `MutexGuard` 跨 `.await` → Tauri 命令 Future 非 `Send` 被拒。光 `drop(guard)` 不够（编译器仍保守判定"maybe used later"），必须用**显式作用域块** `{ let guard = ...; ... }` 把 guard 关死在块内。
**正确**: ①Tauri 命令带引用参数必须返回 `Result<T,E>`；②同步锁 guard 绝不跨 await，用作用域块限定；③编译错误成堆时，先通读相关函数、把每个值的类型（尤其 Option/Result 嵌套）理清，**一次性重写整段**，禁止逐行补丁。
**预防**: 编译错误 >3 个 → 停下来通读报错文件的整段逻辑，画清类型流再动手；一个错误改超过 2 次还没对，就说明没读懂，回去重读而不是继续试。

## 2026-09-20 — python 嵌 Rust 字符串混入草稿占位符（TemporaryId 污染）

**类别**: correction
**场景**: 阶段 1.9 整批重试，用 `python3 - <<'PYEOF'` 在 bash 里拼一大段 Rust 代码做插入。
**错误**: 插入的代码里混进了 `TemporaryId` 这种**草稿占位符**——`let plan = batch_plan(&pairs TemporaryId);`，编译直接报 `expected one of ..., found TemporaryId`。根因：写 python 字符串时把"设计草稿"里没清理的占位文本一起粘进去了；Rust 代码越长，手滑混入垃圾的概率越高。
**正确**: **禁止用 python/bash 嵌大段 Rust 代码做插入**。改 Rust 一律用 `edit` 工具做精确 oldString→newString 替换（先 `read` 拿真锚点）。python 只用于"纯文本小替换 + assert 唯一性"这类简单场景。
**预防**: ①任何"生成代码"的操作，生成后必须 `cargo build` 验证，别信"✓ 已插入"的自我报告；②插入前先 `git status` 确认工作区干净，出问题 `git checkout --` 秒回干净基点；③本会话还犯过"反复 sed 读同一文件，把多次输出叠加当磁盘真身"的幻觉——**磁盘真身只认 `read` 工具或 `git status`，不认 bash 里滚动的碎片输出**。

## 2026-09-21 — 第45章 sync.rs：凭记忆写"幽灵 API"复发 + 目录幻觉 + 长文件截断

**类别**: correction
**场景**: 上云 E2E 加密同步（第45章），新写 `src-tauri/src/db/sync.rs`。
**错误 1（幽灵 API，复发）**: 直接凭记忆写 sync.rs，引用了一堆**真身不存在**的符号——`Db::app_data_dir`（Db 只有 pool/key_path/vault_path/master_key 四个字段）、`passwords::encrypt/decrypt`（当时还是私有 `fn`）、自造的 `master_key_or_reject`/`write_atomic_060`/`read_internal_str` 等。`cargo build` 报 14 个 `E0433`（`cannot find __cmd__* in passwords` 是连带症状）。
**错误 2（目录幻觉）**: 一度把项目根记成 `/home/gao/sync-browser-tauri`（真身是 `/home/gao/shift-browser-tauri`），把代码写进误建的空壳目录，靠 `git -C <dir> log` 对不上历史才识破。
**错误 3（长文件 write 截断）**: 一次性 write 超长 Rust 文件，JSON 被截断成 `Unterminated string`，落盘成半截残稿（因未在 `db/mod.rs` 注册而未参与编译，但污染工作区）。
**正确**: ①**写跨模块调用前，先 grep 被调符号的真身签名贴进对话，再从输出抄**，绝不从记忆造；②项目根只认 `git -C <dir> log` 能对上历史提交的那个；③长文件用 `write` 分块（≤250 行）+ `edit` 拼接，每块后 `cargo build` 验证；④残稿立即 `rm` + `git checkout --` 回绿线，不留尸。
**预防**: 编译错误成堆时，**先怀疑"我引用的 API 是不是真的存在"**，grep 一遍再说。这与 2026-09-20「llms.txt 幽灵符号」是**同一类病**——"凭记忆写 API"是本模型高频失误，必须靠"先 grep 后写"的流程硬约束，不能靠记性。本次最终以"先 grep 真身 → 分块写 → build 全绿 → 108 测试过 → 冒烟 12/12 → 真 Nextcloud WebDAV 验证"收口。

## 2026-09-21 — 垂直标签：GTK 布局"看着合理"的写法把 webview 压成 1×1

**类别**: correction
**场景**: 第47章垂直标签（阶段3.1），内容区需从 x=0 右移到 x=220。
**错误 1（漏调用）**: 新建标签后从未调用 `apply_to_webview`（负责设 GtkFixed 的 margin 定位）——初始化同步 `set_vertical_tabs` 跑时还是 0 个标签、循环空转；新标签只设尺寸没设位置。结果 Fixed 停在 setup 的初始 margin（0,116），垂直模式下 webview 实际落在 (0,116) 而非 (220,72)，**盖住左侧标签栏**（用户："左边标签栏没内容"）。
**错误 2（margin_start 压崩布局）**: 首版修复用 `fixed.set_margin_start(220)` 让 Fixed 右移——GtkOverlay 的叠加子件（`add_overlay`）上 margin_start 会把 Fixed 的分配**压成 1×1**（实测 `wv_alloc=(0,700,1,1)`），webview 直接消失。
**正确**: ①新建标签后立即 `compute + apply` 当前几何；②GTK 里改容器定位**优先用 GtkFixed 内部 `move_(x, y)`**（绝对定位，最可靠），别用叠加子件的 margin；顶部偏移沿用已验证有效的 `margin_top`。最终 `move_(geo.left, 0)` + Fixed 宽度 `left+width` → 实测 `wv_toplevel_pos=(220,72)` 与 HTML content-area 完全对齐。
**预防**: **GTK 布局改动必须实测真实像素**——用 `gtk_widget_translate_coordinates(wv, toplevel, 0,0)` + `allocation()` 打日志验证，**绝不能靠"应该对"**（本次靠它抓到 allocation (0,0) vs 期望 (220,72) 的偏差）。这与"凭记忆写 API"同源：**GTK 的布局语义（margin vs move_、Overlay 子件行为）不能靠直觉**，必须实测。

## 2026-09-21 — 视频/网页画面闪烁：WebKitGTK DMA-BUF 渲染器 + Intel 核显

**类别**: correction
**场景**: ShiftBrowser 看视频时画面闪烁（历史遗留，用户一直没说，2026-09-21 才提）。
**根因**: WebKitGTK 2.42+ 默认走 **DMA-BUF 渲染路径**，在部分核显（本机 **Intel HD 630 / i915**）上与驱动配合不良 → 网页/视频画面闪烁（社区公认问题，Intel/NVIDIA 驱动组合均有报告）。
**正确**: 在 WebKit 初始化**之前**设环境变量 `WEBKIT_DISABLE_DMABUF_RENDERER=1`，回退到共享内存渲染路径。本项目放在 `lib.rs::run()` 最前面（已有环境变量先例）。
**验证**: `/proc/<WebKitWebProcess>/environ` 确含该变量（注意：**父进程 `/proc/<pid>/environ` 是启动时快照，`set_var` 之后不反映**，要查 WebKit 子进程才看得到）。
**预防**: 遇到"画面闪烁/黑屏/撕裂"类**渲染层**问题，先怀疑 **DMA-BUF 渲染器**（WebKitGTK 2.42+ 的已知坑），而不是业务代码。诊断顺序：先隔离最近改动 → 若历史就存在 → 查渲染路径/驱动（env 开关、`glxinfo`、i915）。
---

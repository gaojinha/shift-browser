# Spike · 扩展体系能力边界（WebKitGTK 2.52.3）

> 阶段 3.2 · 2026-09-21 · 承接 [spike-engine-capabilities-2026-09-19.md](spike-engine-capabilities-2026-09-19.md) 的 Spike #4
> 结论一句话：**网络层请求拦截可行**——但走的是"Web 进程扩展（WebProcessExtension）"机制（独立 .so + C FFI），
> 不是浏览器扩展（chrome.*）那套。这决定阶段 3「真扩展体系」的技术路线。

---

## 一、背景与目标

现状：ShiftBrowser 的"扩展"是**内容层注入**（`UserScript` 在 document_start 注入 JS/CSS，见 `extensions.rs`），
能力边界止于 DOM——**无法在网络层拦截/改写请求**（广告拦截 2.0 走的是 WebKit 内容过滤器，
是声明式规则、非脚本化拦截）。

阶段 3.2 要把"注入脚本"升级为"真扩展"。**关键前置问题**：WebKitGTK 2.52.3 能否做**网络层拦截**
（改请求头 / 拦 URL / 改响应）？本 spike 用 GIR + 系统库符号实证回答。

## 二、方法

只读实证，不改代码：

1. 读引擎 GIR 定义：`/usr/share/gir-1.0/WebKit2WebExtension-4.1.gir`（Web 进程侧）+ `WebKit2-4.1.gir`（UI 进程侧）
2. 读系统库导出符号：`nm -D /lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0`
3. 对照 gtk-rs 绑定覆盖：`webkit2gtk-2.0.2`（当前依赖 `=2.0` + `v2_40`）

## 三、证据与发现

### 3.1 网络层拦截：`WebKitWebPage::send-request` 信号 ✅

GIR（`WebKit2WebExtension-4.1.gir:48966`）：

> "This signal is emitted when @request is about to be sent to the server. This signal can be used to
> **modify the #WebKitURIRequest** that will be sent to the server. You can also **cancel the resource load
> operation** by connecting to this signal and **returning %TRUE**."

信号签名：`send-request(request: WebKitURIRequest, redirected_response: WebKitURIResponse) -> gboolean`
（`when="last"`；服务器重定向时会再次触发，`redirected_response` 带首次响应）。

**含义**：可以在**请求出网前**读到完整请求（URL + headers）、**改写**（如加/删请求头、改 URL）、
**取消**（返回 TRUE）——这就是网络层拦截能力。响应体改写不在这个信号里（只有重定向响应元信息）。

### 3.2 加载机制：独立 .so + 目录注册 ✅

- UI 进程侧：`webkit_web_context_set_web_extensions_directory`（符号 `T`，存在）——指定一个目录，
  引擎从该目录加载 Web 进程扩展 `.so`；`..._initialization_user_data` 传初始化数据。
- Web 进程侧：`.so` 需导出入口 `webkit_web_extension_initialize(WebKitWebExtension*)`
  （或 `_with_user_data` 变体），GIR:47915/47932。
- 拿到 `WebKitWebPage`：`webkit_web_extension_get_page`（符号 `T`，存在）；`WebExtension` 类的
  `page-created` 信号在页面创建时给出 page。

**含义**：拦截逻辑跑在**独立的 Web 进程扩展 .so** 里，不在主进程——需要单独构建一个 `cdylib` 并随应用分发。

### 3.3 新 `WebExtension` API（2.52 新增）：消息通道，非 chrome.* ⚠️

`WebExtension` 类（2.52 新）方法/信号（GIR）：

| 成员 | 用途 |
|---|---|
| `get_page` | 取某页面 |
| `send_message_to_context` / `_finish` | 扩展 → UI 进程发消息（异步） |
| `page-created` 信号 | 页面创建时通知 |
| `user-message-received` 信号（2.28 起） | UI 进程 → 扩展发消息 |

配套：`WebExtensionMatchPatternError`、`WebExtensionMode`（匹配模式 / 模式枚举，符号存在）。

**含义**：这是"扩展 ↔ UI 进程消息通道 + 页面生命周期"，**没有** `chrome.*` 兼容层、
没有后台脚本、没有 `declarativeNetRequest`——不是完整浏览器扩展模型，只是**通信骨架**。

### 3.4 绑定缺口：gtk-rs 未覆盖 ⚠️

`webkit2gtk-2.0.2` 的 `src/auto/` **没有** web_process_extension 模块（只有 `web_context`、
`uri_scheme_request/response` 等）。即：上述 C API 在 Rust 里**没有现成安全封装**，
要用必须**裸 FFI**（或等上游绑定 / 用 `webkit2gtk-sys` 手写）。

### 3.5 与"旧机制"的分辨

`set_web_extensions_directory` 属 **Web 进程扩展**（旧、稳定、C API）；
`WebExtension` 类属 **2.52 新加的更高层 API**（骨架）。二者**不是一回事**：
前者是"注入代码进 Web 进程"的底层通道，后者是"扩展模型"的雏形。
网络拦截能力在**前者**（`WebKitWebPage::send-request`）。

## 四、结论

| 问题 | 结论 |
|---|---|
| 能否网络层拦截？ | **能**（`WebKitWebPage::send-request`：改请求 / 取消） |
| 怎么实现？ | **独立 Web 进程扩展 .so**（`cdylib` 导出 `webkit_web_extension_initialize`），UI 侧 `set_web_extensions_directory` 加载 |
| Rust 能直接写吗？ | 能，但要**裸 FFI**（gtk-rs 未绑定） |
| 是 chrome.* 那套吗？ | **不是**。2.52 的 `WebExtension` 只是消息通道 + 页面生命周期，无后台脚本/声明式规则 |
| 响应体能否改写？ | `send-request` 不含响应体改写；如需，另找 `WebKitURISchemeHandler` 或自定义协议路径 |

## 五、推荐路线（分阶段）

**阶段 A · 最小原型 — ✅ 已完成（2026-09-21）**

产物：`src-tauri/web-extension/`（Rust `cdylib` + 裸 FFI，无第三方依赖），
入口 `webkit_web_extension_initialize`，在 `page-created` 里连接 `send-request`，
打印每个请求 URL，并对含标记的 URL 返回 TRUE 取消。构建：`scripts/build-extension.sh`
（构建 .so 并拷到 exe 所在目录 = 已注册的 web extensions 目录）。

**运行实证**（真机日志，`/tmp/opencode/shift-browser.log`）：
```
[shift-ext] Web 进程扩展已加载（阶段A 原型）
[shift-ext] page-created
[shift-ext] send-request: shift://newtab
[shift-ext] send-request: https://www.xiachufang.com/
[shift-ext] send-request: https://dup.baidustatic.com/js/ds.js
[shift-ext] send-request: https://sv2.chuimg.com/dist/d2ece0585770c0bb7990.css
[shift-ext] 拦截（取消）: https://dup.baidustatic.com/js/ds.js   ← 返回 TRUE 的取消分支生效
```
即：**加载 → page-created → 每个请求出网前可见 → 返回 TRUE 取消**，全链路打通。

遗留（进阶段 B 前要收）：① .so 的**生产打包**（`tauri.conf.json` resources/bundle，
当前只在 dev 目录手动拷贝）；② 拦截规则的**性能与真规则引擎**（当前是硬编码标记）。

**阶段 B · 能力落地（把网络层用起来）**
用阶段 A 的通道做**脚本化网络拦截**：动态 URL 黑名单、请求头改写（如强制 `Sec-GPC`、UA 策略）、
按域注入请求级策略——补上现有"内容过滤器（声明式）+ UserScript（DOM 层）"缺的**脚本化网络层**。

> **最小版已完成（2026-09-21）**：扩展按**规则文件**拦截——读
> `~/.local/share/com.shiftbrowser.app/shift-ext-rules.txt`（每行一个 URL 子串，`#` 注释），
> 命中即取消该请求。运行实证：`[shift-ext] 已加载 1 条规则 …` +
> `[shift-ext] 拦截（取消）: https://dup.baidustatic.com/js/ds.js`。
> **主程序落盘已完成（2026-09-21）**：设置项 `net_block_rules`（隐私分区文本框，每行一个 URL 子串）
> 经命令 `sync_ext_rules` 写入规则文件；启动时与保存时各同步一次。
> 运行实证：DB 设 `baidustatic` → 日志 `已写网络层拦截规则文件` → 扩展 `已加载 1 条规则` →
> `拦截（取消）: …/ds.js`。一条龙打通。
> 后续：请求头改写（需 SoupHeaders FFI）、规则热更新（当前改规则需重启浏览器）。

**阶段 C · 扩展模型（平台化）**
在 A/B 之上做打包格式 + 清单 + 权限声明，用 `WebExtension` 消息通道连 UI 进程做"扩展管理面板"，
逐步逼近 Tampermonkey/浏览器扩展的开发体验。

**不建议**：等 `chrome.*` 兼容层——2.52 无此能力，且引擎版本冻结（跟随发行版安全通道），短期不会变。

## 六、风险

1. **独立 .so 的分发与 ABI**：扩展 .so 必须与引擎 ABI 匹配；打包/升级要跟引擎版本联动（引擎已冻结，风险可控）。
2. **跨进程调试**：拦截逻辑在 Web 进程，日志/断点与主进程分离，排查成本高。
3. **裸 FFI 的安全边界**：手写 FFI 无 gtk-rs 的类型保障，需自己守所有权/生命周期（参考内容过滤器 FFI 的踩坑）。
4. **性能**：`send-request` 在每个请求上同步触发，规则匹配要快（避免拖慢页面加载）。

## 七、验证方式

- 证据可复现：`grep -n 'send-request' /usr/share/gir-1.0/WebKit2WebExtension-4.1.gir`；
  `nm -D /lib/x86_64-linux-gnu/libwebkit2gtk-4.1.so.0 | grep web_extension`。
- 阶段 A 原型的验收：指定 URL 的请求被拦截/取消（引擎日志或页面行为可见）。

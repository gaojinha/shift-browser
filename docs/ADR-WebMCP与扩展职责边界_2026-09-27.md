# ADR：WebMCP 与 WebKit 扩展的职责边界、转正与废弃判据

> 日期：2026-09-27 · 状态：已接受（开放生态 P1-4）
> 依据：代码实证（文件:行 见 §1）+ `docs/standard-tracking-2026-09-20.md`（标准跟踪）

## 0. 背景

开放生态里有两块"试验性"能力，长期缺一份"何时转正 / 何时废弃 / 谁管什么"
的书面判据，后人接手只能靠考古注释：

1. **WebMCP polyfill**（阶段 1.5 · 标准跟踪）：页面用
   `navigator.modelContext.registerTool(def)` 向 AI 暴露结构化工具，
   本项目注入 polyfill 捕获声明。
2. **WebKit Web 进程扩展**（阶段 3.2 · 真扩展体系 · 阶段 A 原型）：
   网络层拦截的最小证明。

## 1. 现状（2026-09-27 代码实证）

| 层 | 位置 | 行为 | 边界 |
|---|---|---|---|
| WebMCP 捕获 | `src-tauri/src/engine/webmcp.rs`（注入 JS 内联） | document_start 注入幂等 polyfill；**只实现 `registerTool` 捕获**，存 `window.__webmcp_tools`（内存，不落盘不出网） | 不伪造执行能力（无 executeTool/callTool，单测 `polyfill_is_idempotent_and_only_captures` 断言）；默认关（设置 `webmcp_polyfill`） |
| WebMCP 消费 | `src-tauri/src/engine/content.rs:206-210` | 语义快照（content_snapshot）附带前 10 条工具声明给 AI | 只读附带，不执行 |
| Web 进程扩展 | `src-tauri/web-extension/`（Rust cdylib，手写 FFI） | 连 `send-request`，请求出网前读 URL，**含标记即取消**（阶段 A 原型） | webkit2gtk-sys 2.0.2 未覆盖 WebProcessExtension API，故手写最小 FFI；0 测试 |
| DOM 注入 JS | `src-tauri/src/page_*.js` 等（include_str! 嵌入） | read/click/fill/extract 等采集与交互真身 | 经扩展体系注入（`extensions.rs`） |

标准时点（`docs/standard-tracking-2026-09-20.md`）：WebMCP 2026-06 起 Chrome
149–156 公开 origin trial；**WebKitGTK 2.52.3 不支持**（引擎版本冻结）。

## 2. 职责边界（判据，不重叠）

| 关注点 | 归属 | 依据 |
|---|---|---|
| **网络层**（出网拦截 / 请求改写 / 取消） | `web-extension/`（WebKit Web 进程） | 只有 Web 进程扩展能在请求出网前挂钩；主进程侧 net_guard 是事后审计 |
| **DOM 层**（采集 / 交互 / 注入） | `page_*.js` 注入脚本 | 与页面同源执行，能触 DOM；web-extension 触不到页面 window |
| **WebMCP 声明捕获** | `engine/webmcp.rs` polyfill | document_start 注入，必须先于页面脚本执行；属 DOM 层注入，不占网络层 |

**禁止重叠**：网络拦截逻辑不许写进 `page_*.js`（时序不可靠）；DOM 操作不许
写进 web-extension（够不到页面 window）。新增能力先按本表归属，跨层需求
拆成两层各做一半。

## 3. WebMCP 转正判据（全部满足才从"试验"转正）

1. **标准里程碑**：WebMCP 提案离开 origin trial，进入规范草案（或 Chrome
   正式版默认启用）——以标准跟踪文档更新为准，每次刷新 `standard-tracking`
   报告时核对。
2. **引擎支持**：WebKitGTK 提供 `navigator.modelContext` 原生实现
   （引擎版本解冻后）。原生可用时，polyfill 的捕获职责被原生接管，
   代码路径切换而不是并存。
3. **执行判据（单独决策）**：从"只捕获"到"可执行"是安全边界跨越，
   必须另起 ADR：工具执行须走 capabilities 风险表登记（读/写分级）、
   受 `api_readonly` 只读闸门约束、审计落 `ai_audit`——与开放 API
   同一套闸门，不许开旁路。

**任一不满足**：保持现状（默认关 + 只捕获 + 内存不落盘）。

## 4. web-extension 转正判据（阶段 A → B）

1. **规则引擎替代"含标记即取消"**：阶段 B 须接真规则源（广告拦截关键词表 /
   用户自定义规则），替换原型里的硬编码标记判定；规则匹配逻辑进单测
   （现状 `src-tauri/web-extension/src/lib.rs` 0 测试——转正前测试必须先行）。
2. **FFI 收编**：webkit2gtk-sys 覆盖 WebProcessExtension API 后，
   手写 FFI 迁回绑定库（`#[allow]` 遮羞的裸指针代码不许留）。
3. **与 net_guard 口径统一**：拦截规则、私有地址判定与主进程 net_guard
   共用同一实现（SSOT 铁律：同一类判定只许一份），迁移时单测一起搬。

## 5. 废弃判据

- **WebMCP polyfill**：标准连续两个跟踪周期（≈6 个月）无进展（仍停留在
  提案/origin trial 且无新引擎支持）→ 移除 polyfill 注入与 `webmcp_polyfill`
  设置（设置键走迁移保留旧值），语义快照去掉 `webmcp_tools` 字段。
- **web-extension**：若阶段 B 规则引擎因 WebKit 信号 API 限制（`send-request`
  覆盖面不足）无法落地 → 回退到主进程 net_guard 单侧拦截，web-extension
  目录整体移除（含 FFI）。
- 废弃执行时：删代码 + 删设置 + 迁移处理旧值，**文档同步**（本 ADR 标
  "已废弃"，local-api.md 相关表述清除），不留僵尸模块。

## 6. 验证锚点（本 ADR 引用的代码位置，改动时同步核对）

- polyfill 只捕获不执行：`engine/webmcp.rs` 单测 `polyfill_is_idempotent_and_only_captures`
- 快照附带：`engine/content.rs:206-210`（`webmcp_tools` 前 10 条）
- 设置键：`webmcp_polyfill`（`extensions/privacy_cmd.rs` get/set）
- web-extension 入口：`src-tauri/web-extension/src/lib.rs`
（`webkit_web_extension_initialize` + `send-request` 挂钩）

# ShiftBrowser 战略规划（2026 Q4 起）

> 立档时间：2026-09-19
> 决策记录：**本地模型集成暂缓**（2026-09-19 高哥决策，阶段 1 推理层先只做云端 + 多 provider 抽象，本地 provider 留接口不实现）
> 关联文档：`HANDOFF.md`（工程交接）、`docs/browser-privacy-fingerprint-research-2026.md`（隐私调研）

---

## 〇、战略定位（终局）

> **ShiftBrowser 不是"又一个浏览器"，而是 AI 时代的个人网络操作系统——数据主权归用户、AI 原生、可自托管。**

三条不可让渡的底线，所有技术选型服从于此：

1. **数据主权**：用户数据只存本地或用户自己的服务器（Nextcloud/自建），永不进第三方云
2. **本地优先**：能在本地推理/处理就不出网
3. **开放标准**：W3C / MCP / WebMCP / WebExtensions 优先，不锁死在私有协议

---

## 一、五大战略支柱

| 支柱 | 内涵 | 终局形态 |
|---|---|---|
| **A. AI 原生内核** | AI 不是"加个侧边栏"，而是浏览器的一等公民 | 浏览器即 AI 平台：能读页面、能调工具、能执行任务 |
| **B. 隐私主权** | 反指纹、GPC、端到端加密、零遥测、数据自持 | 可验证的"你自己的浏览器" |
| **C. 交互形态** | 分屏、垂直标签、命令栏、空间 | 面向"多任务/多上下文"的桌面形态 |
| **D. 平台化** | 扩展体系、MCP/WebMCP 工具生态、自动化 | 别人能在上面造东西 |
| **E. 工程基座** | 引擎治理、性能、安全、可观测性、发布工程 | 能力持续演进的地基 |

**战略顺序的逻辑**（与收益无关，纯依赖关系）：

- E 是一切的前置——引擎版本债、GTK 布局债不还清，A/B/C/D 全是沙上盖楼
- A 是产品身份的决定项——不做 AI 原生，就只是"套壳"
- B 是长期护城河——AI 时代数据集中化，主权是唯一差异点
- C/D 是规模化的前提——形态与生态决定能不能承载更复杂的工作流

---

## 二、战略原则（贯穿全程）

1. **可回退**：每一步可回滚，不留"改不回去"的状态
2. **渐进增强**：先可用、再可靠、后优雅，不做大爆炸式重构
3. **能力显性化**：所有隐私/AI 能力必须有可观测面板（用户看得见才信）
4. **不破坏既有**：新能力以"开关 + 默认保守"接入，绝不影响现有稳定性
5. **标准先行**：任何自研机制先查 W3C/WICG 是否已有标准，避免造废轮子

---

## 三、分阶段路线图

### 阶段 0 · 基座加固（所有后续阶段的硬前置）

| 项 | 内容 | 交付物 | 验收 |
|---|---|---|---|
| 引擎治理 | ⚠️ **实测修订（2026-09-19）**：系统源冻结在 WebKitGTK 2.52.3，无法升 2.54 → 改为**版本冻结（跟随发行版安全通道）+ 版本跟踪机制 + 升级评估流程**；不自行自编译引擎 | 版本跟踪清单 + 升级评估流程 | 每季度复核上游版本，安全更新及时跟随 |
| 架构解耦 | 抽出**引擎抽象层**（隔离 WebKitGTK 调用），为多引擎/换引擎留口。耦合集中在 4 文件：`tabs.rs`(25/66)、`lib.rs`(15/25)、`extensions.rs`(10/6)、`newtab.rs`(6/0) | `engine/` 模块 | 业务模块不再直接引用 webkit2gtk |
| 能力补齐（实测发现） | ① **广告拦截 2.0**：接 `UserContentFilterStore` 规则制引擎级拦截（✅ v1 内置 75 条；✅ v2 自定义关键词进网络层 + 变更即时重编译免重启） ② **DevTools**：`enable-developer-extras` 启用 Web Inspector（✅ 已交付） | 规则拦截 + 开发者工具 | 拦截效率/覆盖面提升；可审查元素 |
| 快捷键架构（批次 0.3）✅ 已交付 | 现实问题：前端快捷键绑在 HTML 层，**焦点在网页（child webview）时全部失效**（F12/Ctrl+T/W/R/L/H/Tab 时灵时不灵）→ 已在 GTK 顶层窗口统一路由 + 焦点跟踪（`UI_FOCUSED`），页面持焦时 emit `global-shortcut` 给前端执行；UI 层持焦时放行不重复触发 | 全局快捷键层 | 所有浏览器级快捷键在任意焦点下生效 |
| 测试网 | 端到端冒烟测试（标签/布局/弹窗/分屏/翻译/AI），覆盖 GTK 层 | 可重复执行的测试脚本 | 关键路径自动化覆盖 |
| 可观测性 | 结构化日志分级、崩溃/内存压力上报（本地）、健康自检 | 自检命令 + 日志规范 | 能一键报当前状态 |
| 技术债清算 | 阅读列表死表、标签静音假实现、固定标签逻辑、多用户数据隔离 | 修复清单 | 无"死代码/假功能" |

**里程碑 M0**：地基就绪，后续阶段可安全叠加。

### 阶段 1 · AI 原生内核（产品身份决定项）

**目标**：浏览器从"AI 能对话"升级为"AI 能理解、能调用、能执行"。

#### 1.1 页面理解层（Content Layer）
- 正文提取（复用 readability）→ **语义快照**：可交互元素（角色/标签/文本/坐标）、表单结构、页面结构树——**不是截图**
- 隐私分级：本地摘要优先 → 用户显式授权才出网
- 交付：`content/extract` + `content/snapshot` 两个能力域

#### 1.2 推理层（Provider Layer）
- 多 provider 抽象：**云端（现有 AGNES）+ 预留本地/自建接口**
- 模型路由（按任务/隐私级别选 provider）、流式、取消、配额
- **⚠️ 决策（2026-09-19）：本地模型集成暂缓**——本轮只实现 provider 抽象层与云端 provider，本地 provider（Ollama/llama.cpp）留接口不实现
- 交付：`ai/provider/*`，设置页可配多端点

#### 1.3 工具层（Tool Layer）
- **MCP 客户端**：接本地/远端 MCP server（文件、搜索、日历、Nextcloud）
- 工具注册表 + 权限声明 + 调用审计日志
- 交付：AI 面板能调用外部工具

#### 1.4 执行层（Agent Layer）
- 语义快照 → LLM function calling → 动作执行（点击/填写/滚动/读取）
- **安全铁律**：动作白名单、每步人工确认、仅当前标签、凭据永不外泄、注入防护
- 交付：单标签内的多步任务执行

#### 1.5 标准跟踪
- **WebMCP** origin trial 跟踪 + polyfill 试验；**Agentic Browsing 审计**（Lighthouse 13.3）对标

**里程碑 M1**：AI 助手能读页面、能调工具、能在授权下执行多步任务。

### 阶段 2 · 隐私主权（长期护城河）

**目标**：从"有防护"升级为"可验证的主权"。

| 域 | 内容 | 依据 |
|---|---|---|
| 反指纹 2.0 | 双模式：**加噪**（现）与**统一化**（Tor/Mullvad 派）；指纹自测面板 | 指纹识别率 80-90%，已成主流追踪手段 |
| GPC | 请求头 `Sec-GPC: 1` + JS 属性 + `/.well-known/gpc.json` 感知 | 加州 2027-01 强制，12 州立法 |
| Passkeys | WebAuthn/FIDO2 支持（先 spike 验证 WebKitGTK 能力） | FIDO：50 亿活跃 passkey |
| 数据加密 | 数据库整体加密（SQLCipher）或字段级；密码库主密码 | 数据主权底线 |
| 同步 | **端到端加密**同步（Nextcloud/自建），服务器零知识 | 自托管优势 |
| 零遥测审计 | 全量网络出口清单 + 可验证"无外联" | 信任需要证明 |

**里程碑 M2**：数据可自持、可验证、可迁移；合规能力对齐 2027 法规窗口。

### 阶段 3 · 交互形态与平台化

#### 3.1 交互形态
- **分屏浏览**（双页/多页并排，复用 AI 分屏已验证的 webview 让位机制）
- **垂直标签** + **命令栏（Ctrl+K 万能入口）**
- **空间（Spaces）**：工作区升级为"上下文隔离容器"（账号/代理/主题/标签集）

#### 3.2 平台化
- **扩展体系升级**：WebKitGTK 2.52+ 的 **Web Extensions API**（`WebKitWebProcessExtension`）→ 从"注入脚本"走向"真扩展"，评估网络层拦截能力
- **用户脚本生态**（Tampermonkey 风格）
- **自动化/工作流**：Agent 编排、定时任务、批量操作

**里程碑 M3**：形态对齐前沿，第三方可在平台上开发。

### 阶段 4 · 开放生态（持续）

- WebMCP 工具市场 / 开放 API
- 多端形态（移动/平板协同）
- 自托管服务端套件（同步/工具/模型网关）

---

## 四、架构演进方向

```
现在:  lib.rs 大装配 + tabs/ai/extensions 平铺

目标:  ┌ engine/       引擎抽象（隔离 WebKitGTK，为多引擎留口）
       ├ content/      正文提取 · 语义快照 · 净化
       ├ ai/           provider(云/预留本地) · router · tools(MCP) · agent
       ├ privacy/      反指纹 · GPC · 加密 · 遥测审计
       ├ sync/         端到端加密同步（Nextcloud/自建）
       ├ platform/     扩展 · 用户脚本 · 自动化
       └ ui/           形态层（分屏/垂直标签/命令栏/空间）
       + 既有 db/ tabs/ 逐步下沉
```

**重构原则**：按能力域切分，逐个迁移；迁移一个、验证一个，绝不大爆炸。

### 4.1 引擎耦合现状（批次 0.2 输入）

| 文件 | WebKit 调用点 | GTK 调用点 | 迁移优先级 |
|---|---|---|---|
| `tabs.rs` | 25 | 66 | 最高（主战场） |
| `lib.rs` | 15 | 25 | 高（装配层） |
| `extensions.rs` | 10 | 6 | 中 |
| `newtab.rs` | 6 | 0 | 低（纯生成） |

关键 API 使用频次：`with_webview` 18 · `evaluate_javascript` 9 · `WebContext` 8 · `Settings` 6 · `UserContentManager` 5 · `add_script` 1 · `run_javascript` 1

---

## 五、前置技术验证清单（spike）

> 📄 实测证据与详细结论见：`docs/spike-engine-capabilities-2026-09-19.md`

| # | 验证项 | 用途 | 若不可行的退路 | 状态 |
|---|---|---|---|---|
| 1 | 引擎升级可行性 | 阶段 0 起点 | — | ✅ **实测：不可行**（系统源冻结 2.52.3）→ 改为版本跟踪 |
| 2 | WebAuthn/Passkeys 在 WebKitGTK 的可用性 | 阶段 2 | 降级为"密码管理器增强" | ⚠️ 部分完成（引擎有实现，需运行时实测平台认证器） |
| 3 | `send-request` 信号可改请求头 | GPC 实现 | — | ✅ **实测：可行**（web process extension `send-request` + 可变 `SoupMessageHeaders`） |
| 4 | WebKitWebProcessExtension 能力边界（能否网络层拦截） | 阶段 3 扩展体系 | 维持注入式 + 自研规则 | ⚠️ 部分完成（骨架 API 在，能力面窄，需专项 spike） |
| 5 | WebGPU 在 2.54 的启用状态 | 平台能力 | 保持指纹防护定位 | ✅ **实测：引擎有、无公开开关** → 不作依赖 |
| 6 | 本地模型集成路径（Ollama 兼容性/性能/内存） | 阶段 1 推理层 | — | ⏸ **暂缓（2026-09-19 决策）** |

### 5.1 意外收获（实测发现，可直接转化为能力）

| 发现 | API | 用途 | 建议归属 |
|---|---|---|---|
| 引擎级内容过滤 | `WebKitUserContentFilterStore` + `UserContentManager.add_filter` | 广告拦截从"CSS+关键词"升级为**规则制引擎级拦截**（Safari 风格 JSON 规则，带缓存） | 阶段 0 能力补齐 |
| 开发者工具可启用 | `WebKitSettings.enable-developer-extras` | 启用 Web Inspector（当前 0 命中） | 阶段 0 可观测性 |
| 预取/硬件加速开关 | `enable-dns-prefetching`、`hardware-acceleration-policy` | 性能调优入口 | 阶段 0/3 |

---

## 六、风险与制约

| 风险 | 说明 | 对策 |
|---|---|---|
| 引擎能力天花板 | WebKitGTK 不是 Chromium，MV3 级扩展、完整 WebMCP 未必能做 | 每阶段先 spike；能力边界写进规划 |
| GTK 布局定制债 | 多 webview 靠 GtkFixed 手工布局，新形态（分屏/垂直标签）都要在这上面加复杂度 | 阶段 0 先做引擎抽象与布局重构 |
| Agent 安全 | 注入攻击、误操作、凭据泄露 | 白名单 + 逐步确认 + 只操作当前标签 |
| 单人维护现实 | 阶段 3-4 工作量大 | 每阶段设"最小可交付"，允许阶段内分批 |
| 上游变动 | Chrome MV2 终结、WebMCP 标准演进、Privacy Sandbox 退役 | 标准跟踪机制，每季度复核规划 |

---

## 七、治理与节奏

- **每阶段以"里程碑 + 验收标准"收口**，不达标不进入下一阶段
- **每阶段内可分批次交付**，批次之间保持可发布状态
- **每季度一次战略复核**（标准变动/上游变化/能力边界实测结果）
- **所有能力必须可观测**（面板/日志/统计），不许有"用户看不见的玄学功能"

---

## 战略全景一览

| 阶段 | 主题 | 里程碑 | 依赖 |
|---|---|---|---|
| 0 | 基座加固 | 地基就绪 | — |
| 1 | AI 原生内核 | AI 能读/能调/能执行 | M0 |
| 2 | 隐私主权 | 数据自持可验证 | M0 |
| 3 | 交互与平台 | 形态对齐 + 可扩展 | M0/M1 |
| 4 | 开放生态 | 平台开放 | M3 |

---

## 变更记录

| 日期 | 变更 |
|---|---|
| 2026-09-19 | 方案立档；决策：本地模型集成暂缓（阶段 1.2 只做 provider 抽象 + 云端 provider） |
| 2026-09-19 | 完成批次 0.1（引擎能力 spike #1-#5），产出 `docs/spike-engine-capabilities-2026-09-19.md`；修订阶段 0：引擎治理改为"版本冻结+跟踪"（系统源冻结 2.52.3）；新增"能力补齐"项（内容过滤器 + DevTools）；新增 4.1 引擎耦合现状 |
| 2026-09-19 | 交付① **DevTools**（`enable-developer-extras` + GTK 窗口层 F12/Ctrl+Shift+I 拦截，解决"焦点在网页时快捷键失效"）；交付② **广告拦截 2.0 v1**（新增 `engine/` 模块 = 架构解耦第一块砖；手写 FFI 对接 `WebKitUserContentFilterStore`，75 条网络层拦截规则，启动异步编译并缓存，随注入流程挂载；含 2 个单测） |
| 2026-09-19 | 记录现实问题：**前端快捷键绑在 HTML 层**，页面持有焦点时全部失效 → 新增阶段 0 批次 0.3（GTK 层统一路由 + 焦点跟踪） |
| 2026-09-19 | 交付③ **批次 0.3 快捷键路由**：GTK 顶层窗口统一拦截（焦点跟踪 `UI_FOCUSED`），页面持焦时 emit `global-shortcut` 给前端执行，覆盖 Ctrl+T/W/R/L/H、Ctrl+Tab、Ctrl+Shift+T/S、F12；前端抽出统一动作入口 `handleGlobalAction`。阶段 0 的"工程基座"三项（引擎抽象首块砖 / DevTools / 快捷键路由）已就位 |
| 2026-09-19 | 交付④ **广告拦截 2.0 v2**：自定义关键词编译进网络层规则（清洗正则元字符、上限 200 条）；规则变更在主线程重编译，编译完成后自动重新注入 → **免重启即时生效**；编译触发点前移到注册之后（修掉启动首次跳过的时序问题）；单测扩到 3 个（含关键词清洗）。实测：加 1 个关键词规则 5231→5304 字节 |
| 2026-09-19 | 交付⑤ **架构解耦（批次 0.2 首项）**：新增 `engine/layout.rs` 收敛内容区几何（常量 + `ContentGeometry::compute` + `apply_to_webview`），**tabs.rs 里 6 处重复的布局公式全部消除**（窗口缩放/切标签/建标签/书签栏切换/AI 分屏/几何重应用），常量改为 `pub use` 引擎层（唯一事实来源）；新增几何单测。全量 63 单测通过、零警告 |
| 2026-09-19 | 交付⑥ **可观测性·自检命令**：新增 `selfcheck.rs`（只读体检）+ `selfcheck_cmd` 命令 + 设置页「🩺 自检」分区。17 项分四组：版本与环境（ShiftBrowser/WebKitGTK/系统/数据目录）、数据存储（连接/表数/书签/历史/密码/主密钥权限）、运行时（标签/主窗口/AI 分屏）、能力状态（内容过滤器/广告拦截/隐私档位/扩展）。启动自动自检并把环境+能力写入日志。实测健康=true；自检如实报出广告拦截当前关闭（DB adblock_enabled=false） |
| 2026-09-19 | 交付⑦ **架构解耦（批次 0.2 第二项）**：新增 `engine/webview.rs`（默认设置 + JS 执行封装），收敛散落的 `evaluate_javascript`（tabs 3 + extensions 5 处）与 `settings.set_*`；耦合度 tabs.rs 25→8、lib.rs 15→6、extensions.rs 10→6、newtab.rs 6→0；65 单测通过、零警告、实测注入/探针链路正常 |
| 2026-09-20 | 交付⑧ **架构解耦（批次 0.2 第三项）**：`engine/webview.rs` 补 4 个 API（console 输出/刷新/导航状态/开发者工具），tabs.rs 4 处调用收敛；tabs.rs webkit 依赖 25→1；边界结论：`connect_*` 信号回调留业务层，引擎层只收基础操作；65 单测、零警告 |
| 2026-09-20 | 交付⑨ **架构解耦（批次 0.2 第四项）**：`engine/layout.rs` 补 3 个 API（parent_vbox/find_or_create_fixed/set_fixed_visible），tabs.rs 4 处布局调用收敛、原 find_or_create_fixed 删除；实测布局初始化正常；65 单测、零警告 |
| 2026-09-20 | 交付⑩ **测试网（E2E 冒烟）**：新增 `scripts/smoke.sh`（12 项检查：进程/数据库/自检/环境/引擎版本/快捷键/布局初始化/reparent/隐私注入/翻译探针/扩展注入/无 panic），支持完整模式与 `--no-restart`；实测 12/12 通过、失败路径正确 |
| 2026-09-20 | 交付⑪ **技术债清算 1+2**：①标签静音真实现（`engine/webview.rs` 新增 `set_muted`，`toggle_mute` 异步派发调 WebKit `set_is_muted`——纠正原"无 per-tab 音量 API"误判）；②删除 `reading_list` 死表（迁移 10，零引用零数据）；65 单测、冒烟 12/12 |
| 2026-09-20 | 交付⑫ **技术债清算 3（固定标签排序）**：`TabBar.vue` 渲染层 `pinnedFirst()`（固定永远最前）+ `stores/tabs.ts` 拖拽保护（固定/普通不可互越界）；vue-tsc + 编译 + 冒烟全过 |
| 2026-09-20 | 阶段 1 启动：**1.1 页面理解层**——`engine/content.rs`（语义快照 Snapshot+DOM 提取 JS，本地不出网）+ `content_cmd.rs`（content_snapshot/content_extract 命令）+ reader 复用 + AI 面板接入（对话前附页面快照，失败降级）；67 单测、冒烟 12/12 |
| 2026-09-20 | 交付⑬ **阶段 1.2 推理层（Provider 抽象）**：新增 `engine/ai_provider.rs`（`AiProvider` trait + `AgnesProvider` + `LocalProvider` 暂缓 stub + `resolve_provider`/`list_providers`），`ai.rs` 抽离 `agnes_chat_impl`/`agnes_chat_stream_impl`（SSRF 校验保留共用），`ai_chat`/`ai_chat_stream` 改走 provider 路由（新增 `provider` 参数，前端 `ai_provider` 设置透传），新增 `ai_list_providers` 命令 + 设置页 Provider 下拉（local 置灰提示暂缓）；async-trait 依赖（dyn 兼容）；**测试修正**：resolve 测试原依赖环境变量（测试机有 AGNES_API_KEY 会误判）改为掩码 key 过滤语义（环境无关）；71 单测、零警告、冒烟 12/12 |
| 2026-09-20 | 交付⑭ **阶段 1.3 工具层（MCP 客户端）**：新增 `engine/mcp.rs`（stdio 传输 JSON-RPC 2.0 客户端：进程 spawn + initialize 握手 + 30s 请求超时 + notification 不等待；连接缓存；工具注册表 `list_tools`（白名单过滤，`server__tool` 全局唯一名）；`call_tool` 统一入口（白名单校验 + 审计写库）；`get_audit_log`）+ `mcp_cmd.rs` 7 命令（list/add/remove/enable/tools/call/audit）+ 迁移 11（`mcp_servers` + `tool_audit_log`）；`ai.rs` 扩展 OpenAI function calling（`ChatMessage.tool_call_id/tool_calls`、`ChatRequest.tools`、`agnes_chat_impl` 返回结构化消息），`ai_chat` 加**工具执行循环**（≤6 轮：模型请求工具→执行→结果回传→再请求，自动携带启用工具）；前端：AI 面板有工具时自动走非流式 function calling、设置页 MCP 分区（server 配置/工具列表/手动调用/审计）；**踩坑**：`notifications/initialized` 无 id 不应答，用 `request` 发会死等 → 加 `notify` 方法 + 30s 超时（端到端测试抓出）；Vue 模板单引号属性值内不能含裸双引号（用 `&quot;` 实体）；74 单测（+1 端到端）、零警告、冒烟 12/12 |
| 2026-09-20 | 交付⑮ **阶段 1.4 执行层（Agent）**：新增 `engine/agent.rs`（动作白名单 9 项 click/fill/scroll/read/wait/navigate/back/snapshot/submit，JS 模板参数 JSON 序列化防注入，密码框拒绝读写，`browser_action` 工具定义）+ `agent_cmd.rs`（**每步人工确认**：`request_agent_action` emit `agent-action-request` 事件 + oneshot 等待 60s 超时，`agent_confirm_action` 前端回调，`execute_agent_action` 仅当前活动标签，`agent_debug_execute` 调试入口）；`ai.rs` 工具循环接入：`browser_action` 走确认流程、MCP 工具走原逻辑；前端 AISidebar 确认卡片（允许/拒绝）+ 工具数提示；**踩坑**：tauri 2 取窗口用 `get_webview_window`（`get_window` 返回 `Window` 类型不匹配）；`wv.navigate` 要 `Url` 类型；evaluate 回调返回嵌套 Result 需解两层；80 单测（+6 agent）、零警告、冒烟 12/12 |
| 2026-09-20 | 交付⑯ **阶段 1.5 标准跟踪（WebMCP + Agentic Browsing）**：调研报告 `docs/standard-tracking-2026-09-20.md`——WebMCP＝Google 提案（W3C Web ML CG 孵化），2026-05-15 Chromium Intent-to-Experiment 提交→三天批准，**Chrome 149–156 公开 origin trial**（I/O 2026 宣布）；工具声明预算 **描述≤500 字符 / 参数描述≤150 / 工具名与参数名≤30 / 单工具输出≤1500 字符**；Lighthouse 13.3.0（2026-05-07）把 **Agentic Browsing 从实验性移入默认配置**，按 **pass/fail 比率**计（无 0-100 加权分），四项信号：WebMCP 工具注册/schema 有效性、代理为中心的无障碍树（名称标签·树完整性·可见性）、布局稳定性 CLS、llms.txt（网域根）；2026-09-15 Cloudflare 新域名默认阻止 Training+Agent 爬虫（agent 流量成为独立治理对象）；**差距分析**：WebKitGTK 2.52.3（引擎冻结）不支持 `navigator.modelContext` 且**无法升级**→走**客户端 polyfill 试验**；**polyfill 落地**：`engine/webmcp.rs`（`WEBMCP_POLYFILL_JS` document_start 注入，幂等捕获 `registerTool` 声明到 `window.__webmcp_tools`，**只捕获不伪造执行能力**，试验开关默认关）+ `engine/content.rs` Snapshot 加 `webmcp_tools` 字段（SNAPSHOT_JS 附带提取）+ `extensions.rs` `collect_enabled_items` 改为返回 `(items, webmcp_on)`、`inject_extensions` 加 `webmcp_on` 参数与 Start polyfill 注入（**8 处调用点全适配**）+ `get/set_webmcp_polyfill` 命令+lib.rs 注册；前端：content.ts Snapshot 加 `webmcpTools`、ai.ts 快照描述附带 WebMCP 工具声明、SettingsDialog WebMCP 试验开关（默认关·试验中徽标）+设置页说明；**标准跟踪机制落地**：每季度复核（下次 2026-12-20）；81 单测（+1）、零警告、冒烟 12/12 |

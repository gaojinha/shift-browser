# 引擎能力实测报告（Spike #1-#5）

> 实测时间：2026-09-19 · 实测环境：Ubuntu 25.10 (questing) / 内核 6.17.0-41 / WebKitGTK **2.52.3** / GTK 3.24.50
> 方法：读取已安装 GIR（`/usr/share/gir-1.0/WebKit2-4.1.gir`、`WebKit2WebExtension-4.1.gir`）+ 引擎二进制符号 + 源码耦合点盘点
> 结论用途：修订 `docs/strategy-2026.md` 阶段 0/2/3 的可行性判断

---

## 一、Spike #1 · 引擎版本升级（结论：**不可行，策略调整**）

| 事实 | 值 |
|---|---|
| 系统发行版 | Ubuntu 25.10 (questing) |
| 已装 WebKitGTK | `libwebkit2gtk-4.1-0` 2.52.3-0ubuntu0.25.10.1 |
| apt 候选版本 | **2.52.3**（与已装一致，来自 questing-updates + questing-security） |
| 上游最新 | 2.54.0（2026-09-16 发布） |

**结论**：系统源只提供 2.52.3，**无法通过 apt 升到 2.54.0**。2.52.3 带发行版安全更新，属"跟随发行版"的维护状态。

**策略调整**（已同步战略文档）：
- 阶段 0 的"引擎治理"从"升级到 2.54"改为：**版本冻结（跟随发行版安全通道）+ 版本跟踪机制 + 升级评估流程**
- 升级到 2.54 的可行路径（未来评估）：① 等发行版升级（26.04 LTS）② 第三方源/自编译（风险高：GNOME 全家桶共用 libwebkit2gtk，牵一发动全身）
- **本阶段不自行升级引擎**（战略原则"可回退"，自编译引擎不可回退）

---

## 二、Spike #3 · GPC 请求头（结论：**可行**）

| 证据 | 说明 |
|---|---|
| `WebKitWebPage::send-request` 信号存在 | 位于 `WebKit2WebExtension-4.1.gir`，Web 进程扩展侧，**请求发出前**可介入 |
| `WebKitURIRequest.get_http_headers` 存在 | 返回 `SoupMessageHeaders`（**可变**），具备修改请求头能力 |

**结论**：GPC（`Sec-GPC: 1`）可通过 **web process extension 的 `send-request`** 注入请求头实现。

**注意**：`WebKitWebResource::sent-request`（UI 进程侧）是**事后通知**，不能改请求——两者别搞混。实测确认信号来自 `WebKitWebResource.cpp`，是通知型。

**落地形态**：需要一个 web process extension（WebKitGTK 的 `webkit_web_context_set_web_extensions_directory` + WPE 扩展二进制），工作量中等，需独立 spike 验证注入链路。

---

## 三、Spike #4 · 扩展体系能力（结论：**基础 API 已存在，能力边界待深验**）

| 证据 | 说明 |
|---|---|
| `WebKit2WebExtension-4.1.gir` 内含 **`WebExtension`** 类 | 2.52 新增的 Web Extensions 初始 API |
| `WebExtension` 方法 | `get_page`、`send_message_to_context(_finish)` |
| 匹配模式 | `WebExtensionMatchPatternError`、`WebExtensionMode`（UI 进程侧） |

**结论**：2.52 已具备 Web Extensions 的**骨架 API**（扩展→页面消息通道、匹配模式），但**方法面很窄**，尚不确定是否支持 `chrome.*` 兼容层、后台脚本、网络层拦截。

**待深验**：扩展打包格式、清单解析、C 侧加载入口（`webkit_web_context_set_web_extensions_directory` 属旧 web process extension 机制，与浏览器扩展不是一回事，需分辨）。

**战略含义**：阶段 3"扩展体系升级"的前置条件**部分满足**——可做，但需一次专门的扩展 API spike。

---

## 四、Spike #2 · WebAuthn / Passkeys（结论：**引擎有实现，平台认证器待运行时验证**）

| 证据 | 说明 |
|---|---|
| 引擎二进制含 `WebAuthn`、`U2F`、`PublicKeyCredential` 符号 | WebAuthn 在引擎层**已编译进去** |
| 引擎链接 `libsecret-1.so.0` | 凭证存储疑似走系统密钥环 |
| GIR 中**无**认证器/凭证 UI 相关 API | 无 `WebKitAuthenticator` 之类公开接口 |

**结论**：引擎支持 WebAuthn 的 Web 平台侧，但**是否有可用的平台认证器（含用户验证 UI）未定**，需**运行时实测**（在真实站点跑 passkey 注册流程）。

**退路**（若不可用）：阶段 2 降级为"密码管理器增强"（当前已有 AES-256-GCM 密码库）。

---

## 五、Spike #5 · WebGPU（结论：**引擎含实现，无运行时开关 API**）

| 证据 | 说明 |
|---|---|
| 二进制含 `WebGPUEnabled`、`WebGPU`、`WebGPUBindingsEnabled` 等符号 | 引擎内有 WebGPU 实现 |
| `WebKitSettings` 中**无** WebGPU 属性 | 无法通过 GIR 公开 API 运行时开关 |
| `WebKitSettings` 可开关项 | `enable-webgl`、`enable-webrtc`、`enable-developer-extras`、`enable-dns-prefetching`、`hardware-acceleration-policy`、`enable-encrypted-media` 等 |

**结论**：WebGPU 存在但**无公开运行时开关**（可能走特性标志/环境变量）。战略上**保持现有定位**（WebGPU 指纹归一化防护），不作为平台能力依赖。

---

## 六、意外收获（本次实测发现，可直接转化为能力）

### 1. 正经的内容过滤 API —— `UserContentFilterStore` ✅
| API | 说明 |
|---|---|
| `WebKitUserContentFilterStore` | `new` / `load` / `load_finish` / `fetch_identifiers` / `get_path` / `save` |
| `WebKitUserContentManager` | `add_filter` / `remove_filter` / `remove_filter_by_id` / `remove_all_filters` |

**含义**：WebKitGTK 提供 **Safari 风格的内容拦截器**（JSON 规则、引擎编译为高效匹配器、带磁盘缓存）。当前项目的广告拦截是"CSS 规则 + 关键词文本扫描"，**可升级为规则制引擎级拦截**——效率与覆盖面都是量级提升。

**建议**：纳入阶段 0 的"技术债清算"或提前到阶段 1 作为"广告拦截 2.0"。

### 2. 开发者工具可开 —— `enable-developer-extras` ✅
`WebKitSettings` 提供 `enable-developer-extras` → 可启用 **Web Inspector**（当前项目 DevTools 0 命中，仅 console 打到 stdout）。

**建议**：纳入阶段 0"可观测性"（开发调试的地基，也为用户提供"审查元素"）。

### 3. 其它已确认可用的 Settings 开关
`enable-dns-prefetching`（可做预取优化）、`hardware-acceleration-policy`（硬件加速策略）、`enable-media-capabilities`、`enable-encrypted-media`、`javascript-can-access-clipboard` 等。

---

## 七、引擎耦合点盘点（架构解耦/批次 0.2 的输入）

**直接依赖 WebKit 的文件与调用点数**：

| 文件 | WebKit 相关 | GTK 相关 | 说明 |
|---|---|---|---|
| `tabs.rs` | 25 | 66 | 主战场：webview 生命周期 + GTK 布局 |
| `lib.rs` | 15 | 25 | 装配层：协议、锁屏 GTK 对话框、扩展注入 |
| `extensions.rs` | 10 | 6 | 注入与广告拦截 |
| `newtab.rs` | 6 | 0 | 新标签页生成 |
| `translate.rs` / `reader.rs` / `db/downloads.rs` | 各 1 | — | 零星 |

**关键 API 使用次数**：`with_webview` 18、`evaluate_javascript` 9、`WebContext` 8、`Settings` 6、`UserContentManager` 5、`add_script` 1、`run_javascript` 1

**结论**：耦合**集中在 4 个文件**（tabs/lib/extensions/newtab），抽象层可按此边界切分，风险可控。

---

## 八、Spike 状态汇总（回填战略文档）

| # | 验证项 | 状态 | 结论 |
|---|---|---|---|
| 1 | 引擎升级可行性 | ✅ 完成 | **不可行**（系统源冻结 2.52.3）→ 改为版本跟踪 |
| 2 | WebAuthn/Passkeys | ⚠️ 部分 | 引擎有实现，平台认证器需运行时实测 |
| 3 | GPC 请求头 | ✅ 完成 | **可行**（web process extension `send-request` + 可变 headers） |
| 4 | WebExtensions API 边界 | ⚠️ 部分 | 骨架 API 在，能力面窄，需专项 spike |
| 5 | WebGPU 启用状态 | ✅ 完成 | 引擎有、无公开开关 → 不作依赖 |
| 6 | 本地模型集成 | ⏸ 暂缓 | 高哥决策（2026-09-19） |

# 设置体系信息架构（栏目）重规划方案

> 范围：ShiftBrowser 的「设置」信息架构——设置弹窗栏目、设置项归属、设置入口分布与导航效率。
> 边界：**纯前端 IA 重组 + 既有设置键补 UI**，不改后端命令、不改 settings 白名单、不改数据模型；所有功能行为零变化、可整批回退。
> 立档：2026-09-25。关联战略：[docs/strategy-2026.md](file:///home/gao/shift-browser-tauri/docs/strategy-2026.md)（五大支柱 A–E）。

---

## 一、现状调研结论

### 1.1 设置弹窗：扁平 10 栏目（无分组、无层级）

入口：[AddressBar.vue](file:///home/gao/shift-browser-tauri/src/components/AddressBar.vue) 齿轮按钮、[StatusBar.vue](file:///home/gao/shift-browser-tauri/src/components/StatusBar.vue) 设置按钮（隐私战绩可深链 `privacy`）、命令栏 Ctrl+K「打开设置」、启动页 IPC。

导航定义见 [SettingsDialog.vue#L35-L46](file:///home/gao/shift-browser-tauri/src/components/SettingsDialog.vue#L35-L46)，10 项一级平铺：

| # | 栏目 | 实际内容（卡片） | 内容域数量 |
|---|---|---|---|
| 1 | 通用 | 默认浏览器、主页、搜索引擎、标签休眠、垂直标签、**3 个定时任务 + Agent 提示词**、翻译源/目标语言/超时 | 4 个不同域 |
| 2 | 外观 | 主题、书签栏、**DMA-BUF 渲染兼容开关**、启动页壁纸开关、字号 | 3 个域 |
| 3 | AI 助手 | Provider、模型、API 地址、Key | 1 |
| 4 | MCP 工具 | server 管理、工具列表、手动调用测试、审计日志（4 卡） | 1 |
| 5 | 扩展 | 本地路径加载 JS/CSS 扩展、启停删 | 1 |
| 6 | 隐私防护 | 浏览防护（档位/反指纹/白名单/统计/自测）、网络与追踪（GPC/广告拦截/规则/请求头/GPC 检测/外联审计） | 2 |
| 7 | 数据安全 | 主密码、E2E 同步、启动锁屏、**记录历史、清历史、清除浏览数据、证书中心、User-Agent 管理** | 4 个域 |
| 8 | 开放 API | eval/端口/绑 LAN/IP 白名单/模型网关/状态面板+token、**WebMCP polyfill 试验开关** | 2 个域 |
| 9 | 自检 | 17 项只读体检（版本/存储/运行时/能力） | 1 |
| 10 | 关于 | 版本、核心能力介绍、隐私承诺、仓库/数据目录 | 1 |

### 1.2 第二套设置 UI：启动页「用户中心 → 偏好设置」

[newtab/index.html#L1072-L1130](file:///home/gao/shift-browser-tauri/src-tauri/resources/newtab/index.html#L1072-L1130) 内嵌独立偏好面板，含 7 项：农历、每日一句、天气、**字体大小**、天气城市、**主题色**、**壁纸 URL**。

- 保存走专用命令 `user_update_prefs`（写 `user_prefs` 表），登录时由 [session.rs#L73 `apply_prefs_to_settings`](file:///home/gao/shift-browser-tauri/src-tauri/src/db/users/session.rs#L73) 单向同步进 `settings` 表；
- 设置弹窗同样消费 `settings` 表（show_lunar 等 7 键均在 [contracts.rs](file:///home/gao/shift-browser-tauri/src-tauri/src/contracts.rs) 白名单），但**弹窗内没有这些键的 UI**。
- 结果：用户在「外观」栏找不到主题色/壁纸/字号（字号弹窗内又有一份），两套设置界面、两处心智负担。

### 1.3 管理弹窗中的设置型入口（第三类入口）

密码库（[PasswordsDialog](file:///home/gao/shift-browser-tauri/src/components/PasswordsDialog.vue)，地址栏按钮）、用户脚本（[UserscriptsDialog](file:///home/gao/shift-browser-tauri/src/components/UserscriptsDialog.vue)，地址栏按钮，设置弹窗内无入口）、证书中心（地址栏锁图标 + 数据安全栏内一个按钮）、书签/历史/下载/会话/分组/空间各自独立弹窗。这些是「数据管理」而非「配置」，保持独立弹窗合理，但**设置弹窗内缺少通往其中"设置型"页面（用户脚本、证书）的导航**。

### 1.4 设置项覆盖度盘点（42 个白名单键）

- 弹窗有 UI：33 键；
- 仅启动页面板有 UI：6 键（theme_color、wallpaper、show_lunar、show_quote、show_weather、weather_city）；
- **完全无 UI 的孤儿键：3 个**——`tab_hibernate_count`（休眠数量阈值）、`api_readonly`（API 只读模式）、`api_agent_actions`（API 页面交互闸门）；
- `privacy_shield` 旧布尔兼容键无需 UI（合理）。

### 1.5 用户与功能模块（目标用户推导依据）

产品定位（[README](file:///home/gao/shift-browser-tauri/README.md)、[strategy-2026.md](file:///home/gao/shift-browser-tauri/docs/strategy-2026.md)）：本地优先、AI 原生、可自托管、Linux/WebKitGTK、单人维护、中文首发。可辨识四类用户：

| 用户 | 核心诉求 | 对应功能模块 |
|---|---|---|
| U1 日常浏览者 | 默认浏览器/主页/搜索/主题/翻译，开箱即用 | 通用基础项、外观、翻译 |
| U2 隐私敏感者 | 反指纹、GPC、广告/网络拦截、数据清理、UA、证书、E2E 同步、锁屏 | 隐私防护、数据安全 |
| U3 AI 工作流用户 | 模型配置、MCP 工具、Agent、定时任务、WebMCP | AI、MCP、自动化 |
| U4 开发者/自托管者 | 开放 API、MCP Server、用户脚本、网络规则、自检、兼容开关 | 开放 API、扩展、自检 |

---

## 二、问题诊断

1. **分类错位（最高优先级问题）**
   - 「通用」是 4 域大杂烩：基础项 + 标签 + 自动化 + 翻译；
   - 「外观」混入 DMA-BUF 渲染故障排查开关（与视觉无关）；
   - 「数据安全」混入数据管理：记录历史/清除数据本质是隐私操作，UA 本质是身份伪装（反指纹同族），证书是传输安全；
   - WebMCP polyfill 是「让 AI 感知网页工具」的 AI 能力，却放在「开放 API」；
   - 3 个定时任务（清历史/刷订阅/跑 Agent）是自动化，散在「通用」；
   - 战略上同属「平台化」的四种注入/拦截机制（扩展 JS/CSS、用户脚本、广告拦截、隐私网络规则）分散在 3 个栏目。

2. **栏目不对应用户分层**：MCP、开放 API、自检 3 个纯高级/开发者栏目与外观、通用等高频栏目**等权平铺在一级导航**，新手认知负担重；8/10 栏目普通用户不会点开。

3. **设置入口分裂**：弹窗 + 启动页面板两套设置 UI；主题色/壁纸/天气等在弹窗不可达；字号两处可改但走不同保存通道。

4. **导航效率低**：10 项无分组标签、无搜索框、无子层级（卡片分组不在导航中反映）；深链机制 `initialSection` 只支持一级 id（[SettingsDialog.vue#L27-L34](file:///home/gao/shift-browser-tauri/src/components/SettingsDialog.vue#L27-L34)）。

5. **孤儿设置项 3 个**：后端白名单与文档承诺存在，用户界面永远无法触及（只读模式/页面交互是安全闸门，无 UI 等于功能不可用）。

6. **命名与概念碎片**：「MCP 工具」与「开放 API」图标同为 plug；「扩展」与「用户脚本」是两套数据模型两套 UI（extensions 表 / userscripts 表），用户无法形成统一「扩展生态」心智；「隐私防护」与「数据安全」边界含糊。

7. **与战略 IA 脱节**：战略五支柱（AI 原生 / 隐私主权 / 交互形态 / 平台化 / 工程基座）在栏目结构中无映射，AI+MCP+WebMCP+Agent 同属 A 支柱却三处分列，隐私+数据+证书+UA 同属 B 支柱却两栏并混。

---

## 三、新栏目方案

### 3.1 设计原则

1. **按用户任务域（心智模型）分类，不按技术模块分类**；
2. **一级栏目 10 → 8，侧栏加 4 个分组标签**：高频在上、开发者向全部收纳进「高级」；
3. **设置入口单一化**：所有 settings 键在设置弹窗可达（启动页面板保留为账号偏好镜像，不再是唯一入口）；
4. **零行为变更**：仅搬运控件 + 为既有键补 UI，绑定的 settings 键与 invoke 命令一律不变；
5. **可渐进、可回退**：分三批落地，每批结束都可发布；纯前端改动，revert 即恢复；
6. **危险/试验项视觉隔离**：统一进入「高级」或保留「危险/试验中」徽标。

### 3.2 新栏目结构（层级 · 定位 · 内容组织 · 目标用户）

```
设置
├─ 【基础】
│  ├─ 常规        基础浏览 + 标签 + 语言翻译 …………… U1 全员
│  └─ 外观        主题/主题色/书签栏 + 启动页个性化 …… U1 全员
├─ 【安全与主权】
│  ├─ 隐私与安全  防护/追踪网络/身份/历史清理/审计 …… U2
│  └─ 数据与同步  密码库/锁屏/E2E 同步 …………………… U2
├─ 【智能与平台】
│  ├─ AI 助手     模型服务/MCP/WebMCP 实验 …………… U3
│  └─ 扩展与自动化 扩展/用户脚本/定时任务 ……………… U3+U4
└─ 【系统】
   ├─ 高级        开放 API/兼容性/系统自检 …………… U4
   └─ 关于        版本/能力/承诺/链接 ………………… 全员
```

#### 1) 常规（general）——「每天都在用的浏览器基础项」

- **基础**卡片：默认浏览器、主页 URL、搜索引擎；
- **标签页**卡片：垂直标签栏、休眠超时（现有）、**休眠数量阈值（补 `tab_hibernate_count` UI）**；
- **语言与翻译**卡片：翻译源、目标语言、超时（从原通用平移，独立成卡）。
- 移出：3 个定时任务 → 扩展与自动化。

#### 2) 外观（appearance）——「所有视觉与个性化的唯一归宿」

- **主题外观**：深色/浅色、**主题色（新补，`theme_color`）**、显示书签栏；
- **启动页**：动态壁纸开关（现有 show_bg）、**自定义壁纸 URL（新补 `wallpaper`）**、字号（现有）、**农历 / 每日一句 / 天气 / 天气城市（新补 4 键）**；卡片注明「登录本地账号后，这些偏好随账号保存并以此为准」。
- 移出：DMA-BUF → 高级→兼容性。

#### 3) 隐私与安全（privacy）——战略支柱 B 单一归宿

- **防护总览**：防护档位、反指纹模式、加噪白名单、防护统计、防护自测（平移）；
- **追踪与网络**：GPC、广告拦截+自定义关键词、网络层拦截规则、自定义请求头、GPC 站点检测、外联审计（平移）；
- **身份与传输**（新归集）：**User-Agent 全局/站点覆盖、证书中心入口**（自数据管理迁入）；
- **历史与清理**（新归集）：记录浏览历史、立即清除历史、清除浏览数据（六域勾选）；定时清理天数保留在「扩展与自动化→定时任务」，本卡放一行关联说明，避免双份控件。

#### 4) 数据与同步（data）——「数据主权底线」

- **密码库**：主密码设置/解锁/移除、空闲自动锁定分钟数（平移）；
- **启动锁屏**（平移）；
- **E2E 加密同步**：WebDAV 配置/推送/拉取/清除（平移）。
- 移出：数据管理整块（历史/清理/UA/证书）→ 隐私与安全。

#### 5) AI 助手（ai）——战略支柱 A 单一归宿

栏内二级锚点（子 tab 或卡片锚），避免页面过长：
- **模型服务**：Provider、模型、API 地址、Key（现 AiSection 平移）；
- **MCP 工具**：server 管理、可用工具、手动调用、审计日志（**原一级栏目整体并入**）；
- **试验特性**：**WebMCP polyfill（自开放 API 迁入）**。
- 定时 Agent 放「扩展与自动化→定时任务」，与其它自动化集中。

#### 6) 扩展与自动化（extensions）——战略支柱 D

- **扩展**：本地 JS/CSS 扩展加载/启停删（平移）；
- **用户脚本**：新增入口卡片 → 打开现有 [UserscriptsDialog](file:///home/gao/shift-browser-tauri/src/components/UserscriptsDialog.vue)（弹窗内零新功能，仅导航收口）；
- **定时任务**：定时清理历史、定时刷新订阅、定时 Agent + 提示词（自通用迁入，集中一处）。
- 远期（不在本次范围）：扩展与用户脚本的数据模型与 UI 合并为统一扩展中心。

#### 7) 高级（advanced）——开发者/排障向，默认折叠感

- **开放 API**：eval、**只读模式（补 `api_readonly`）**、**页面交互闸门（补 `api_agent_actions`）**、端口、绑局域网、IP 白名单、模型网关、状态面板与 token 轮换（平移 + 补 2 开关）；
- **兼容性**：DMA-BUF 渲染开关（自外观迁入，保留「改后需重启」说明）；
- **系统自检**：现自检栏目整体并入。

#### 8) 关于（about）——内容不变。

### 3.3 旧栏目 → 新栏目映射（含深链兼容）

| 旧 id | 新 id（锚点） | 处理 |
|---|---|---|
| general | general | 拆出定时任务；补休眠数量 |
| appearance | appearance | 补启动页 6 项；DMA-BUF 移出 |
| ai | ai#model | 平移 |
| mcp | ai#mcp | 一级 → AI 栏内锚点 |
| extensions | extensions | 并入用户脚本入口 + 定时任务 |
| privacy | privacy#shield | 并入数据管理 4 块 |
| data | data（vault/sync/lock）+ privacy（其余） | 拆两栏 |
| api | advanced#api | 补 2 开关；WebMCP 移出 |
| selfcheck | advanced#selfcheck | 一级 → 高级内锚点 |
| about | about | 不变 |

外部跳转（状态栏隐私战绩传 `privacy`）经映射表仍落到新隐私栏；映射在 SettingsDialog 内做常量常量化，旧 id 全兼容。

---

## 四、实施步骤（依赖顺序，三批）

### 批 1：IA 骨架重组（纯搬运，零功能增减）

1. [SettingsDialog.vue](file:///home/gao/shift-browser-tauri/src/components/SettingsDialog.vue)：`SECTIONS` 改为带 `group` 的 8 项结构（分组标签：基础 / 安全与主权 / 智能与平台 / 系统）；右侧改按栏目组合多个卡片组件；加入旧→新 id 映射。
2. 拆分 [DataSection.vue](file:///home/gao/shift-browser-tauri/src/components/settings/DataSection.vue)：
   - `DataVaultSection.vue`（主密码/同步/锁屏，归属 data）；
   - `PrivacyDataCard.vue`（记录历史/清历史/清除浏览数据/UA/证书入口，并入 privacy 栏）。
3. 从 [GeneralSection.vue](file:///home/gao/shift-browser-tauri/src/components/settings/GeneralSection.vue) 抽出定时任务三块为 `AutomationCard.vue`（归属 extensions 栏）。
4. 从 [AppearanceSection.vue](file:///home/gao/shift-browser-tauri/src/components/settings/AppearanceSection.vue) 抽出 DMA-BUF 块为 `CompatCard.vue`（归属 advanced）。
5. 从 [ApiSection.vue](file:///home/gao/shift-browser-tauri/src/components/settings/ApiSection.vue) 抽出 WebMCP 块为 `WebmcpCard.vue`（归属 ai 栏）；AiSection 更名/复用为 `AiModelCard.vue`。
6. 新建容器组合：privacy 栏 = PrivacySection 现内容 + PrivacyDataCard；ai 栏 = AiModelCard + 现 [McpSection.vue](file:///home/gao/shift-browser-tauri/src/components/settings/McpSection.vue) + WebmcpCard（栏内锚点/tab）；extensions 栏 = ExtensionsSection + 新建 `UserscriptsEntryCard.vue`（emit 打开 UserscriptsDialog，需 App.vue 传一个打开事件，复用 openPanel('userscripts')）+ AutomationCard；advanced 栏 = ApiSection（剩余）+ CompatCard + SelfCheckSection。
7. App.vue 为 SettingsDialog 增加 `open-userscripts` 事件透传（或经现有 openPanel 机制）。
- 验收：旧弹窗内全部 33 个有 UI 设置项在新位置可见、可改、保存行为不变；逐栏手工冒烟。

### 批 2：补缺口（为既有键补 UI，零后端改动）

8. AppearanceSection 启动页卡：新增 theme_color（取色器）、wallpaper（URL 输入）、show_lunar/show_quote/show_weather（开关）、weather_city（输入），全部走现有 `useSettingsSet.onSet` 与 settingsStore（白名单已含，set_setting 直接可用）；与现有 show_bg/font_size 同卡。
9. GeneralSection 标签卡：补 `tab_hibernate_count` 数字输入（休眠数量阈值）。
10. ApiSection（advanced 内）：补 `api_readonly`、`api_agent_actions` 两个危险开关（含现有文案风格的风险说明）。
- 验收：弹窗设置项覆盖率 33/42 → 41/42（仅旧兼容键无 UI）；新控件改动即时落库并被对应消费方读取（启动页重开生效、API 重启生效等既有语义）。

### 批 3：导航精修（可裁剪）

11. 设置弹窗顶部加**设置搜索框**：前端静态注册表（label + 所属栏目锚点 + 别名关键词），过滤后点击跳栏并滚动定位；注册表与 SECTIONS 同文件维护。
12. 锚点深链：`initialSection` 支持 `privacy#identity` 形态；状态栏跳转随之细化。
13. 每张卡片评估加「恢复默认」（仅对有明确默认值的键；可后续迭代）。

### 明确不做（防 scope 蔓延）

- 不改 newtab/index.html 的账号偏好面板（账号级通道保留；弹窗外观卡注明优先级）；
- 不合并 extensions/userscripts 数据模型；
- 不处理 `ai_provider`/`privacy_fp_mode` 走专用命令而非 settings 白名单的历史差异（另立小任务核实）；
- 不动命令栏 17 条命令（批 3 可选让命令栏索引设置项，默认不做）。

---

## 五、依赖与注意事项

- 所有新控件绑定的 settings 键均已在 [contracts.rs](file:///home/gao/shift-browser-tauri/src-tauri/src/contracts.rs) 白名单（theme_color/wallpaper/show_lunar/show_quote/show_weather/weather_city/tab_hibernate_count/api_readonly/api_agent_actions），**无需后端改动、无 TS 契约重新生成需求**；
- 账号偏好双轨：未登录时外观卡直接写 settings 表；登录账号在启动页保存偏好会经 `apply_prefs_to_settings` 覆盖同键——卡片文案需明示，数据行为与现状一致，不引入新冲突；
- 弹窗 `v-if` 懒挂载语义保留（切到栏目才挂载取数）；ai 栏同屏多组件会并行拉取 MCP/模型列表，均为既有轻量命令，可接受；实现时 MCP 等重块仍按「栏内锚点激活才挂载」；
- UserscriptsEntryCard 打开独立弹窗时遵循现有 webview 显隐惯例（setWebviewVisible 由 UserscriptsDialog 自管）；
- 4 空格 / 不 rustfmt 单文件等 Rust 惯例本任务基本不涉及 Rust；前端遵循现有 ESLint + Prettier；
- 遵循项目惯例「新增功能必须添加防漂移测试」：本任务为 UI 重组，批 2 新增控件在现有 settings 组件测试模式下补最小渲染/交互测试（vitest）。

## 六、验证

- 前端：冷缓存 `vue-tsc` + ESLint + vitest 全绿 + `npm run build`（记忆教训：拆分类改动必须跑冷缓存 tsc/build）；
- 后端：`src/types/generated/` 零漂移（无契约改动）；保险起见 `cargo test --lib ts_export` 与全量 Rust 测试（当前基线 215）跑一次；
- 冒烟：`scripts/smoke.sh --no-restart`；手工逐栏验证 8 栏渲染、外部深链（隐私战绩）、用户脚本入口、新 8 个控件保存生效；
- 截图回归：深色/浅色两主题下设置弹窗 8 栏各一张，确认无长卡溢出/样式回归（沿用 tokens.css 设计变量）。

## 七、风险与对策

| 风险 | 对策 |
|---|---|
| 栏目内卡片过多导致长滚动（AI、隐私） | 栏内二级锚点/子 tab（AI 三页签：模型/MCP/试验）；隐私维持卡片分组+锚点 |
| 组件拆分过程中样式/交互回归 | 批 1 只搬运不改模板内部结构；每批独立验证、可单独发布/回退 |
| 旧深链 id 失效 | SettingsDialog 内置旧→新 id 映射表，外部调用点仅状态栏一处，一并适配 |
| 启动页账号偏好与弹窗外观卡互相覆盖的用户困惑 | 外观卡注明账号优先级；不改变任何既有写入路径 |
| 搜索注册表与实际栏目漂移 | 搜索注册表与 SECTIONS/卡片定义同文件集中维护，防漂移测试断言「每个注册项的目标锚点存在」 |

## 八、预期效果评估标准

**信息架构指标（前后对比）**

| 指标 | 现状 | 目标 |
|---|---|---|
| 一级栏目数 | 10（无分组） | 8（4 分组标签） |
| 一级导航中开发者向栏目 | 3（MCP/API/自检） | 0（全部并入 AI 栏或高级） |
| 设置弹窗设置项覆盖率 | 33/42（79%） | 41/42（98%） |
| 无 UI 孤儿键 | 3 | 0 |
| 设置入口套数 | 2（弹窗 + 启动页面板，后者 6 项弹窗不可达） | 1 套主入口（弹窗全覆盖），启动页面板降级为账号镜像 |
| 栏目与其内容域一致率 | 6/10 栏目混域 | 8/8 单一主题域 |

**典型任务路径（点击次数，含定位滚动，5 名技术同事走查取中位数）**

| 任务 | 现状 | 目标 |
|---|---|---|
| 改主题色 | 设置弹窗不可达（需启动页登录用户中心） | 外观 ≤2 击 |
| 配置 WebDAV 同步 | 设置→数据安全→滚到第 2 卡 | 数据与同步→第 2 卡 |
| 关闭广告拦截并加关键词 | 隐私→网络与追踪→滚动 | 隐私与安全#追踪 ≤2 击 |
| 开启 API 只读模式 | **不可达（无 UI）** | 高级#api ≤3 击 |
| 查证书/改 UA | 数据安全→滚动到底 | 隐私与安全#身份 ≤2 击 |
| 配置 MCP server | 一级 MCP | AI#mcp ≤2 击（路径深度不变，分类正确度提升） |

**质量门禁（零回归红线）**

- vitest / Rust 215 测试 / vue-tsc / eslint / build / smoke 全绿；generated 零漂移；
- 行为零变化：所有 invoke 命令与 settings 键名一一对应旧实现，无新增/删除命令；
- 可回退：三批均为前端文件改动，git revert 单批即可恢复。

**主观评估（批 3 完成后问卷，1–5 分）**

- 「我能猜到每个功能在哪个栏目」≥4；
- 「高级功能没有干扰我的日常设置」≥4；
- U2/U3/U4 三类用户各自在对应分组完成 3 个代表任务的成功率 100%。

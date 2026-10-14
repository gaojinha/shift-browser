# R4-6 可访问性审计报告（主窗口 UI）

> 审计日期：2026-10-14
> 范围：主窗口 UI（标签栏/地址栏/设置/命令面板/AI 侧栏/状态栏等 45 个组件）
> 方法：人工代码走查（键盘导航 + aria 属性）+ axe-core 全量规则跑分
> 约束：本包只审计出计划，**不改代码**；修复留后续小包

---

## 一、工具跑分结果（axe-core）

跑法：`npx vite --port 5199` 起 dev server，用系统 `google-chrome` headless + `@axe-core/playwright` 全量扫描，
规则集 `wcag2a/wcag2aa/wcag21a/wcag21aa/best-practice`。原始结果：
`docs/a11y-axe-results.json`（主页）与 `docs/a11y-axe-dialogs.json`（三个对话框场景）。
脚本着地留存：`scripts/axe-audit.mjs`、`scripts/axe-dialogs.mjs`（后续小包修完可直接复跑验分）。

### 命令面板（serious + critical 级，最重）

| 级别 | 规则 | 位置 | 说明 |
|---|---|---|---|
| critical | aria-required-attr | `src/components/CommandPalette.vue:188` | Reka `ComboboxInput` 渲染出的 input 上 `aria-controls=""` 为空字符串（空串≠省略，axe 判缺必填属性），`aria-autocomplete="listbox"` 指了个空目标 |
| serious | aria-input-field-name | `src/components/CommandPalette.vue:199` | 面板 listbox（`role="listbox"` 的 `.cp-list-wrap`）没有可访问名称，读屏无法说出"这是命令面板的下拉候选" |

### 三个场景共有的 moderate 违规（15~19 节点/场景）

| 级别 | 规则 | 位置 | 说明 |
|---|---|---|---|
| moderate | landmark-one-main | 文档级（`index.html`） | 整页没有 `<main>` 地标。浏览器 UI 天然特殊，但 axe 判文档级违规；最低成本修法：`src/App.vue` 根容器加 `role="main"` 或把内容区包进 `<main>` |
| moderate | page-has-heading-one | 文档级 | 无 `<h1>`。同上，`role="main"` 之外，至少给主区域一个视觉隐藏的 `h1`（如"标签页"） |
| moderate | region | 各场景 12~17 节点 | 页面内容不在 landmark 内：`.address-container`（AddressBar.vue:277）、`.status-left`/`.net-status`（StatusBar.vue:217/230）、toast 容器（AppToast.vue:9）等全部裸 div。给这些区域加 landmark role（`toolbar`/`status`/`region`）即消 |

对话框场景与主页违规一致（设置弹窗、地址栏建议场景未引入新规则，只有 region 节点数变化）。

---

## 二、键盘导航审计（人工走查）

### 2.1 标签栏（TabBar.vue）— serious

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-1 | 标签项是 `div.tab`，无 `tabindex`/`role`，键盘到不了标签本身 | `TabBar.vue:462-485, 506-536` | 切标签只能靠 Ctrl+Tab/Ctrl+Shift+Tab 全局快捷键；标签条带自身对键盘用户不可达，焦点直接跳过整条 tab-scroll |
| K-2 | 标签关闭按钮平时 `display:none`，焦点序断裂 | `TabBar.vue:691-713`（CSS）+ `:484`（模板） | `.close-btn` 默认 `display:none`，hover/active 才 `display:block`——非 hover 态（键盘用户）根本 tab 不到关闭按钮；且 `display:none` 对读屏不可达 |
| K-3 | 右键菜单全是 `div.menu-item`，键盘打不开、不能选 | `TabBar.vue:560-586` | 菜单只有右键/点击入口，无 `tabindex`、无 `role="menu"`/`menuitem`、无 arrow-key 导航；键盘用户完全拿不到"固定/静音/分屏/关闭其他"这些操作 |
| K-4 | 窗口控制按钮（最小化/最大化/关闭）无 `aria-label` | `TabBar.vue:545-547` | 三个 `button.wc-btn` 只有 `title`（`title` 不算可靠可访问名），读屏读不出"最小化" |
| K-5 | 分组折叠按钮无 `aria-expanded` | `TabBar.vue:497-503` | `group-toggle` 切换折叠态但无 `aria-expanded`，读屏不知道当前是展开还是折叠 |

### 2.2 垂直标签栏（VerticalTabBar.vue）— serious

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-6 | 标签项 `div.vtab-item` 无 `tabindex`/`role` | `VerticalTabBar.vue:81-97` | 同 K-1，键盘到不了标签；关闭按钮 `vtab-close` 内是图标，无 `aria-label`（`:94`） |

### 2.3 地址栏（AddressBar.vue）— moderate

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-7 | 建议下拉无 `role="listbox"`/`option`，读屏不可达 | `AddressBar.vue:294-308` | `.suggest-dropdown` 与 `.suggest-item` 全是裸 div；键盘上下键能选（JS 已实现），但读屏不知道这是个可选项列表 |
| K-8 | 建议项无 `tabindex`，键盘选中后 Enter 可打开但读屏选不动 | `AddressBar.vue:295-301` | 选中态 `selectedIndex` 纯 JS 维护，无 ARIA 同步 |
| K-9 | 锁图标（证书中心入口）是 `span`，键盘到不了 | `AddressBar.vue:278` | `lock-icon` 是 `span` 带 `@click`，无 `tabindex`/`role`；键盘用户打不开证书中心（需先找到其他入口） |
| K-10 | 地址栏导航按钮（后退/前进/刷新/主页）无 `aria-label` | `AddressBar.vue:258-276` | 四个 `nav-btn` 只有 `title`，读屏读不出"后退"等；`title` 在部分读屏不可靠 |

### 2.4 命令面板（CommandPalette.vue）— serious

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-11 | 无焦点陷阱，Esc 能关但 Tab 可逃出面板 | `CommandPalette.vue:183` | 面板是普通 overlay div，无 `aria-modal`、无焦点圈定；Reka `ComboboxRoot` 本身不管模态焦点。键盘 Tab 会跳到面板外（被遮罩挡住交互但焦点已出），读屏仍"在"原页面 |
| K-12 | 候选项列表用 Reka `ComboboxContent`，`tabindex=-1` | `CommandPalette.vue:199` | 列表 `tabindex="-1"`（axe 同时报 `aria-input-field-name`），键盘只能在输入框内用上下键选，tab 进不了列表 |

### 2.5 设置弹窗（SettingsDialog.vue + UiDialog.vue）— moderate

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-13 | 设置搜索框无 `aria-label` | `SettingsDialog.vue:127` | `input` 只有 `placeholder="搜索设置…"`，placeholder 不算可靠可访问名 |
| K-14 | 左侧导航项 `div.nav-item` 无 `tabindex`/`role` | `SettingsDialog.vue:145-155` | 8 个栏目切换是 div 点击，键盘到不了栏目本身（Ctrl+K 能绕开但读屏选栏目不可达） |
| K-15 | 搜索结果项 `button.search-item` 无 `aria-label` | `SettingsDialog.vue:131-139` | 按钮内是两行文本（label + section），按钮可聚焦但可访问名是拼接文本，无独立 `aria-label` 指明"跳到 XX 设置" |
| K-16 | AiSection 子页签 `button.ai-tab` 无 `aria-selected`/`tablist` | `settings/AiSection.vue:125-134` | 三个子页签（模型/MCP/试验）用 button 切换，但无 `role="tablist"`/`tab`/`aria-selected`，读屏不知道是 tab 且当前选哪个 |
| K-17 | 设置弹窗焦点陷阱依赖 Reka `DialogContent`（UiDialog.vue），但未验证关闭后焦点归位 | `UiDialog.vue:41-68` | Reka 自带焦点陷阱（优点），但设置弹窗是 860px 宽表单，关闭后焦点是否回触发源需人工验证；属 minor 验证项 |

### 2.6 全局快捷键与焦点管理（App.vue）— moderate

| # | 问题 | 位置 | 说明 |
|---|---|---|---|
| K-18 | 全局 keydown 只拦 Ctrl 组合，无 Ctrl 的裸键（如 Esc）在 GTK 页面持焦时由 Rust 转发 | `App.vue:341-411` | 双通道（前端 keydown + GTK global-shortcut）设计合理，但 `isDialogGuardedAction` 守卫只覆盖 Ctrl 路径（`App.vue:217-218`），Esc 路径（`App.vue:414+`）逐层关闭时无"已锁定"保护，AI 侧栏开着按 Esc 会关 AI 而非关面板 |
| K-19 | 焦点管理无 `:focus-visible` 替代（全局 `outline:none` 被吃掉） | 多文件（见 2.7） | 各组件 `outline:none` 后多配了 `box-shadow` 焦点环（settings-shared.css:218/267/338 等），但部分控件（TabBar 标签、SettingsDialog nav-item、AddressBar suggest-item）无焦点环，键盘焦点不可见 |

### 2.7 焦点环覆盖不全（minor）

`outline:none` 出现 30+ 处。已配 `box-shadow` 焦点环的：`settings-shared.css`（seg-btn/mini-btn/theme-card/ai-tab）、`components.css`（mini-btn/confirm-btn/add-btn）、`UiSwitch.vue:39-41`、`CommandPalette.vue:317-321`。
**无焦点环的**：
- `TabBar.vue` `.tab`/`.close-btn`/`.new-tab-btn`/`.wc-btn`/`.group-toggle`（全组）
- `VerticalTabBar.vue` `.vtab-btn`/`.vtab-win`/`.vtab-item`/`.vtab-close`
- `AddressBar.vue` `.nav-btn`/`.action-btn`/`.suggest-item`/`.lock-icon`
- `SettingsDialog.vue` `.nav-item`/`.search-item`/`.settings-search input`
- `StatusBar.vue` `.privacy-badge`/`.net-status`/`.status-btn`
- `AppToast.vue` `.toast-item`（toast 本不需焦点，可忽略）
- `BookmarksBar.vue`/`AISidebar.vue`/`SessionDialog.vue`/`BookmarksDialog.vue`/`TranslatePanel.vue`/`UserscriptsDialog.vue`/`HistoryDialog.vue`/`SpaceSwitcher.vue` 的 input/textarea（部分配了 `:focus` border-color，部分全裸）

键盘用户焦点不可见 = 焦点"跳来跳去但看不到"，serious 级。

---

## 三、aria 属性覆盖审计（人工走查）

### 3.1 纯图标按钮缺 `aria-label`（serious）

全项目 `button` 共 200+ 处。已带 `aria-label` 的约 60 处（多为 `title` 兜底）。**纯图标/emoji 按钮只有 `title` 无 `aria-label` 的**：

| 位置 | 控件 | 缺 |
|---|---|---|
| `TabBar.vue:545-547` | 窗口控制（─/□/✕） | `aria-label`（只有 `title`） |
| `TabBar.vue:540-541` | 新建标签 / 新建无痕 | `aria-label`（`+`/`🕶` 无文本） |
| `TabBar.vue:497-503` | 分组折叠 ▶/▼ | `aria-label` + `aria-expanded` |
| `AddressBar.vue:258-343` | 导航/动作按钮 13 个（arrowLeft/arrowRight/rotateCw/house/bookOpen/globe/sparkles/key/folder/scroll/clock/download/gear） | `aria-label`（只有 `title`） |
| `AddressBar.vue:278` | 锁图标（证书入口） | `role="button"` + `tabindex` + `aria-label` |
| `VerticalTabBar.vue:58-75` | 新建/切横向/窗口控制 5 个图标按钮 | `aria-label` |
| `StatusBar.vue:225-235` | 隐私徽章/下载/设置/帮助 4 个 emoji 按钮 | `aria-label`（dl-btn 有 `dl-badge` 角标数，但读屏读不出"下载管理（进行中 3）"） |
| `AISidebar.vue:233` | AI 关闭 ✕ | 已有 `aria-label="关闭"`（合规） |
| `AISidebar.vue:238-247` | 技能按钮 5 个（emoji+文本） | 有文本，无需 `aria-label`（合规） |
| `SpaceSwitcher.vue` | 空间切换按钮 | 需复核 `aria-label` |

### 3.2 角色缺失（serious）

| 位置 | 控件 | 应有 role |
|---|---|---|
| `TabBar.vue:560-586` | 右键菜单 + 菜单项 | `role="menu"` / `role="menuitem"` |
| `AddressBar.vue:294-308` | 建议下拉 + 项 | `role="listbox"` / `role="option"` + `aria-selected` |
| `CommandPalette.vue:183` | 面板 overlay | `role="dialog"` + `aria-modal="true"` |
| `SettingsDialog.vue:145-155` | 左侧栏目导航 | `role="navigation"` 或 `role="tablist"` |
| `settings/AiSection.vue:124-134` | AI 子页签 | `role="tablist"` / `role="tab"` + `aria-selected` |
| `AISidebar.vue:279-298` | 对话区 `.ai-list` | `role="log"`（读屏实时播报） |

### 3.3 状态类控件无 `aria-live`/`role="status"`（moderate）

| 位置 | 控件 | 缺 |
|---|---|---|
| `AppToast.vue:9-18` | toast 容器 | `role="status"` + `aria-live="polite"`（error 用 `assertive`）——读屏完全听不到 toast |
| `StatusBar.vue:218` | 加载失败提示 | `role="alert"`（临时出现 5s，读屏需 `assertive`） |
| `AddressBar.vue:318` | 阅读模式提示 `.reader-tip` | `role="status"` |
| `AISidebar.vue:294-295` | "思考中…" | `role="status"` + `aria-live` |
| `StatusBar.vue:117` | 下载角标 `.dl-badge` | `aria-label` 含数字（如"下载管理（3 进行中）"） |

### 3.4 label 关联（moderate）

grep 显示 30+ 处 `<label>` 用了 `for=`，但多数表单字段（settings 各 Section、AISidebar 输入框、TranslatePanel textarea）`<label>` 未用 `for=` 包裹/关联 input，读屏读不出字段名。集中在 `settings/*.vue` 与 `TranslatePanel.vue`、`UserscriptsDialog.vue`。

---

## 四、分级清单（按 WCAG 影响定级）

**critical（阻塞读屏/键盘主流程，1 项）**
- C-1 CommandPalette.vue:188 `aria-controls=""` 空串（axe critical）——Reka 渲染 bug，键盘 + 读屏都受影响

**serious（主流程键盘/读屏重大障碍，7 项）**
- S-1 TabBar.vue 标签无键盘可达（K-1）
- S-2 TabBar.vue 右键菜单键盘不可达（K-3）
- S-3 VerticalTabBar.vue 标签无键盘可达（K-6）
- S-4 CommandPalette.vue 无焦点陷阱（K-11）+ listbox 无名称（axe serious）
- S-5 纯图标按钮大面积缺 `aria-label`（§3.1，约 30 处）
- S-6 角色缺失（§3.2，6 处）
- S-7 焦点环覆盖不全导致键盘焦点不可见（§2.7，约 8 个组件组）

**moderate（可访问但效率/完整度受损，8 项）**
- M-1 地址栏建议下拉无 listbox/option role（K-7/K-8）
- M-2 地址栏导航/锁图标键盘不可达（K-9/K-10）
- M-3 设置弹窗搜索框无 label、栏目导航无键盘可达（K-13/K-14/K-15）
- M-4 AiSection 子页签无 tab role（K-16）
- M-5 状态类无 aria-live（§3.3，5 处）
- M-6 文档级 landmark/h1 缺失（axe moderate）
- M-7 表单 label 未关联 input（§3.4）
- M-8 TabBar 标签关闭按钮 focus 断裂（K-2）

**minor（优化项，3 项）**
- m-1 分组折叠无 `aria-expanded`（K-5）
- m-2 状态栏下载角标读屏不含数字（§3.3）
- m-3 设置弹窗关闭后焦点归位未验证（K-17）

**合计：critical 1 / serious 7 / moderate 8 / minor 3 = 19 个可操作项**

---

## 五、分步修复计划（拆成后续小包建议）

按"先解锁键盘主流程、再补读屏语义、最后扫尾"排序，每包改完可复跑 `scripts/axe-*.mjs` 验分 + 关口全绿：

**包 1（critical + serious-4）：命令面板 a11y 收尾**
- 修 C-1：`CommandPalette.vue` Reka `ComboboxInput` 的 `aria-controls` 空串（升级 Reka 或手动补 `aria-controls` 指向列表 id）
- 修 S-4：面板加 `role="dialog"` + `aria-modal="true"` + 焦点陷阱（Reka `DialogContent` 模式或自实现 Tab 圈定 + 关闭焦点归位）；listbox 补 `aria-label="命令候选"`
- 预计：1 文件，改完跑 `axe-dialogs.mjs` 验证 critical/serious 清零
- 风险：Reka 版本升级可能影响其他 Reka 组件（UiDialog/UiSwitch），需全量回归

**包 2（serious-1/2/3/8）：标签栏键盘可达**
- `TabBar.vue` + `VerticalTabBar.vue`：标签 `div` → 加 `tabindex="0"` + `role="tab"`，`tabbar` 容器 `role="tablist"`；标签间 ArrowLeft/Right 切换、Enter 选中、Ctrl+W 关闭
- 关闭按钮常态可见或 `:focus-within` 显示（修 K-2 焦点断裂）
- 右键菜单改 `role="menu"` + 键盘打开（右键后 `tabindex` 进菜单、上下键选、Esc 关）
- 窗口控制/新建标签补 `aria-label`
- 预计：2 文件，CSS 改动为主 + 键盘事件绑定；回归标签拖拽不破坏

**包 3（serious-5/6）：图标按钮 + 角色补齐**
- 全项目纯图标 `button` 补 `aria-label`（§3.1 约 30 处，机械活）
- 补角色：TabBar menu / AddressBar listbox / SettingsDialog tablist / AiSection tablist / AISidebar log（§3.2）
- 预计：约 10 文件，纯属性加挂，低风险

**包 4（moderate）：焦点环 + 状态播报**
- 全项目 `outline:none` 处统一补 `:focus-visible` 焦点环（`box-shadow: 0 0 0 3px var(--accent-ring)`，§2.7 8 组）
- 状态类补 `aria-live`（toast/status/reader-tip/思考中，§3.3）
- 文档级加 `role="main"` + 视觉隐藏 `h1`（消 axe moderate landmark/heading）
- 表单 `label for=` 关联（§3.4）
- 预计：约 15 文件，CSS 主体 + 少量属性；`prefers-reduced-motion` 下焦点环需保留

**包 5（minor + 验证）**
- 分组 `aria-expanded`、下载角标读屏、设置弹窗焦点归位验证
- 全量复跑 `axe-audit.mjs` + `axe-dialogs.mjs`，目标 critical/serious/moderate 全清零
- 人工键盘走查一遍（标签→地址→设置→命令→AI→状态栏）

**包 6（可选，工具化）**
- 把 `scripts/axe-*.mjs` 固化进 CI（跑 dev server + 扫描 + 卡分），后续改动自动 a11y 守门
- 需补 `pa11y`/`axe` CLI 配置与 CI 步骤，依赖 CI 环境可起 chrome headless

---

## 六、未覆盖 / 需补充条件

- **GTK 网页层 webview 内的 a11y** 不在本审计范围（webview 内容属渲染层 R2，Q-5/Q-6 未闭合）；本审计只覆盖 HTML 层 UI（Vue 组件）
- **真机读屏验证**（Orca/NVDA 实操）缺条件：当前环境无屏幕阅读器，键盘走查 + axe 跑分为人工替代；建议修复包落地后找真机过一遍
- **pa11y CLI 未跑**：已用 axe-core（覆盖更全，含 best-practice）替代；pa11y 规则是 axe 子集，可视为已覆盖
- 跑分基于 dev server（`localhost:5199`）静态首帧，**动态交互态**（如标签拖拽中、AI 流式输出中、建议下拉展开中）的 axe 快照可能有遗漏，修复包需动态态复检
- 对话框场景 axe 只扫了设置/命令/地址建议 3 个，**AI 侧栏、翻译面板、历史/书签/密码/会话/下载弹窗** 未单独 axe 快照（结构上与已扫的 UiDialog 同源，违规面预期一致，修复包 3 一并补扫）

---

## 附：跑分命令（后续小包复跑用）

```bash
npx vite --port 5199 --strictPort &   # 起 dev server
node scripts/axe-audit.mjs            # 主页 + 全量规则
node scripts/axe-dialogs.mjs          # 设置/命令面板/地址建议三场景
# 结果：docs/a11y-axe-results.json、docs/a11y-axe-dialogs.json
```

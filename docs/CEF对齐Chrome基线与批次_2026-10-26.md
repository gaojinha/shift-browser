# CEF 独立窗口对齐 Chrome 基线（2026-10-26 高哥拍板）

> 立档：2026-10-26 · 王二狗
> 方针（高哥原话）：「选 B 对齐 Chrome 的各个方向，不但是书签密码还有 UI 界面菜单功能，我们在对齐所有的项目基础上再说。」
> 含义：**以 Chrome 桌面版为基线，功能 / UI / 菜单全面对齐**；单项收口（如下载）不再是终点，做完只算补齐一格。
> 证据口径：CEF 侧全部来自 `src-tauri/src/engine/cef/` 代码原文；主应用能力面来自 `src/components/` 与 `contracts.rs` 快捷键矩阵；数字均标日期。

---

## 一、CEF 独立窗口现状（已有）

| 能力 | 现状 | 代码位置 |
|---|---|---|
| 标签栏 | 新建 / 切换 / 关闭 / 关闭其他 / 关闭右侧 / 中键关闭 / 空白双击新建 | `toolbar.rs:240-247`、`shell.rs` `handle_action`（`tabcloseothers`/`tabcloseright`） |
| 地址栏 | 输入导航、URL 同步、加载中转圈、前进/后退/刷新/停止 | `toolbar.rs:267-311`、`shell.rs` |
| 窗口 | 去系统标题栏、自绘最小化/最大化/关闭、拖动 | `toolbar.rs:249-251`、`windrag` |
| 书签栏 | 显示/收起（Ctrl+Shift+B）、右键增删改、文件夹下拉、移动到文件夹 | F-CEF-09 / F-CEF-13（`6765478`） |
| 密码面板 | 列表 / 搜索 / 复制（30s 清剪贴板）/ 删除 | F-CEF-12（未提交？以实际提交为准） |
| 快捷键 | Ctrl+T / Ctrl+W / Ctrl+Tab / Ctrl+Shift+T / Ctrl+Shift+B | `shell.rs:1691-1760` |
| 注入 | 广告拦截 / 隐私脚本 / 品牌表补全 | F-CEF-11 |
| 下载 | 仅落盘 `~/Downloads`（`bc2cb7b`）；attachment 自动转下载为 F-CEF-14 半成品，未验证 | `shell.rs:1770-1815` |

**结论：标签 + 地址栏 + 书签栏 + 密码面板四块有，Chrome 的大部分「浏览器常规面」还没有。**

---

## 二、差距清单（按 Chrome 桌面版对齐）

### P0 · 核心体验（缺了就不像浏览器）

| # | 能力 | Chrome 行为 | 现状 | 备注 |
|---|---|---|---|---|
| 1 | **⋮ 主菜单** | 新标签/新窗口/无痕、历史、下载、书签与清单、扩展、缩放、打印、查找、设置、帮助、退出 | 无 | 独立窗口必须有入口，否则所有面板都不可达 |
| 2 | **页面右键菜单** | 后退/前进/刷新/另存为/打印/复制/粘贴/全选/翻译/查看源代码/检查 | 无（`grep context_menu` 零命中） | CEF `on_context_menu` 回调 + 自绘菜单 |
| 3 | **标签右键菜单** | 重载/复制/固定/静音/关闭/关闭其他/关闭右侧/新建标签组/移动到新窗口 | 无 | 现在只有工具栏按钮 |
| 4 | **地址栏下拉建议** | 历史 + 书签 + 搜索引擎建议、键盘上下选择 | 无（`#addr` 只有 keydown 回车） | 需要 Rust 侧查询 + 工具栏浮层 |
| 5 | **书签星标** | 地址栏右侧一键收藏/编辑当前页 | 无 | 现在只能开书签菜单新建 |
| 6 | **下载完整链路** | 记录/进度/完成/暂停/取消/打开文件夹/重新下载/下载面板 | 只落盘，无记账无进度无可视入口 | 见 F-CEF-15（B 方案） |
| 7 | **缩放** | Ctrl+`+`/`-`/`0`，按站点记忆 | 无 | CEF `SetZoomLevel` |
| 8 | **页内查找** | Ctrl+F，高亮/计数/上下条 | 无 | `FindHandler` 或 JS 注入 |
| 9 | **历史面板** | Ctrl+H，搜索、按日期分组、清除 | 无 | 主应用 `HistoryDialog` 能力可迁 |
| 10 | **设置页** | 外观/隐私/搜索引擎/下载目录/默认浏览器/密码/证书 | 无 | `SettingsDialog` 能力迁 CEF |
| 11 | **站点信息** | 安全锁、证书、站点权限 | 无 | 与证书面板一起做 |
| 12 | **快捷键补齐** | `contracts.rs:380-560` 主应用 17 条矩阵（Ctrl+L/R/H/J/O/K/S/F12…） | 只有 5 条 | 单源表，CEF 侧接同一矩阵 |

### P1 · 主应用能力迁移（CEF 不能比 WebKit 版少）

| # | 能力 | 主应用现状 | CEF 现状 |
|---|---|---|---|
| 13 | 书签管理面板 | `BookmarksDialog.vue` | 只有栏 + 右键菜单，无独立管理页 |
| 14 | 用户脚本管理 | `UserscriptsDialog.vue` | 无 |
| 15 | 证书面板 | `CertificatesDialog.vue` | 无 |
| 16 | 会话管理 | `SessionDialog.vue` | 无 |
| 17 | 标签分组 / 工作区 | `GroupsDialog.vue` / `SpaceSwitcher.vue` | 无 |
| 18 | 翻译面板 | `TranslatePanel.vue` / `TranslateProgress.vue` | 无（且翻译桥缺宿主通道，见待拍板卡） |
| 19 | 隐私盾牌 | `ShieldPanel.vue` | 注入已生效，无 UI 入口 |
| 20 | 权限提示 | `PermissionPrompt.vue` | 无 |
| 21 | 状态栏 / 下载角标 | `StatusBar.vue` | 无 |
| 22 | 竖向标签栏 | `VerticalTabBar.vue` | 无（可选） |
| 23 | 命令面板 | `CommandPalette.vue`（Ctrl+K） | 无 |
| 24 | AI 侧栏 | `AISidebar.vue` | 无 |
| 25 | 导入模块 | `ImportModule.vue` | 无 |

### P2 · 高级能力（Chrome 有、可后置）

| # | 能力 | 备注 |
|---|---|---|
| 26 | 标签拖拽重排 | 现在 `mousedown` 只用于窗口拖动 |
| 27 | 标签固定 / 静音 | 需 CEF 音频状态回调 |
| 28 | 标签组 | 与 P1-17 合并考虑 |
| 29 | 无痕模式 | 独立 profile |
| 30 | 打印 Ctrl+P | CEF `print_to_pdf` 或系统对话框 |
| 31 | DevTools F12 | CEF `ShowDevTools` |
| 32 | 阅读模式 | 主应用 `engine/content.rs` 能力可迁 |
| 33 | 恢复上次会话 | 现在 CEF 无会话恢复（F-CEF-14 隔离 bug 同源） |
| 34 | 扩展管理页 | 与 F-CEF-11 注入通道配套 |

### 不做（本期明确排除）

- Chrome 账号同步、Chrome Web Store、Google 服务集成（与产品定位不符）。
- Windows/macOS CEF（维持 `docs/CEF换核规划_2026-10-25.md` 非目标）。

---

## 三、批次建议（一卡一交付，执行 copilot / 验收 atomcode）

> 编号从 **F-CEF-15** 起。注意：现有 WORKBOARD 里有两张都叫 F-CEF-14（下载、注入子 frame），先不改号，新卡避开。

| 批次 | 卡号 | 内容 | 依赖 |
|---|---|---|---|
| 1 | F-CEF-15 | ✅ **已交付 `45d5227`** | **下载完整链路（B 方案）**：设置目录 / 同名去重 / 记账（`insert_download` + 进度 + 完成/失败）/ 工具栏下载面板（`SHIFT_ACT:dl*`）/ 打开文件夹 / 重新下载 | F-CEF-14 半成品 |
| 1 | F-CEF-16 | ✅ **已交付 `9f1590c`** | **主菜单 ⋮ + 页面右键菜单**：菜单骨架先接已有能力（新标签/历史/下载/书签/密码/设置/缩放/退出），右键菜单接 CEF `on_context_menu` | 无 |
| 2 | F-CEF-17 | ✅ **已交付 `12a8989`** | 地址栏下拉建议（历史+书签+搜索）+ 书签星标 | F-CEF-16 菜单骨架 |
| 2 | F-CEF-18 | 缩放 + 页内查找 + 历史面板 | 无 |
| 3 | F-CEF-19 | 设置页 + 证书面板 + 站点信息 | 主应用面板迁移 |
| 3 | F-CEF-20 | 用户脚本 + 会话 + 分组/工作区 | 主应用面板迁移 |
| 4 | F-CEF-21 | 标签右键菜单 + 拖拽重排 + 固定/静音 | F-CEF-16 菜单组件 |
| 4 | F-CEF-22 | 无痕 + 打印 + DevTools + 阅读模式 | 视批次 1-3 结果 |

> **额外完成（未单独立卡）**：CEF 启动页跟主应用数据联动——搜索引擎读 `settings.search_engine`、快捷站点读 `settings.quick_sites`、最近访问读 `history`（`5495d78` / `15a9b84`）。

---

## 四、验收口径（每卡通用）

1. **真机 CDP**：独立临时 profile（`SHIFT_CEF_CACHE_DIR` 必须真隔离，先修 `open_shell_on_main` 不读 env 的 bug）+ 截图；DOM 断言不算数（F-CEF-13 浮层裁切教训）。
2. **行为对照 Chrome**：同一操作在 Chrome 里的表现写成 1-2 行预期，实测对齐；不一致的写明差异与取舍。
3. **关口**：`cargo clippy --all-targets` 双配置 0 警告；`cargo test --lib` 649（默认）/ 659（`--features cef-engine`）；`npm run lint`（当前有 2 个历史 lint 错，先修）；`vitest`；`npm run build`。
4. **不许回归**：每卡交付前跑一遍上一批的验收项（书签/密码/标签栏/注入）。

---

## 五、当前已知前置阻塞

1. `SHIFT_CEF_CACHE_DIR` 半隔离：`cef_cmd.rs:81` 读了 env，`cef_cmd.rs:348` 的 `open_shell_on_main` 没读，工具栏页仍落 `~/.cache/com.shiftbrowser.app/shell/` → 真机验证不可信。**批次 1 开工前必修**。
2. 同树多人并行：编译窗口独占、`git add` 整文件会吞别人改动（历史踩过两次）。
3. `npm run lint` 现有 2 个 error（`scripts/cef-coordinator/{ff09-probe,probe3}.js` 的未使用 `e`），交工关口会挂。

---

*本清单为基线，不含推测数字；CEF 侧「无」项均以 grep 零命中/代码原文为据。后续每完成一卡，在本表对应行标注提交号与日期。*

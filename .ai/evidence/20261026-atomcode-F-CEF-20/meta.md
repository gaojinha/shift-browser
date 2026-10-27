# F-CEF-20 书签管理面板 — 验收取证（atomcode，独立）

> 2026-10-27 00:22 收。验收对象：F-CEF-20 Part1（`cc44c10`）+ Part2（`214a47b`）交付态。
> 结论：**不通过（1 项范围内缺陷：JS 侧面板互斥单向缺失）**；其余全项过。

## 被测二进制身份
- worktree：`/tmp/f20-accept-214a47b` @ `214a47ba4de8ceebec8ab0e68b53e5ef52d8f83`（Part1+Part2 干净提交态，**不含** 23:41 后在途 WIP，归因干净）
- 构建：`cargo build --features cef-engine`（CARGO_TARGET_DIR 指向共树 target，23:52 完成，1m04s）
- 二进制：`src-tauri/target/debug/shift-browser`，mtime Oct 26 23:52
- sha256：`f85e73fb6a42789e26e3e517b62e6e3762b46ddbe5e0c317687f7def396dee59`

## 隔离环境
- Xvfb `:99`（1400x900x24）+ 独立 D-Bus（`dbus-daemon --session --print-address --fork`）
- HOME=`/tmp/f20-iso-home`、SHIFT_CEF_CACHE_DIR=`/tmp/cef-f20`、CDP 端口 9365
- 主窗 1280x860（win 6291459）；CEF 隐藏窗 10x10（设计如此）
- 造数（sqlite 直写隔离库，确定性 id）：夹 101 F1 / 102 F2 / 103 F1a（F1 子）；书签 201-207（root：201/202/203/207，F1：204/205，F1a：206）

## 关口（独立复跑，worktree @214a47b）
- `cargo clippy --all-targets --features cef-engine`：exit 0，warning **0**
- `cargo test --lib --features cef-engine`：**669 passed / 0 failed / 2 ignored**（130s）

## 真机 CDP 逐项结果（稳态读 = 动作完成后的独立 eval；DB 列 = sqlite 直查）
| # | 项 | 期望 | 实测 | 结果 | 证据件 |
|---|-----|------|------|------|--------|
| 1 | 初始态 | 面板关，h=122 | cls=[]，h=122 | ✅ | t1-init |
| 2 | 真实键 Ctrl+Shift+O | 面板开，h=502，树 5 节点，列表 7 | cls=[bmmanage-open]，h=502，树 全部书签*/未分类/F1/F1a/F2，rows=201..207 | ✅（真实 xdotool 键，窗口 6291459） | t2-open + t2-open.png |
| 3 | 菜单「书签管理器」开面板 | 同上 | 同上（另测 t6a-reopen） | ✅ | t6a-reopen |
| 4 | 搜索 example | 4 条（201/202/204/206） | rows=[201,202,204,206] | ✅ | fb-search-example |
| 5 | 搜索 github | 1 条（203） | rows=[203] | ✅ | t4-github + t5b-after |
| 6 | 清空搜索 | 恢复 7 条 | rows=7 | ✅ | t4b-clear |
| 7 | 打开 207 | 内容页导航 + 面板自关 | 内容 target=`file:///tmp/f20-open-check.html`（body 含 F20-OPEN-OK），cls=[]，h=122 | ✅ | t5-open / t5c-content / t5b-after |
| 8 | 删除 205-arm | 首次点击 →「确认」armed=1 | {txt:确认, armed:1} | ✅ | t7a-arm + t7a-arm.png |
| 9 | 删除 205-3s 解除 | 3.5s 后回「删除」armed='' | {txt:删除, armed:''} | ✅ | t7b-disarm |
| 10 | 删除 205-二次点击 | 行消失，DB 6 条、无 205 | rows=6（无 205）；DB count=6 has205=0 | ✅ | t7c-confirm / t7d-after |
| 11 | 编辑 204（Enter 提交） | DB title 变更 + DOM 同步 | DB「F1 内书签A改」；DOM 同 | ✅ | t8a-edit / t8b-title |
| 12 | 编辑 Esc 取消 | input 清除、DB 不写、无延迟幽灵提交 | liveEditInputs=0、row206title 原值；+0.7s 后 DB 仍原值 | ✅ | fb-esc-reprobe |
| 13 | 移动 202→F1 | select 5 项（未分类+4 夹递归）；DB folder_id=101 | "moved options=5"；DB 202folder=101 | ✅ | t10-move |
| 14 | 新建夹 F3（顶层） | DB (104,F3,NULL) + 树节点 | folders 含 (104,'F3',None)；树出现 📁 F3 | ✅ | t9-newfolder / t9b-tree + DB 直查 |
| 15 | 互斥（Rust 高度侧） | 全对归零 | bmmanage=502、hist=462、关=122（**仅实测 bmmanage↔hist 一对**；静态全表 Rust 侧缺 3 处，见「缺陷」节；**逐对真机复验待 F-CEF-21 全表**） | ⚠️ 部分覆盖 | fb-stable-502 / fb-hist-both / t5-open |
| 16 | 互斥（JS class 侧，旧→新） | 开旧面板后 cls 单面板 | **cls=[bmmanage-open, hist-open] 双开**；静态全表 JS 侧缺 17 处（见「缺陷」节；**逐对真机复验待 F-CEF-21 全表**） | ❌ **缺陷** | fb-hist-both + fb-hist-both.png |
| 17 | 互斥（JS class 侧，新→旧） | setBmManage 清 7 旧 class | 实测/源码均对（T12 cls=[bmmanage-open]） | ✅ | fb-final-502 |

## 缺陷（不通过项，2026-10-27 再补正后列全 20 处；结论不通过不变）
**面板互斥双向均不完整**（@214a47b；按二狗 09:30 复核回的静态全表列全，方法=regex 抽每个 setter 的 `classList.remove` / `*_EXTRA.store(0)` 清单对照全集、剔除"自己不移除自己"后计数，原始行逐条肉眼复核。我 09:22 版报 12 处系**偏低**，更正）：

- **① JS class 侧（17 处缺项）**：`setBmManage` 清全 7 个旧 class（新→旧 ✅）；六个旧 setter 缺项全表：

  | 旧版 setter | 没移除的（除自身） | 缺 |
  |---|---|---|
  | `setPwPanel` | menu-open, site-open, settings-open, bmmanage-open | 4 |
  | `setDlPanel` | menu-open, site-open, settings-open, bmmanage-open | 4 |
  | `setMenuPanel` | site-open, settings-open, bmmanage-open | 3 |
  | `setHistPanel` | site-open, settings-open, bmmanage-open | 3 |
  | `setSitePanel` | settings-open, bmmanage-open | 2 |
  | `setSettingsPanel` | bmmanage-open | 1 |
  | `setBmManage` | —（清全 7） | 0 ✅ |

  （`find-open` 维旧版 7 个 setter 都在清，不缺，勿误计。）
- **② Rust extra 侧（3 处）**：`set_site_panel` 清零 6/8，**缺 `SETTINGS_EXTRA` + `BM_MANAGE_EXTRA_V`（2 处）**——其中"site 不清 SETTINGS"我 09:22 版与二狗首轮都漏；可验证后果：先开设置页（122+320=442）再开站点信息（+150）→ 两 extra 叠加，h≈592；`set_settings_panel` 清零 7/8，**缺 `BM_MANAGE_EXTRA_V`（1 处）**。其余 `set_dl/set_menu/set_hist/set_pw/set_find_bar` 均清零 8/8（0 处 ✅）。
- **合计 17 + 3 = 20 处**（我 09:22 版报 10+2=12，偏低 8 处已更正）。
- 真机稳态证据仅 bmmanage→hist 一对：`cls=[bmmanage-open,hist-open]`、h=462，截图 `fb-hist-both.png`；其余对各缺项经源码核查确认，未逐一上机。
- **修复现状（2026-10-27 09:30 再补正措辞）**：copilot F-CEF-21（面板互斥矩阵收口）**已入 HEAD 但未真机复验**——Rust `enum Panel` + `clear_all_extras_except(keep)`/`set_panel_extra()`，JS `removePanelClasses(except)` 由单一注册表 `PANEL_CLASSES`（`panel_classes_json()` 注入）驱动，`12817c3` 是二狗明确标注的 **WIP，不得视为交付**。静态层面"手写清单这个物种"已被结构性堵死（8 atom + BM_EXTRA 全 extra 覆盖 / JS 8 类全清），但**静态成立 ≠ 验收通过**；逐对真机复验待 F-CEF-21 全表交付（原 8×7=56 对；certpanel 接入注册表后 9×8=72 对），届时按回执「复验方式」补 site/settings 两对走一遍。

## 日志
- 无新增形态 panic（`grep panicked` 无新命中）。
- init 期 `set_bookmarks → glib::idle_add_local_once`（`cef_cmd.rs:169` → `shell.rs:1543`）panic 复现 = **F-CEF-18 已记录的存量 bug**（tokio worker 线程调 GTK local main context），tokio 已捕获，功能无影响（本次全部书签操作的推送/刷新正常）。

## 文件清单
- 有效证据：t1-init / t2-open(+png) / t4-github / t4b-clear / t5-open / t5b-after / t5c-content / t6a-reopen / t7a-arm(+png) / t7b-disarm / t7c-confirm / t7d-after / t8a-edit / t8b-title / t10-move / t11-hist / t12-final(+png) / fb-search-example / fb-esc-reprobe / fb-stable-502 / fb-hist-both(+png 缺陷证据) / fb-final-502(+png)
- 第一轮失败记录（备查）：t3-search / t5 系 / t6a-arm~t6d / t7-edit / t8-move / t9a-esc / t9b-tree 中部分件含无效选择器（`[data-id=203]` 未加引号，CSS 属性值不能以数字开头 → `querySelector` SyntaxError）的异常输出，属验收侧测试脚本问题，**非被测代码缺陷**；有效结论以第二/三轮（s* / fb* 及上表）为准。

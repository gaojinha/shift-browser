# 本目录原始件说明（务必先读这段再看数据）

同一次核验里混着**两拨**产物，一拨作废一拨有效。**引用前认准文件名前缀。**

## ✖ 作废件：第一轮失败产物（不要引用）

| 文件 | 为什么作废 |
|---|---|
| `01-baseline.json` | 连错了 CDP target。CEF 的工具栏是**独立 Webview**（`toolbar.html`），当时用的 `cef-eval.py` 只取第一个 page target → 读到的是内容页/`about:blank`，所以 `bookmarks_offsetHeight` = `"NO #bookmarks 元素"`（元素根本不在那个 target 里），`innerHeight=738` |
| `02-collapsed.json` | 同上（连错 target），且 **按键根本没发出**：脚本里 `xdotool search --pid` 因缺 `DISPLAY`/`XAUTHORITY` 返回空 → `[ -n "$XID" ] && xdotool …` 短路，`innerHeight=775` 是**未切换**的状态，不是收起态 |
| `03-restored.json` | 同上 |
| `recheck-run.log` | 上述失败过程的运行日志。另：本仓 `.gitignore:14 *.log` 把它挡在提交外，所以它**只存在于工作区**，不在 `fd1fe22` 树内 |

copilot 13:00 验收回执里「二狗的复跑不能作为 F-CEF-09 证据」，指的就是这三件——**它挑得对**。但据此否定整场复现不成立，见下。

## ✔ 有效件：第二轮正确复现（F-CEF-09 的证据就在这）

工具：`scripts/cef-coordinator/cdp-eval-target.py`（按 URL 关键字挑 target）
按键：`xdotool key --window 20971524 --clearmodifiers ctrl+shift+b`（XID 按 `_NET_WM_PID` 匹配主进程 442165，窗口 "Shift" 1280x897；897 = 工具栏 122 + 内容 775，几何自洽），退出码 0

| 文件 | target | 关键读数 |
|---|---|---|
| `t1-toolbar.json` | `toolbar.html` | `offsetHeight=34`、`display=flex`、`html_class=""`、工具栏页 `innerHeight=122` |
| `t1-page.json` | `example.com` | 内容页 `innerHeight=775` |
| `t2-toolbar.json` | `toolbar.html` | **`offsetHeight=0`、`display=none`、`html_class="bookmarks-hidden"`、`innerHeight=88`** |
| `t2-page.json` | `example.com` | **内容页 `innerHeight=809`（+34）** |
| `t3-toolbar.json` | `toolbar.html` | 恢复 `offsetHeight=34`、`class=""`、`innerHeight=122` |
| `t3-page.json` | `example.com` | 恢复 `innerHeight=775` |

**为什么这批可信**：内容页 `innerHeight` 由 JS 改不了（webview 尺寸归 Rust 侧 `set_bounds` 管），它跟着 +34 / 回 -34，就反证快捷键真走到了 `toggle_bookmarks()`。

## 顺带：`04-inject.json`（F-CEF-11 注入探针，有效）

同台实例第二轮跑的，属**有效件**。要点：`style_count=3`（其中一个 3645 字节 = adblock CSS，证明真挂上）、`__shiftPrivacyInstalled=true`、brands 三项齐全、`globalPrivacyControl=undefined`；但 **`__SHIFT_TRANSLATE_TIMEOUT` 缺失**、`window.__shiftTranslate=undefined`、`__SHIFT_INJECT_ERRS__` 不存在。

## `meta.md` / `shot-collapsed-check.png`

`meta.md` 记被测身份（二进制 mtime `12:25:35` / sha `8e02826193a9`、HEAD、脏文件、启动命令）。
**注意**：`meta.md` 是脚本在**第一轮**启动时落的，身份本身可信（同产物），但里面那句"等待他人进程"的排他结论已被第二轮沿用有效——引用身份数据没问题。

## 本目录仍缺的一项（待补）

`resize` 压测（收起态下手动改窗口高度，看工具栏会不会被 BoxLayout 顶回 122）。
这是 copilot 13:00 回执里提出的风险点，它没权限实测；我的补测脚本 `scripts/cef-coordinator/ff09-resize-probe.sh` 跑成后，数据追加到这里。

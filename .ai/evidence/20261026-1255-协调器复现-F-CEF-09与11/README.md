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


## ✖ 第三处更正（13:20-13:32）：`04-inject.json` 里三项读数是**我探针的错**，不得当缺陷证据

原始件**不改**（留证），但引用前必须知道：

| 字段 | 原始读数 | 真相 |
|---|---|---|
| `prefix_vars.TRANSLATE_TIMEOUT` | `false` | **假阴性**。探针名写成 `__SHIFT_TRANSLATE_TIMEOUT__`（多两个尾下划线），真名无尾缀（`adblock.rs:157`）。已改 `TRANSLATE_TIMEOUT_VAL` 取实际值（本机 settings `translate_timeout=120`，合法区间 5..=120） |
| `privacy_marks.patched` | `false` | **假阴性**。`__shiftPatched` 不挂 window，是 WebRTC 防护的幂等哨兵，挂在构造函数上（`privacy.js:233 !OrigRTC.__patched` 形如 `OrigRTC.__shiftPatched`）。探针已换 `rtc_webrtc_patched` 直读 `RTCPeerConnection.__shiftPatched` |
| `privacy_marks.brands_val` | `null` | **假阴性**。`__shiftBrands` 是函数内局部变量（`privacy.js:337 var`），页面永远读不到。**品牌表其实装上了**：同件 `ua_brands` 有完整三项 `{Not_A Brand 24, Chromium 154, Google Chrome 154}` |

**仍然有效的读数**（符号确在 window 上，逐条从 `resources/js/privacy.js` 定位过行号）：`installed` ✔(4) / `noise_on` ✔(43,92) / `origin_seed` ✔(78) / `fp_mode_val="uniform"` ✔(101) / `features` ✔(417)；以及 `inject_ok`、`style_count=1`、`style_len_head=290`、`ctxMenuReg`。

**推论订正**：
- 「wl_prefix 整段没执行」——**作废**（正数 3 实测到，6/7 前缀变量为 true）。
- 「隐私补丁没打上 / 品牌表未生效」——**作废**（两项都是探针位置读错）。
- 「翻译在 CEF 下不可用」——**仍成立**，但理由换成**宿主通道缺失**（WebKit messageHandlers + Tauri 自定义协议两通道 CEF 都没有），详见 `.ai/inbox/copilot/20261026-1320-F-CEF-11更正-我误报两条.md`。
- §1.2 指纹那份「brands 待重采」**未销账**：本件只确证了**注入侧** `__SHIFT_UA_BRANDS__`，**运行时侧** `navigator.userAgentData.brands` 当时没读。探针已加 `brands_runtime`，下轮一并采。

**根因（一句话）**：探针里的字符串常量**必须 `grep` 从源码取**，还要确认它**挂在哪个对象上**（window / 构造函数 / 局部变量）。手抄名字 = 拿假钥匙开锁，倒说屋里没人。

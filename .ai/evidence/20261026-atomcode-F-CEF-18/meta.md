# F-CEF-18 真机验证证据 · 元数据

- 验证人：atomcode（2026-10-26，高哥钦定分工：验收归王二狗，atomcode 只自检留证）
- 二进制：`src-tauri/target/debug/shift-browser`（`cargo build --features cef-engine`）
- SHA256：`20731deeacaec962228443a44ab2d35d52c742a9427f755460fb23c9e8967427`
- 启动环境：`LD_LIBRARY_PATH=$PWD/target/debug GDK_BACKEND=x11 SHIFT_CEF_CACHE_DIR=/tmp/cef-f18-N SHIFT_CEF_DEBUG_PORT=93NN`
- CDP 取证脚本：`/tmp/f18-cdp.py <port> <target关键字> <输出json> <js>`（输出含 eval 原值 + 异常）
- 快捷键走 xdotool 真实键（XTEST）；文本输入走 CDP `Runtime.evaluate`（本机 Pinyin IME 会吞 XTEST 字符，两条路径都打到同一 JS 处理器）

## 证据文件索引

| 文件 | 内容 |
| --- | --- |
| 01-content-baseline.json / 01-toolbar-baseline.json | 缩放前基线（内容 inner_w / 工具栏 zoomval） |
| 02-zoom-in1-*.json、125-*.json、0125-*.json、150-*.json、0150-*.json、200-*.json、0200-*.json | 缩放逐档 100→110→125→150→175(0150)→200 每档双端读数 |
| shot-zoom-200.png / 200-shot.png | 200% 窗口截图 |
| reset100-*.json | Ctrl+0 复位 100% 双端读数 |
| minus90-*.json | Ctrl+- 到 90% 下界钳制双端读数 |
| 21-testpage-loaded.json | 靶页加载确认（title + 渲染 needle 数） |
| 22-ctrlf-opened.json | Ctrl+F（真实键）：find_open=true、焦点 findinput、0/0 |
| 23-typed-needle.json | 输入 needle：CEF 回调 count=18（显示 0/18，旧二进制 0 基） |
| 24-26 | 真实键 Enter×2 / Shift+Enter 当时未推进（0/0）——根因：工具栏未持 X 焦点，键进了内容窗口 |
| 27-at-3.json、shot-find-3ofN.png | 上一轮查找栏快照 |
| 70-81（终验轮） | 新二进制（ordinal 1 基修复后）全流程：导航/Ctrl+F/计数/上下条/Esc/缩放快查，见回执 |

## 现场事件时间线（均为外部干扰，非代码缺陷）

1. **21:03 单实例冲突**：有新实例启动，Tauri single-instance 把参数转发给在跑实例，随后两实例均退出（code 0）。当时正跑第一轮验证，查找的 Enter/上下条/Esc 未跑完。
2. **~21:38 撞车**：atomcode 起实例 2（profile cef-f18-2，端口 9362）与王二狗复测探针脚本（`pkill -9 -x shift-browser; sleep 2; ...` 循环）撞车——实例 2 成了第二实例，转发参数后秒退（日志仅 Cookie 行、CDP 从未绑定），非 CDP 接线问题。
3. **21:52:52 起**：王二狗复测实例在跑（pid 116841，父进程即其探针脚本）。atomcode 不干扰，等其窗口期再做终验轮。
4. **靶页被改**：`/tmp/cef-f18-test.html` 在两轮之间被覆盖（needle 从 18 处变 3 处）。终验轮改用新建 `/tmp/cef-f18-final-test.html`（正文恰好 18 处 needle、说明文字不含目标词，`grep -oi` 核数=18）。

## 计数口径说明

- 显示计数 = CEF `FindHandler::on_find_result` 回调真值（Rust 推 `window.__shiftFind(count, active)`），工具栏页面不自己数 DOM。
- C API 的 `active_match_ordinal` 是 0 基（-1=无匹配）；旧二进制直接显示 0 基（首条显示 0/N），已修为 1 基（`saturating_add(1)`，shell.rs on_find_result）。终验轮验证新二进制显示 1/N。

## 关口（终验轮同一二进制）

- `cargo clippy --all-targets`（默认 + `--features cef-engine`）：0 警告
- `cargo test --lib`（默认）：649 passed / 0 failed
- `cargo test --lib --features cef-engine`：669 passed / 2 ignored（两次全量跑一致；`-- --list` 共 671 例；本卡新增 4 个缩放档位纯函数用例，见 zoom_* 四条）
- `npm run lint`：0 错误；`npm run build`：过；`vue-tsc --noEmit`：过

## 终验轮（2026-10-26 23:0x，ordinal 补丁后的二进制）

- 实例 4：Xvfb :99 + 独立 D-Bus 会话 + 独立 profile `/tmp/cef-f18-4`，CDP 端口 9364（隔离原因：王二狗 F-CEF-19 复测探针循环 `pkill -9 -x shift-browser` 会杀同机实例，进程名无法隔离，只能隔 X/D-Bus/profile/端口）。
- 补丁：`on_find_result` 去掉 Rust 侧 `saturating_add(1)`（与工具栏 `Math.min(active+1,count)` 双加一，首条显示 2/N）；该 hunk 被共树卷入二狗 `bf23486`（22:57），未另提交。
- 文件 90-105：90 导航 / 91 靶页确认（18 处渲染）/ 92 Ctrl+F 开栏 / 93 首查 **1/18** / 94 截图（首条 active 高亮）/ 95-97 Enter·Enter·Shift+Enter = 2/18→3/18→2/18 / 98a 重开首查 1/18 / 98b 首条 Shift+Enter **wrap 18/18** / 99 wrap 截图 / 100 单匹配 1/1 / 101a 无匹配 0/0 / 101b 无匹配后再搜 1/18 / 102 Esc 关闭 0/0 / 103 缩放 110% / 104 缩放截图 / 105 缩放复位 100%。
- find 调用 9 次与 Rust 日志 9 行一一对应（`grep -c 页内查找`）。
- 关口（同一二进制）：build 过、clippy 双配置 0 警告、`cargo test --lib --features cef-engine` **669 passed / 2 ignored**（141.59s）。
- 注：第一轮（文件 01-27）跑在补丁前二进制上；其中查找显示类读数（2/18 系列）已被终验轮取代，缩放类读数（01-0200）继续有效（缩放代码未被 ordinal 补丁触碰）。

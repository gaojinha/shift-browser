---
name: e2e-verify
description: 用 playwright MCP + 本项目自带能力做浏览器端 E2E 验收（UI 行为/整页汉化/随手问/live 测试）。当改动涉及前端交互、webview 注入、或用户要求"真机验收/E2E/截图验证"时使用。
allowed-tools: Read, Bash, Grep
---

# E2E 验收（e2e-verify）

$ARGUMENTS 为验收目标描述（如"验证翻译面板打开后不遮挡地址栏"）。

## 三条验收通道（按场景选）

### 通道 A：playwright MCP 直驱前端（dev 下最常用）
1. 确认 dev 环境：`pgrep -f "vite" >/dev/null || (npm run dev &)`，前端在 `http://localhost:5173`。
2. 用 playwright MCP 打开 `http://localhost:5173`，按 $ARGUMENTS 操作。
   - 注意：Tauri 前端里 `@tauri-apps/api` 的 invoke 在纯浏览器里会失败，
     纯 UI 渲染/布局/交互可验，IPC 链路必须走通道 B/C。
3. 关键状态截图存 `docs/test-reports/e2e-$(date +%F)/<序号>-<说明>.png`，
   并用 read_file 查看截图确认（截图是唯一证据，"应该没问题"不算通过）。
4. 出报告：每步操作 + 截图路径 + 结论。

### 通道 B：真机 live 测试（IPC 链路 / opencode 集成）
前置：本机 `opencode serve` 在跑（带 `OPENCODE_SERVER_PASSWORD`）。
```bash
cd src-tauri
cargo test --test webqa_live -- --ignored --nocapture
cargo test --test webzh_live -- --ignored --nocapture
cargo test --test opencode_chat_live -- --ignored --nocapture
```
全过才通过；失败先看报错里 base 地址（默认 127.0.0.1:4096）是否正确。

### 通道 C：smoke.sh 冒烟（启动/解耦链路 17 项）
前置：图形会话 + `src-tauri/target/release/shift-browser` 已构建。
```bash
scripts/smoke.sh            # 完整：重启实例逐项检查
scripts/smoke.sh --no-restart  # 只查运行实例
```
退出码 0 才通过。

## 规矩
- 结论只基于证据（截图/live 输出/退出码），写不出证据就写"未验证"。
- 截图目录当天建、当天留；报告路径回报给用户。

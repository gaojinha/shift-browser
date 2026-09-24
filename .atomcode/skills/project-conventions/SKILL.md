---
name: project-conventions
description: Shift Browser 项目约定与惯例（迁移命名、Tauri unstable API 复核、提交信息、代码风格）。写代码/审查时自动应用。
user-invocable: false
---

# Shift Browser 项目约定

写代码、改配置、提交时自动遵守以下约定。这些规则来自仓库现状，与通用最佳实践冲突时以这里为准。

## 数据库迁移

- 文件：`src-tauri/migrations/`，命名 `YYYYMMDDNNNNNN_<snake_case>.sql`（日期 + 当天 6 位序号）。
- 新增迁移必须注册到 `src-tauri/src/db/mod.rs` 的 `run_migrations()` 向量（version 递增 + `include_str!`）。
- **不改已提交的迁移文件**——需要修正就追加新迁移。
- 详细流程见 `/create-migration` skill。

## Tauri unstable API 复核

项目登记了两个不稳定 API（`Cargo.toml` 注释有完整清单）：
- `WebviewWindow::with_webview()`（约 23 处）
- `Window::gtk_window()`（约 5 处）

**每次升级 tauri / webkit2gtk / gtk 依赖后必须复核**：
```bash
grep -rn "\.with_webview(\|\.gtk_window()" src-tauri/src
```
并在提交说明里列出复核结果。

## 提交信息

- Conventional Commit 格式：`type(scope): 描述`，type/scope 用英文。
- 主题行和正文的叙述部分用**简体中文**；代码标识符、trailer 保持原样。
- 代用户提交时在正文末尾加 trailer（空行分隔）：
  `Co-Authored-By: AtomCode (qwen3.8-27b) <noreply@atomgit.com>`
  （`--amend` / `--revert` 不加。）
- 仓库注释语言偏中文（`Cargo.toml`、`db/mod.rs` 大量中文注释）——新注释随文件现状，不强行英文化。

## 构建与验证

- 前端：`npm run build`（含 `vue-tsc --noEmit`）、`npm run lint`、`npm test`（vitest）。
- 后端：`cd src-tauri && cargo check && cargo clippy`；CI 固定 rust-toolchain（仓库根 `rust-toolchain.toml`）。
- 验证顺序：小改只跑对应侧（改 .vue/.ts 跑前端，改 .rs 跑 cargo）；跨 IPC 契约的改动两侧都跑。
- 不要手改 `package-lock.json` / `Cargo.lock`（已有 hook 拦截）。

## 代码风格

- 前端：ESLint（`eslint.config.js`）+ Prettier（`.prettierrc.json`）；Vue 3 Composition API + Pinia；
  类型严格（tsconfig 严格模式），避免 `any`。
- 后端：clippy 默认配置（`src-tauri/clippy.toml`）；错误用 `thiserror` + `?` 传播，不用 panic 兜底可恢复错误；
  日志走 `log` crate（`tauri-plugin-log`），中文日志前缀如 `[ShiftBrowser]`。
- 前端测试：vitest，`*.test.ts` 与源文件同目录（参考 `src/utils/webview.test.ts`）。

## 双二进制注意

`src-tauri` 同时产出 `shift-browser`（主程序）和 `shift-browser-mcp`（MCP 服务 bin）。
改动 `lib.rs` / 共享模块时注意不要破坏 `--bin shift-browser-mcp` 的编译（CI 只编主程序，MCP bin 靠手动 `cargo build --release --bin shift-browser-mcp`）。

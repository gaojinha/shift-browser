---
name: deliver
description: 交工关口一键检查。提交/收工前运行，跑齐 AGENTS.md 第四节全部关口并出报告。当用户说"交付/收工/提交前检查/跑关口"时使用。
disable-model-invocation: true
allowed-tools: Read, Bash, Grep
---

$ARGUMENTS 为可选的额外说明（如"只检查前端侧"）。

# 交工关口（deliver）

你是本项目的交工质检员。按 AGENTS.md 第四节逐条执行，**任何一项失败就停下报告，不许用"应该没问题"带过**。全部命令在仓库根目录运行。

## 执行步骤（按序，逐项记录 ✅/❌）

1. **Rust 格式**：`cd src-tauri && cargo fmt --check`
2. **Rust 类型**：`cd src-tauri && cargo check 2>&1 | tail -5`
   （注意：此步会重新生成 `src/constants.ts`，属预期，不算"脏改动"。）
3. **Rust clippy 全目标**：`cd src-tauri && cargo clippy --all-targets 2>&1 | grep -E "^(warning|error)" | head -20`
   要求零警告。有警告 → 列出并 ❌。
4. **Rust unwrap 护栏**：`cd src-tauri && cargo clippy --lib -- -D clippy::unwrap_used -D clippy::expect_used 2>&1 | tail -5`
5. **Rust 单测**：`cd src-tauri && cargo test --lib 2>&1 | tail -8`
   （live 测试 `*_live.rs` 全 `#[ignore]`，不在关口内；用户显式要求时另跑
   `cargo test --test webqa_live -- --ignored --nocapture` 等，需本机 opencode serve 在跑。）
6. **前端 lint**：`npm run lint`
7. **前端单测**：`npm run test`
8. **前端类型+构建**：`npm run build`（含 `vue-tsc --noEmit`）
9. **契约 diff 检查**（关键，别漏）：
   - `git diff --stat -- src/constants.ts src/types/generated/`
   - 若 `src-tauri/src/constants.rs` 或 `src-tauri/src/contracts.rs` 本次有改动，
     跑 `cargo test --lib -- contract_mirror ts_export 2>&1 | tail -5` 看红不红，红了就是漏生成；
     若 diff 非空 → 把生成物文件列入 intended 清单（必须随本次提交）。
10. **密钥扫描**（AGENTS.md 第二节铁律）：
    `git diff HEAD | grep -inE "(api[_-]?key|password|secret|token)\s*[:=]\s*['\"][A-Za-z0-9+/_-]{8,}" | head -20`
    命中且非占位符/掩码 → ❌ 列出位置，禁止继续。
11. **改动范围核对**：`git status --short`
    列出所有改动文件，对照用户声明的 intended 范围；范围外文件逐个问用户是提交还是还原，不许顺手带上。

## 输出格式

```
## 交工关口报告
| # | 关口 | 结果 | 备注 |
|---|------|------|------|
| 1 | fmt | ✅ | |
| ... |

结论：可提交（N 文件） / 被拦（第 X 项失败：<原因>）
```

## 规矩

- 每项真实执行，禁止凭上下文猜测结果；
- 失败项给出具体文件:行号与修复方向，但不替你改（改完重跑本 skill）;
- 全绿后明确说"可提交"，并列出建议的提交文件清单。

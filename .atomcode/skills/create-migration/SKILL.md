---
name: create-migration
description: 为 shift-browser 创建 SQLite 迁移文件并注册到 run_migrations。当需要新增/修改表、列、索引时使用。
disable-model-invocation: true
allowed-tools: Read, Write, Edit, Grep, Bash
---

为 $ARGUMENTS 创建数据库迁移。

## 项目迁移机制（必读）

- 迁移文件位于 `src-tauri/migrations/`，纯 SQLite 方言（sqlx 0.8 + sqlite feature）。
- 文件名格式：`YYYYMMDDNNNNNN_<snake_case 描述>.sql`
  - `YYYYMMDD` 是创建日期，后 6 位是当天序号（从 `000000` 起）。
  - 参考现有文件：`20260922000000_certificates.sql`、`20260922000001_feed_url_index.sql`。
- 应用入口：`src-tauri/src/db/mod.rs` 的 `run_migrations()`。
  每个迁移必须手动注册到 `let migrations: Vec<(i64, &str, &str)>` 列表中：
  - 第一项是递增的 `version`（取当前列表最大 version + 1）；
  - 第二项是 snake_case 短名（如 `feed_url_index`）；
  - 第三项是 `include_str!("../../migrations/<文件名>.sql")`。
  注册缺失 = 文件永远不会执行（`include_str!` 是编译期检查，写错路径会编译失败）。
- 执行语义：启动时按 version 顺序逐个执行，每个迁移包在独立事务里；
  已应用的（`_migrations` 表记录）会跳过。全新数据库会按顺序跑全部历史，
  所以新迁移必须在"完整历史已应用"的前提下成立。
- **禁止修改已发布（已提交进 git）的迁移文件**——老用户数据库里已按原内容应用过，
  改文件只会让新装机与存量库产生 schema 漂移。需要修正就追加新迁移
  （参考 `20260918000002_passwords_uniq_fix.sql` 的先例）。

## 步骤

1. `ls src-tauri/migrations/` 确认最新文件名，生成新文件名（当天序号 +1）。
2. 用 Write 创建 SQL 文件，结构参考现有文件：
   - 头部一行中文注释说明意图（`-- 2026-09-24: <说明>`）；
   - 尽量幂等/可重入（`IF NOT EXISTS`、先查后建），但注意项目惯例更倾向直接写
     一次性 DDL，复杂索引操作用 `CREATE INDEX IF NOT EXISTS`。
3. 编辑 `src-tauri/src/db/mod.rs`，在 `migrations` 向量末尾追加：
   ```rust
   (
       <下一个 version>,
       "<snake_case 名>",
       include_str!("../../migrations/<新文件名>"),
   ),
   ```
4. 验证：
   ```bash
   cd src-tauri && cargo check 2>&1 | tail -5
   ```
   （`include_str!` 路径错误会在此暴露。）
5. 报告：新文件路径、注册的 version、cargo check 结果。

## 注意

- 涉及密码/用户数据时，同时检查 `src-tauri/src/db/passwords.rs` / `users.rs`
  里的测试是否需要同步更新（它们会 `include_str!` 个别迁移做隔离测试）。
- 不要创建"down 迁移"——本项目无回滚机制，回滚靠追加新迁移。

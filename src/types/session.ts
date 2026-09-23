// 会话类型：由 ts-rs 从 Rust `db::sessions` 生成（唯一来源，勿手改）。
// 生成命令：cd src-tauri && cargo test --lib ts_export
export type { Session } from './generated/Session'
export type { SessionTab } from './generated/SessionTab'

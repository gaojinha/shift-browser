// 密码类型：PasswordMeta 由 ts-rs 从 Rust `db::passwords::PasswordMeta` 生成
//（唯一来源，勿手改；生成命令：cd src-tauri && cargo test --lib ts_export）。
// PasswordEntry 是前端附加（含明文密码，仅内存传递，Rust 侧不落库）。
import type { PasswordMeta } from './generated/PasswordMeta'

export type { PasswordMeta }

export type PasswordEntry = PasswordMeta & {
  password: string
}

// 标签页类型：基础字段由 ts-rs 从 Rust `tabs::Tab` 生成（唯一来源，勿手改）。
// 生成命令：cd src-tauri && cargo test --lib ts_export
// 前端在此附加运行时字段（Rust 侧不存在）。
import type { Tab as TabBase } from './generated/Tab'

export type Tab = TabBase & {
  /** 运行时标签分组 id（前端内存字段，不持久化；null = 未分组） */
  groupId: number | null
}

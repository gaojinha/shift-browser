// 页面理解层类型：由 ts-rs 从 Rust `engine::content` / `engine::webmcp` 生成
//（唯一来源，勿手改）。生成命令：cd src-tauri && cargo test --lib ts_export
export type { Snapshot } from './generated/Snapshot'
export type { AriaElement } from './generated/AriaElement'
export type { InteractiveStats } from './generated/InteractiveStats'
export type { FormField } from './generated/FormField'
export type { LinkInfo } from './generated/LinkInfo'
export type { WebmcpToolDef } from './generated/WebmcpToolDef'

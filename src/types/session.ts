// 会话类型（对应 Rust 侧 db/sessions.rs）
export interface SessionTab {
  url: string
  title?: string
}

export interface Session {
  id: number
  name: string
  workspace_id: number | null
  /** 标签页 JSON 快照 */
  data: string
  created_at: string
}
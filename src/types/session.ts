// 会话类型（对应 Rust 侧 db/sessions.rs）
export interface SessionTab {
  url: string
  title?: string
  /** 标签分组归属（持久化；null = 未分组） */
  groupId?: number | null
}

export interface Session {
  id: number
  name: string
  workspace_id: number | null
  /** 标签页 JSON 快照 */
  data: string
  created_at: string
}

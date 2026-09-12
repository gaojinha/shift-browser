// 下载记录类型（对应 Rust db/downloads.rs 的 Download）
export interface Download {
  id: number
  filename: string
  url: string | null
  path: string | null
  total_bytes: number
  received_bytes: number
  /** in_progress / completed / failed / cancelled */
  state: string
  created_at: string
}
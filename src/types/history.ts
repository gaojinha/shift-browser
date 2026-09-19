// 历史记录类型（对应 Rust 侧 db/history.rs）
export interface HistoryEntry {
  id: number
  url: string
  title: string | null
  visit_count: number
  last_visit_at: string
}
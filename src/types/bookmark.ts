// 书签类型（对应 Rust 侧 db/bookmarks.rs）
export interface Bookmark {
  id: number
  title: string
  url: string
  folder_id: number | null
  sort_order: number
  created_at: string
}

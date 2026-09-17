// 标签页类型（与 Rust tabs::Tab 一致）
export interface Tab {
  id: string
  url: string
  title: string
  favicon: string
  is_active: boolean
  is_pinned: boolean
  is_muted: boolean
  /** 运行时标签分组 id（前端内存字段，不持久化；null = 未分组） */
  groupId: number | null
}

// 标签页类型（与 Rust tabs::Tab 一致）
export interface Tab {
  id: string
  url: string
  title: string
  favicon: string
  is_active: boolean
  is_pinned: boolean
}

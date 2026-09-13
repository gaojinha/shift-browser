// 扩展类型（对应 Rust extensions.rs 的 ExtensionInfo）
export interface Extension {
  id: number
  name: string
  path: string
  enabled: boolean
  version: string | null
  description: string | null
  js_count: number
  css_count: number
  created_at: string
}

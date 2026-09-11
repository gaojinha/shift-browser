// 密码类型（对应 Rust 侧 db/passwords.rs）
export interface PasswordMeta {
  id: number
  domain: string
  username: string | null
  notes: string | null
  created_at: string
  updated_at: string
}

export interface PasswordEntry extends PasswordMeta {
  password: string
}
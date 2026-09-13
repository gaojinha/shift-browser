// 密码管理状态（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'
import type { PasswordEntry, PasswordMeta } from '../types/password'

interface PasswordsState {
  list: PasswordMeta[]
  loaded: boolean
}

export const usePasswordsStore = defineStore('passwords', {
  state: (): PasswordsState => ({
    list: [],
    loaded: false,
  }),

  actions: {
    /** 拉取全部密码记录（不含明文） */
    async loadPasswords() {
      const list = await invoke<PasswordMeta[]>('list_passwords')
      this.list = list
      this.loaded = true
    },

    /** 保存密码（同域名+用户名存在则更新） */
    async savePassword(
      domain: string,
      username: string | null,
      password: string,
      notes?: string | null,
    ) {
      try {
        await invoke<number>('save_password', { domain, username, password, notes })
        await this.loadPasswords()
        useToastStore().success('密码已保存')
      } catch (e) {
        console.error('[passwords] 保存密码失败:', e)
        useToastStore().error('保存密码失败')
      }
    },

    /** 获取某域名密码明文 */
    async getPassword(domain: string): Promise<PasswordEntry | null> {
      return await invoke<PasswordEntry | null>('get_password', { domain })
    },

    /** 按 id 获取密码明文（同域名多账号时精确定位） */
    async getPasswordById(id: number): Promise<PasswordEntry | null> {
      return await invoke<PasswordEntry | null>('get_password_by_id', { id })
    },

    /** 删除密码 */
    async deletePassword(id: number) {
      try {
        await invoke<void>('delete_password', { id })
        this.list = this.list.filter((p) => p.id !== id)
      } catch (e) {
        console.error('[passwords] 删除密码失败:', e)
        useToastStore().error('删除密码失败')
      }
    },
  },
})

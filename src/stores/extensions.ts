// 扩展管理状态（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import type { Extension } from '../types/extension'

interface ExtensionsState {
  list: Extension[]
}

export const useExtensionsStore = defineStore('extensions', {
  state: (): ExtensionsState => ({
    list: [],
  }),

  actions: {
    /** 拉取扩展列表 */
    async loadExtensions() {
      const list = await invoke<Extension[]>('list_extensions')
      this.list = list
      return list
    },

    /** 加载扩展（目录或 manifest.json 路径） */
    async loadExtension(path: string) {
      const ext = await invoke<Extension>('load_extension', { path })
      await this.loadExtensions()
      return ext
    },

    /** 启用/禁用扩展 */
    async setEnabled(id: number, enabled: boolean) {
      await invoke('set_extension_enabled', { id, enabled })
      const item = this.list.find((e) => e.id === id)
      if (item) item.enabled = enabled
    },

    /** 删除扩展 */
    async deleteExtension(id: number) {
      await invoke('delete_extension', { id })
      this.list = this.list.filter((e) => e.id !== id)
    },
  },
})

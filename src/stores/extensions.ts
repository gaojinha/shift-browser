// 扩展管理状态（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'
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
      try {
        const ext = await invoke<Extension>('load_extension', { path })
        await this.loadExtensions()
        return ext
      } catch (e) {
        console.error('[extensions] 加载扩展失败:', e)
        useToastStore().error('加载扩展失败，请检查路径是否正确')
        throw e
      }
    },

    /** 启用/禁用扩展 */
    async setEnabled(id: number, enabled: boolean) {
      try {
        await invoke('set_extension_enabled', { id, enabled })
        const item = this.list.find((e) => e.id === id)
        if (item) item.enabled = enabled
      } catch (e) {
        console.error('[extensions] 切换扩展状态失败:', e)
        // 回写原值触发重渲染，避免 checkbox 视觉状态与 store 脱节
        const item = this.list.find((e) => e.id === id)
        if (item) item.enabled = !enabled
        useToastStore().error('切换扩展状态失败')
      }
    },

    /** 删除扩展 */
    async deleteExtension(id: number) {
      try {
        await invoke('delete_extension', { id })
        this.list = this.list.filter((e) => e.id !== id)
      } catch (e) {
        console.error('[extensions] 删除扩展失败:', e)
        useToastStore().error('删除扩展失败')
      }
    },
  },
})

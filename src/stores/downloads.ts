// 下载记录状态管理（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'
import type { Download } from '../types/download'

interface DownloadsState {
  list: Download[]
  loading: boolean
}

export const useDownloadsStore = defineStore('downloads', {
  state: (): DownloadsState => ({
    list: [],
    loading: false,
  }),

  actions: {
    /** 拉取全部下载记录 */
    async loadDownloads() {
      this.loading = true
      try {
        const list = await invoke<Download[]>('get_downloads')
        this.list = list
        return list
      } finally {
        this.loading = false
      }
    },

    /** 删除下载记录 */
    async deleteDownload(id: number) {
      try {
        await invoke('delete_download', { id })
        this.list = this.list.filter((d) => d.id !== id)
      } catch (e) {
        console.error('[downloads] 删除下载记录失败:', e)
        useToastStore().error('删除下载记录失败')
      }
    },
  },
})

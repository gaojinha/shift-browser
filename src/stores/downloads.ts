// 下载记录状态管理（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import type { Download } from '../types/download'

interface DownloadsState {
  list: Download[]
}

export const useDownloadsStore = defineStore('downloads', {
  state: (): DownloadsState => ({
    list: [],
  }),

  actions: {
    /** 拉取全部下载记录 */
    async loadDownloads() {
      const list = await invoke<Download[]>('get_downloads')
      this.list = list
      return list
    },

    /** 记录下载开始 */
    async createDownload(
      filename: string,
      url: string | null = null,
      path: string | null = null,
      totalBytes = 0,
    ) {
      const id = await invoke<number>('create_download', { filename, url, path, totalBytes })
      await this.loadDownloads()
      return id
    },

    /** 更新下载进度/状态 */
    async updateProgress(id: number, receivedBytes: number, state?: string) {
      await invoke('update_download_progress', { id, receivedBytes, state: state ?? null })
      // 进度更新频繁，不每次全量刷新；由列表打开时 loadDownloads 兜底
    },

    /** 删除下载记录 */
    async deleteDownload(id: number) {
      await invoke('delete_download', { id })
      this.list = this.list.filter((d) => d.id !== id)
    },
  },
})

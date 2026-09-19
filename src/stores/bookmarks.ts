// 书签状态管理（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'
import type { Bookmark } from '../types/bookmark'

interface BookmarksState {
  bookmarks: Bookmark[]
  loaded: boolean
}

export const useBookmarksStore = defineStore('bookmarks', {
  state: (): BookmarksState => ({
    bookmarks: [],
    loaded: false,
  }),

  actions: {
    /** 拉取全部书签 */
    async loadBookmarks() {
      const bookmarks = await invoke<Bookmark[]>('get_bookmarks')
      this.bookmarks = bookmarks
      this.loaded = true
    },

    /** 添加书签 */
    async addBookmark(title: string, url: string) {
      try {
        await invoke<number>('create_bookmark', {
          input: { title, url, folder_id: null },
        })
        await this.loadBookmarks()
      } catch (e) {
        console.error('[bookmarks] 添加书签失败:', e)
        useToastStore().error('添加书签失败')
      }
    },

    /** 删除书签，成功返回 true，失败返回 false（不再吞错伪装成功，2026-09-19 修复） */
    async removeBookmark(id: number): Promise<boolean> {
      try {
        await invoke<void>('delete_bookmark', { id })
        this.bookmarks = this.bookmarks.filter((b) => b.id !== id)
        return true
      } catch (e) {
        console.error('[bookmarks] 删除书签失败:', e)
        useToastStore().error('删除书签失败')
        return false
      }
    },

    /** 更新书签（标题/URL） */
    async updateBookmark(id: number, title?: string, url?: string) {
      try {
        await invoke<void>('update_bookmark', { id, title: title ?? null, url: url ?? null, folderId: null })
        await this.loadBookmarks()
      } catch (e) {
        console.error('[bookmarks] 更新书签失败:', e)
        useToastStore().error('更新书签失败')
      }
    },

    /** 更新书签所属文件夹（null = 未分类，Rust 侧 0 表示置 NULL） */
    async updateBookmarkFolder(id: number, folderId: number | null) {
      try {
        await invoke<void>('update_bookmark', { id, title: null, url: null, folderId: folderId ?? 0 })
        await this.loadBookmarks()
      } catch (e) {
        console.error('[bookmarks] 更新书签文件夹失败:', e)
        useToastStore().error('更新书签文件夹失败')
      }
    },

    /** 新建文件夹 */
    async createFolder(name: string): Promise<number | null> {
      try {
        const id = await invoke<number>('create_bookmark_folder', {
          input: { name, parent_id: null },
        })
        return id
      } catch (e) {
        console.error('[bookmarks] 新建文件夹失败:', e)
        useToastStore().error('新建文件夹失败')
        return null
      }
    },

    /** 文件夹列表（管理弹窗用） */
    async loadFolders(): Promise<{ id: number; name: string }[]> {
      try {
        return await invoke<{ id: number; name: string }[]>('get_bookmark_folders')
      } catch (e) {
        console.error('[bookmarks] 加载文件夹失败:', e)
        return []
      }
    },

    /** 导出书签为 Netscape HTML 到下载目录，返回文件路径 */
    async exportHtml(): Promise<string> {
      const path = await invoke<string>('export_bookmarks_html')
      return path
    },
  },
})

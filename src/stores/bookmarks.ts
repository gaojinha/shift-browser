// 书签状态管理（Pinia）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
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
      await invoke<number>('create_bookmark', {
        input: { title, url, folder_id: null },
      })
      await this.loadBookmarks()
    },

    /** 删除书签 */
    async removeBookmark(id: number) {
      await invoke<void>('delete_bookmark', { id })
      this.bookmarks = this.bookmarks.filter((b) => b.id !== id)
    },
  },
})
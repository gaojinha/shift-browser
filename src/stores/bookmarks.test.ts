// bookmarks store 单测（F-72）
// 契约：loadBookmarks 置 loaded / 写操作成功后重载 / 失败 toast 且不伪装成功 / 文件夹接口降级
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useBookmarksStore } from './bookmarks'
import { useToastStore } from './toast'
import type { Bookmark } from '../types/bookmark'

const bm = (id: number): Bookmark => ({ id, title: `t${id}`, url: `https://e${id}.com/` }) as Bookmark

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

describe('bookmarks store', () => {
  it('loadBookmarks: 填充列表并置 loaded，重复调用以最新为准', async () => {
    invokeMock.mockResolvedValueOnce([bm(1), bm(2)]).mockResolvedValueOnce([bm(3)])
    const s = useBookmarksStore()
    expect(s.loaded).toBe(false)
    await s.loadBookmarks()
    expect(s.loaded).toBe(true)
    expect(s.bookmarks.map((b) => b.id)).toEqual([1, 2])
    await s.loadBookmarks()
    expect(s.bookmarks.map((b) => b.id)).toEqual([3])
  })

  it('loadBookmarks: 失败向上抛且不置 loaded', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useBookmarksStore()
    await expect(s.loadBookmarks()).rejects.toThrow('boom')
    expect(s.loaded).toBe(false)
  })

  it('addBookmark: 以 folder_id=null 创建后重载', async () => {
    invokeMock.mockResolvedValueOnce(7).mockResolvedValueOnce([bm(7)])
    const s = useBookmarksStore()
    await s.addBookmark('T', 'https://a.com/')
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'create_bookmark', {
      input: { title: 'T', url: 'https://a.com/', folder_id: null },
    })
    expect(s.bookmarks).toHaveLength(1)
  })

  it('addBookmark: 失败弹 toast，不重载', async () => {
    invokeMock.mockRejectedValueOnce(new Error('x'))
    const s = useBookmarksStore()
    await s.addBookmark('T', 'https://a.com/')
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(useToastStore().items.map((t) => t.message)).toContain('添加书签失败')
  })

  it('removeBookmark: 成功本地剔除返回 true；失败返回 false 且列表不变', async () => {
    const s = useBookmarksStore()
    s.bookmarks = [bm(1), bm(2)]
    invokeMock.mockResolvedValueOnce(undefined)
    expect(await s.removeBookmark(1)).toBe(true)
    expect(s.bookmarks.map((b) => b.id)).toEqual([2])
    invokeMock.mockRejectedValueOnce(new Error('x'))
    expect(await s.removeBookmark(2)).toBe(false)
    expect(s.bookmarks.map((b) => b.id)).toEqual([2])
    expect(useToastStore().items.map((t) => t.message)).toContain('删除书签失败')
  })

  it('updateBookmark: 缺省字段传 null；updateBookmarkFolder: null 映射为 0', async () => {
    invokeMock.mockResolvedValue([])
    const s = useBookmarksStore()
    await s.updateBookmark(5, 'new')
    expect(invokeMock).toHaveBeenCalledWith('update_bookmark', { id: 5, title: 'new', url: null, folderId: null })
    await s.updateBookmarkFolder(5, null)
    expect(invokeMock).toHaveBeenCalledWith('update_bookmark', { id: 5, title: null, url: null, folderId: 0 })
    await s.updateBookmarkFolder(5, 9)
    expect(invokeMock).toHaveBeenCalledWith('update_bookmark', { id: 5, title: null, url: null, folderId: 9 })
  })

  it('createFolder: 成功返回 id（默认顶层）；失败返回 null', async () => {
    const s = useBookmarksStore()
    invokeMock.mockResolvedValueOnce(3)
    expect(await s.createFolder('F')).toBe(3)
    expect(invokeMock).toHaveBeenCalledWith('create_bookmark_folder', { input: { name: 'F', parent_id: null } })
    invokeMock.mockRejectedValueOnce(new Error('x'))
    expect(await s.createFolder('G', 3)).toBeNull()
  })

  it('loadFolders 失败降级为空数组；exportHtml 透传路径', async () => {
    const s = useBookmarksStore()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    expect(await s.loadFolders()).toEqual([])
    invokeMock.mockResolvedValueOnce('/tmp/b.html')
    expect(await s.exportHtml()).toBe('/tmp/b.html')
  })
})

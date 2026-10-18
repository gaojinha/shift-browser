// downloads store 单测（F-80，王二狗）
// 契约：拉取期间 loading 置位、成败都复位（finally）/ 返回值透传 /
//       删除成功本地过滤 / 删除失败 toast 且列表不动。
// 范式：真实 pinia + mock @tauri-apps/api/core 的 invoke（见 stores/settings.test.ts）。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useDownloadsStore } from './downloads'
import { useToastStore } from './toast'
import type { Download } from '../types/download'

function dl(id: number, over: Partial<Download> = {}): Download {
  return {
    id,
    filename: `file${id}.zip`,
    url: `https://example.com/file${id}.zip`,
    path: `/tmp/file${id}.zip`,
    total_bytes: 1000,
    received_bytes: 1000,
    state: 'completed',
    created_at: '2026-10-15T00:00:00Z',
    ...over,
  }
}

const LIST = [dl(1), dl(2)]

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
})

describe('downloads store：列表拉取', () => {
  it('loadDownloads：调 get_downloads，写入 list 并返回；loading 置位后复位', async () => {
    invokeMock.mockImplementationOnce(async () => {
      // 拉取中应处于 loading
      expect(s.loading).toBe(true)
      return [...LIST]
    })
    const s = useDownloadsStore()
    expect(s.loading).toBe(false)

    const r = await s.loadDownloads()
    expect(invokeMock).toHaveBeenCalledWith('get_downloads')
    expect(r).toEqual(LIST)
    expect(s.list).toEqual(LIST)
    expect(s.loading).toBe(false)
  })

  it('loadDownloads 失败：loading 仍复位（finally），错误向上抛', async () => {
    invokeMock.mockRejectedValueOnce(new Error('db down'))
    const s = useDownloadsStore()
    await expect(s.loadDownloads()).rejects.toThrow('db down')
    expect(s.loading).toBe(false)
  })
})

describe('downloads store：删除', () => {
  it('deleteDownload 成功：调 delete_download 并本地过滤', async () => {
    const s = useDownloadsStore()
    s.list = [...LIST]
    await s.deleteDownload(1)
    expect(invokeMock).toHaveBeenCalledWith('delete_download', { id: 1 })
    expect(s.list.map((d) => d.id)).toEqual([2])
  })

  it('deleteDownload 失败：toast 报错、列表不动', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useDownloadsStore()
    s.list = [...LIST]
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')

    await expect(s.deleteDownload(1)).resolves.toBeUndefined()
    expect(errSpy).toHaveBeenCalledWith('删除下载记录失败')
    expect(s.list).toEqual(LIST)
  })
})

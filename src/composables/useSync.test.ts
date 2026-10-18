// useSync 单测（F-74）：状态加载与 URL 回填 / 配置校验 / 推拉成功失败路径 / busy 复位
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useSync } from './useSync'
import { useToastStore } from '../stores/toast'

const status = (over = {}) => ({
  vault_protected: true,
  unlocked: true,
  configured: true,
  ready: true,
  host: 'dav.example.com',
  last_push_at: null,
  last_pull_at: null,
  ...over,
})
const toasts = () => useToastStore().items.map((t) => `${t.type}:${t.message}`)

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

describe('useSync', () => {
  it('loadSyncStatus: 有 host 且 URL 为空时回填 https://host；已有 URL 不覆盖', async () => {
    const s = useSync()
    invokeMock.mockResolvedValue(status())
    await s.loadSyncStatus()
    expect(s.syncUrl.value).toBe('https://dav.example.com')
    s.syncUrl.value = 'https://mine.example.org'
    await s.loadSyncStatus()
    expect(s.syncUrl.value).toBe('https://mine.example.org')
  })

  it('loadSyncStatus: host 为 null 不回填；失败保持 null', async () => {
    const s = useSync()
    invokeMock.mockResolvedValueOnce(status({ host: null }))
    await s.loadSyncStatus()
    expect(s.syncUrl.value).toBe('')
    const s2 = useSync()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await s2.loadSyncStatus()
    expect(s2.syncStatus.value).toBeNull()
  })

  it('saveSyncConfig: 地址或用户名为空白时拒绝', async () => {
    const s = useSync()
    s.syncUrl.value = '  '
    s.syncUser.value = 'u'
    await s.saveSyncConfig()
    expect(invokeMock).not.toHaveBeenCalled()
    expect(toasts()).toContain('error:请填写 WebDAV 地址与用户名')
  })

  it('saveSyncConfig: 成功后清空口令；失败保留口令并复位 busy', async () => {
    const s = useSync()
    s.syncUrl.value = 'https://d.com'
    s.syncUser.value = 'u'
    s.syncPass.value = 'p'
    invokeMock.mockRejectedValueOnce('拒绝')
    await s.saveSyncConfig()
    expect(s.syncPass.value).toBe('p')
    expect(s.syncBusy.value).toBe(false)
    expect(toasts()).toContain('error:配置失败：拒绝')

    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status())
    await s.saveSyncConfig()
    expect(invokeMock).toHaveBeenCalledWith('sync_configure', { url: 'https://d.com', username: 'u', password: 'p' })
    expect(s.syncPass.value).toBe('')
  })

  it('pushSync / pullSync: 成功透传后端消息；失败带前缀且 busy 复位', async () => {
    const s = useSync()
    invokeMock.mockResolvedValueOnce('已推送 3 项').mockResolvedValueOnce(status())
    await s.pushSync()
    expect(toasts()).toContain('success:已推送 3 项')
    invokeMock.mockRejectedValueOnce('断网')
    await s.pullSync()
    expect(toasts()).toContain('error:拉取失败：断网')
    expect(s.syncBusy.value).toBe(false)
  })

  it('clearSync: 成功提示云端保留；失败静默', async () => {
    const s = useSync()
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(status({ configured: false }))
    await s.clearSync()
    expect(s.syncStatus.value?.configured).toBe(false)
    expect(toasts()).toContain('success:已清除本地同步配置（云端包保留）')
    setActivePinia(createPinia())
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await useSync().clearSync()
    expect(toasts()).toEqual([])
  })
})

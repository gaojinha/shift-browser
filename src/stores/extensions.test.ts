// extensions store 单测（F-80，王二狗）
// 契约：列表拉取 / 加载扩展成功即重载并返回 / 加载失败 toast 且 rethrow /
//       启用开关成功就地改、失败回写原值 + toast / 删除成功本地过滤、失败 toast。
// 范式：真实 pinia + mock @tauri-apps/api/core 的 invoke（见 stores/settings.test.ts）。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useExtensionsStore } from './extensions'
import { useToastStore } from './toast'
import type { Extension } from '../types/extension'

function ext(id: number, enabled = true, over: Partial<Extension> = {}): Extension {
  return {
    id,
    name: `ext${id}`,
    path: `/tmp/ext${id}`,
    enabled,
    version: '1.0.0',
    description: null,
    js_count: 1,
    css_count: 0,
    created_at: '2026-10-15T00:00:00Z',
    ...over,
  }
}

const LIST = [ext(1, true), ext(2, false)]

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
})

describe('extensions store：列表拉取', () => {
  it('loadExtensions：调 list_extensions，写入 list 并返回同一引用', async () => {
    invokeMock.mockResolvedValueOnce([...LIST])
    const s = useExtensionsStore()
    const r = await s.loadExtensions()
    expect(invokeMock).toHaveBeenCalledWith('list_extensions')
    expect(s.list).toEqual(LIST)
    expect(r).toEqual(LIST)
  })
})

describe('extensions store：加载扩展', () => {
  it('loadExtension 成功：按 {path} 调 load_extension，随后重载并返回新扩展', async () => {
    const created = ext(9)
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'load_extension') return Promise.resolve(created)
      if (cmd === 'list_extensions') return Promise.resolve([...LIST, created])
      return Promise.resolve(undefined)
    })
    const s = useExtensionsStore()
    const r = await s.loadExtension('/tmp/ext9')
    expect(invokeMock).toHaveBeenCalledWith('load_extension', { path: '/tmp/ext9' })
    expect(invokeMock).toHaveBeenCalledWith('list_extensions')
    expect(r).toEqual(created)
    expect(s.list).toContainEqual(created)
  })

  it('loadExtension 失败：toast 报错并向上抛（供调用方感知）', async () => {
    invokeMock.mockRejectedValueOnce(new Error('bad manifest'))
    const s = useExtensionsStore()
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')

    await expect(s.loadExtension('/tmp/bad')).rejects.toThrow('bad manifest')
    expect(errSpy).toHaveBeenCalledWith('加载扩展失败，请检查路径是否正确')
  })
})

describe('extensions store：启用开关', () => {
  it('setEnabled 成功：按 {id,enabled} 调 set_extension_enabled 并就地改状态', async () => {
    const s = useExtensionsStore()
    s.list = [ext(1, true), ext(2, false)]
    await s.setEnabled(1, false)
    expect(invokeMock).toHaveBeenCalledWith('set_extension_enabled', { id: 1, enabled: false })
    expect(s.list.find((e) => e.id === 1)!.enabled).toBe(false)
    expect(s.list.find((e) => e.id === 2)!.enabled).toBe(false) // 未涉及项不动
  })

  it('setEnabled 失败：回写原值（!enabled）+ toast', async () => {
    invokeMock.mockRejectedValueOnce(new Error('disk full'))
    const s = useExtensionsStore()
    s.list = [ext(1, true)]
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')

    await s.setEnabled(1, false)
    expect(errSpy).toHaveBeenCalledWith('切换扩展状态失败')
    // 目标是 false，失败后回写为 true（原值）
    expect(s.list.find((e) => e.id === 1)!.enabled).toBe(true)
  })

  it('setEnabled 对不存在的 id：不崩（失败回写路径 find 落空）', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useExtensionsStore()
    s.list = []
    await expect(s.setEnabled(999, true)).resolves.toBeUndefined()
  })
})

describe('extensions store：删除', () => {
  it('deleteExtension 成功：调 delete_extension 并本地过滤', async () => {
    const s = useExtensionsStore()
    s.list = [...LIST]
    await s.deleteExtension(1)
    expect(invokeMock).toHaveBeenCalledWith('delete_extension', { id: 1 })
    expect(s.list.map((e) => e.id)).toEqual([2])
  })

  it('deleteExtension 失败：toast 报错、列表不动', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useExtensionsStore()
    s.list = [...LIST]
    const toast = useToastStore()
    const errSpy = vi.spyOn(toast, 'error')

    await expect(s.deleteExtension(1)).resolves.toBeUndefined()
    expect(errSpy).toHaveBeenCalledWith('删除扩展失败')
    expect(s.list).toEqual(LIST)
  })
})

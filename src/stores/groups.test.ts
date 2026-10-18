// groups store 单测（F-72）
// 契约：自动组不落库且不被 loadGroups 冲掉 / 手动组走命令并重载 / 失败 toast 并上抛 / pruneAutoGroups 只丢空自动组
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { useGroupsStore, type TabGroup } from './groups'
import { useToastStore } from './toast'

const g = (id: number, over: Partial<TabGroup> = {}): TabGroup => ({
  id,
  name: `g${id}`,
  color: '#fff',
  collapsed: false,
  created_at: '2026-01-01',
  ...over,
})

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
  vi.spyOn(console, 'info').mockImplementation(() => {})
})

describe('groups store', () => {
  it('loadGroups: 数据库组在前，保留内存自动组；loading 复位', async () => {
    const s = useGroupsStore()
    s.groups = [g(-1, { isAuto: true }), g(1)]
    invokeMock.mockResolvedValueOnce([g(2), g(3)])
    await s.loadGroups()
    expect(s.groups.map((x) => x.id)).toEqual([2, 3, -1])
    expect(s.loading).toBe(false)
  })

  it('loadGroups: 失败上抛且 loading 复位', async () => {
    const s = useGroupsStore()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await expect(s.loadGroups()).rejects.toThrow('x')
    expect(s.loading).toBe(false)
  })

  it('createGroup(auto): 不调用 invoke，生成负 id 的临时组', async () => {
    const s = useGroupsStore()
    const id = await s.createGroup('example.com', '#000', true)
    expect(id).toBeLessThan(0)
    expect(invokeMock).not.toHaveBeenCalled()
    expect(s.groups[0]).toMatchObject({ name: 'example.com', isAuto: true })
  })

  it('createGroup(手动): 落库后重载并返回 id，默认颜色', async () => {
    const s = useGroupsStore()
    invokeMock.mockResolvedValueOnce(5).mockResolvedValueOnce([g(5)])
    expect(await s.createGroup('Work')).toBe(5)
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'create_tab_group', { name: 'Work', color: '#4a9eff' })
    expect(s.groups.map((x) => x.id)).toEqual([5])
  })

  it('createGroup 失败: toast 并上抛', async () => {
    const s = useGroupsStore()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await expect(s.createGroup('Work')).rejects.toThrow('x')
    expect(useToastStore().items.map((t) => t.message)).toContain('创建分组失败')
  })

  it('pruneAutoGroups: 只丢未被使用的自动组，手动组保留', () => {
    const s = useGroupsStore()
    s.groups = [g(1), g(-1, { isAuto: true }), g(-2, { isAuto: true })]
    s.pruneAutoGroups(new Set([-2]))
    expect(s.groups.map((x) => x.id)).toEqual([1, -2])
  })

  it('updateGroup / deleteGroup: 成功重载，失败 toast 并上抛', async () => {
    const s = useGroupsStore()
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce([])
    await s.updateGroup(1, { collapsed: true })
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'update_tab_group', { id: 1, collapsed: true })
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await expect(s.deleteGroup(1)).rejects.toThrow('x')
    const msgs = useToastStore().items.map((t) => t.message)
    expect(msgs).toContain('删除分组失败')
  })
})

// workspaces store 单测（F-73 前端测试补课）
// 契约：load 成功/失败兜底、getters 默认工作区语义、create 命名与调色板回绕、
// rename/setColor 参数、delete 当前/非当前（保存失败中止）、
// switch 安全顺序（保存→清空→恢复→提交）与中止/重建/回滚全路径
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

// tabs/toast store 以 mock 替身介入：精确断言 workspaces 对标签的编排顺序与中止条件
const tabsMock = {
  saveCurrentSession: vi.fn(),
  clearAllTabs: vi.fn(),
  restoreLastSession: vi.fn(),
  createTab: vi.fn(),
  assignTabToGroup: vi.fn(),
}
vi.mock('./tabs', () => ({
  useTabsStore: () => tabsMock,
}))

const toastError = vi.fn()
vi.mock('./toast', () => ({
  useToastStore: () => ({ error: toastError }),
}))

import { useWorkspacesStore, type Workspace } from './workspaces'
import { UI } from '../constants'

const ws = (id: number, name: string, color = UI.spaceColors[0]): Workspace => ({
  id,
  name,
  color,
  sort_order: id,
  created_at: '2026-10-18 00:00:00',
})

// 默认 invoke：get_workspaces 返回空列表（loadWorkspaces 兜底安全），其余命令成功
const defaultInvoke = (cmd: string) =>
  cmd === 'get_workspaces' ? Promise.resolve([]) : Promise.resolve(undefined)

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset().mockImplementation(defaultInvoke)
  tabsMock.saveCurrentSession.mockReset().mockResolvedValue(true)
  tabsMock.clearAllTabs.mockReset().mockResolvedValue([])
  tabsMock.restoreLastSession.mockReset().mockResolvedValue(true)
  tabsMock.createTab.mockReset().mockResolvedValue({ id: 'new-tab' })
  tabsMock.assignTabToGroup.mockReset()
  toastError.mockClear()
})

describe('useWorkspacesStore 加载与 getters', () => {
  it('loadWorkspaces: 成功时按 invoke 结果填充列表', async () => {
    invokeMock.mockResolvedValue([ws(1, 'A'), ws(2, 'B')])
    const s = useWorkspacesStore()
    await s.loadWorkspaces()
    expect(invokeMock).toHaveBeenCalledWith('get_workspaces')
    expect(s.workspaces.map((w) => w.id)).toEqual([1, 2])
  })

  it('loadWorkspaces: invoke 失败时空表兜底，不抛出', async () => {
    invokeMock.mockRejectedValue(new Error('db locked'))
    const s = useWorkspacesStore()
    await expect(s.loadWorkspaces()).resolves.toBeUndefined()
    expect(s.workspaces).toEqual([])
  })

  it('getters: currentId=null（默认工作区）→ current 空、名称"默认"、无色', () => {
    const s = useWorkspacesStore()
    s.workspaces = [ws(1, 'A')]
    expect(s.current).toBeNull()
    expect(s.currentName).toBe('默认')
    expect(s.currentColor).toBe('')
  })

  it('getters: 指向存在项返回其名称与色；指向不存在项回退"默认"/无色', () => {
    const s = useWorkspacesStore()
    s.workspaces = [ws(1, 'A', '#111111'), ws(2, 'B', '#222222')]
    s.currentId = 2
    expect(s.current?.name).toBe('B')
    expect(s.currentName).toBe('B')
    expect(s.currentColor).toBe('#222222')
    // 边界：currentId 指向已被删除的 id（删除/竞态窗口）不崩，回退默认语义
    s.currentId = 99
    expect(s.current).toBeNull()
    expect(s.currentName).toBe('默认')
    expect(s.currentColor).toBe('')
  })
})

describe('useWorkspacesStore 增删改', () => {
  it('createWorkspace: 空列表建"空间 1"并分配首色，成功后刷新', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_workspace') return Promise.resolve(7)
      if (cmd === 'get_workspaces') return Promise.resolve([ws(7, '空间 1', UI.spaceColors[0])])
      return Promise.resolve(undefined)
    })
    const s = useWorkspacesStore()
    const id = await s.createWorkspace()
    expect(id).toBe(7)
    expect(invokeMock).toHaveBeenCalledWith('create_workspace', { name: '空间 1' })
    expect(invokeMock).toHaveBeenCalledWith('update_workspace', {
      id: 7,
      name: null,
      sort_order: null,
      color: UI.spaceColors[0],
    })
    expect(s.workspaces).toHaveLength(1)
  })

  it('createWorkspace: 命名按现有数量递增，第 9 个空间调色板回绕到首位（(9-1)%8=0）', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_workspace') return Promise.resolve(9)
      return defaultInvoke(cmd)
    })
    const s = useWorkspacesStore()
    s.workspaces = [1, 2, 3, 4, 5, 6, 7, 8].map((i) => ws(i, `空间 ${i}`))
    const id = await s.createWorkspace()
    expect(id).toBe(9)
    expect(invokeMock).toHaveBeenCalledWith('create_workspace', { name: '空间 9' })
    expect(invokeMock).toHaveBeenCalledWith('update_workspace', {
      id: 9,
      name: null,
      sort_order: null,
      color: UI.spaceColors[0],
    })
  })

  it('createWorkspace: invoke 失败返回 null 并弹 toast', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useWorkspacesStore()
    expect(await s.createWorkspace()).toBeNull()
    expect(toastError).toHaveBeenCalledWith('创建空间失败')
  })

  it('renameWorkspace: 按 {id,name,color:null} 更新并刷新列表', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'get_workspaces' ? Promise.resolve([ws(1, '新名')]) : Promise.resolve(undefined),
    )
    const s = useWorkspacesStore()
    await s.renameWorkspace(1, '新名')
    expect(invokeMock).toHaveBeenCalledWith('update_workspace', {
      id: 1,
      name: '新名',
      sort_order: null,
      color: null,
    })
    expect(s.workspaces[0].name).toBe('新名')
  })

  it('renameWorkspace: 失败弹 toast，不向上抛', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useWorkspacesStore()
    await expect(s.renameWorkspace(1, 'x')).resolves.toBeUndefined()
    expect(toastError).toHaveBeenCalledWith('重命名空间失败')
  })

  it('setColor: 按 {id,color} 更新（name/sort_order 传 null 不动）', async () => {
    const s = useWorkspacesStore()
    await s.setColor(1, '#e53935')
    expect(invokeMock).toHaveBeenCalledWith('update_workspace', {
      id: 1,
      name: null,
      sort_order: null,
      color: '#e53935',
    })
  })

  it('deleteWorkspace: 非当前工作区直接删并刷新，不触碰标签', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'get_workspaces' ? Promise.resolve([ws(2, 'B')]) : Promise.resolve(undefined),
    )
    const s = useWorkspacesStore()
    s.workspaces = [ws(1, 'A'), ws(2, 'B')]
    s.currentId = 1
    await s.deleteWorkspace(2)
    expect(invokeMock).toHaveBeenCalledWith('delete_workspace', { id: 2 })
    expect(s.currentId).toBe(1)
    expect(tabsMock.saveCurrentSession).not.toHaveBeenCalled()
    expect(tabsMock.clearAllTabs).not.toHaveBeenCalled()
    expect(s.workspaces).toHaveLength(1)
  })

  it('deleteWorkspace: 删当前工作区 → 存默认会话/清标签/回默认/恢复默认会话', async () => {
    invokeMock.mockImplementation((cmd: string) =>
      cmd === 'get_workspaces' ? Promise.resolve([ws(2, 'B')]) : Promise.resolve(undefined),
    )
    const s = useWorkspacesStore()
    s.workspaces = [ws(1, 'A'), ws(2, 'B')]
    s.currentId = 1
    await s.deleteWorkspace(1)
    expect(tabsMock.saveCurrentSession).toHaveBeenCalledWith(null)
    expect(tabsMock.clearAllTabs).toHaveBeenCalled()
    expect(s.currentId).toBeNull()
    expect(tabsMock.restoreLastSession).toHaveBeenCalledWith(null)
    expect(toastError).not.toHaveBeenCalled()
  })

  it('deleteWorkspace: 当前工作区但保存会话失败 → 中止，不清标签不回默认', async () => {
    tabsMock.saveCurrentSession.mockResolvedValueOnce(false)
    const s = useWorkspacesStore()
    s.workspaces = [ws(1, 'A'), ws(2, 'B')]
    s.currentId = 1
    await s.deleteWorkspace(1)
    expect(toastError).toHaveBeenCalledWith('保存标签失败，已中止删除后的切换')
    expect(tabsMock.clearAllTabs).not.toHaveBeenCalled()
    expect(tabsMock.restoreLastSession).not.toHaveBeenCalled()
    expect(s.currentId).toBe(1)
  })

  it('deleteWorkspace: invoke 失败弹 toast，currentId 不变', async () => {
    invokeMock.mockRejectedValueOnce(new Error('boom'))
    const s = useWorkspacesStore()
    s.currentId = 2
    await s.deleteWorkspace(2)
    expect(toastError).toHaveBeenCalledWith('删除工作区失败')
    expect(s.currentId).toBe(2)
  })
})

describe('useWorkspacesStore switchWorkspace 安全顺序', () => {
  it('切到当前工作区直接返回，不触发任何标签操作', async () => {
    const s = useWorkspacesStore()
    s.currentId = 1
    await s.switchWorkspace(1)
    expect(tabsMock.saveCurrentSession).not.toHaveBeenCalled()
    expect(s.switching).toBe(false)
  })

  it('切换进行中（switching=true）重复调用被挡，不二次保存', async () => {
    let release!: (v: boolean) => void
    tabsMock.saveCurrentSession.mockImplementationOnce(
      () => new Promise<boolean>((r) => {
        release = r
      }),
    )
    const s = useWorkspacesStore()
    s.currentId = 1
    const p1 = s.switchWorkspace(2)
    await s.switchWorkspace(3) // in-flight → 直接返回
    expect(tabsMock.saveCurrentSession).toHaveBeenCalledTimes(1)
    release(true)
    await p1
    expect(s.currentId).toBe(2)
    expect(s.switching).toBe(false)
  })

  it('成功路径按 保存→清空→恢复→提交 currentId 顺序执行', async () => {
    const s = useWorkspacesStore()
    s.currentId = 1
    await s.switchWorkspace(2)
    expect(tabsMock.saveCurrentSession).toHaveBeenNthCalledWith(1, 1)
    expect(tabsMock.restoreLastSession).toHaveBeenNthCalledWith(1, 2)
    expect(s.currentId).toBe(2)
    const saveIdx = tabsMock.saveCurrentSession.mock.invocationCallOrder[0]
    const clearIdx = tabsMock.clearAllTabs.mock.invocationCallOrder[0]
    const restoreIdx = tabsMock.restoreLastSession.mock.invocationCallOrder[0]
    expect(saveIdx).toBeLessThan(clearIdx)
    expect(clearIdx).toBeLessThan(restoreIdx)
  })

  it('保存失败直接中止——不清空、不切换、currentId 不变', async () => {
    tabsMock.saveCurrentSession.mockResolvedValueOnce(false)
    const s = useWorkspacesStore()
    s.currentId = 1
    await s.switchWorkspace(2)
    expect(toastError).toHaveBeenCalledWith('保存当前工作区失败，已中止切换')
    expect(tabsMock.clearAllTabs).not.toHaveBeenCalled()
    expect(s.currentId).toBe(1)
  })

  it('目标无会话但原快照重建成功 → 留在目标，标签不丢，不弹错', async () => {
    tabsMock.restoreLastSession.mockResolvedValueOnce(false)
    tabsMock.clearAllTabs.mockResolvedValueOnce([
      { url: 'https://a.com', title: 'A', groupId: null },
      { url: 'https://b.com', title: 'B', groupId: 5 },
    ])
    let n = 0
    tabsMock.createTab.mockImplementation(async (url: string) => {
      n += 1
      return { id: `rebuilt-${n}`, url }
    })
    const s = useWorkspacesStore()
    s.currentId = 1
    await s.switchWorkspace(2)
    expect(tabsMock.createTab).toHaveBeenCalledTimes(2)
    expect(tabsMock.assignTabToGroup).toHaveBeenCalledWith('rebuilt-2', 5)
    expect(s.currentId).toBe(2)
    // 只恢复过目标一次（无回滚）
    expect(tabsMock.restoreLastSession).toHaveBeenCalledTimes(1)
    expect(toastError).not.toHaveBeenCalled()
  })

  it('快照全部重建失败 → 回滚原工作区并提示（无 url 项跳过不建页）', async () => {
    tabsMock.restoreLastSession.mockImplementationOnce(async (id: number | null) => id === 1)
    tabsMock.clearAllTabs.mockResolvedValueOnce([
      { url: 'https://a.com', title: 'A', groupId: null },
      { title: '无 url 项', groupId: null },
    ])
    tabsMock.createTab.mockRejectedValueOnce(new Error('tab create fail'))
    const s = useWorkspacesStore()
    s.currentId = 1
    await s.switchWorkspace(2)
    expect(tabsMock.createTab).toHaveBeenCalledTimes(1) // 无 url 项不建页
    expect(tabsMock.restoreLastSession).toHaveBeenNthCalledWith(1, 2)
    expect(tabsMock.restoreLastSession).toHaveBeenNthCalledWith(2, 1)
    expect(s.currentId).toBe(1)
    expect(toastError).toHaveBeenCalledWith('切换工作区失败，已回滚')
  })

  it('恢复抛异常 → 清空+回滚原工作区，currentId 复位、switching 复位', async () => {
    tabsMock.restoreLastSession.mockRejectedValueOnce(new Error('restore boom'))
    const s = useWorkspacesStore()
    s.currentId = 1
    await expect(s.switchWorkspace(2)).resolves.toBeUndefined()
    expect(tabsMock.clearAllTabs).toHaveBeenCalledTimes(2) // 正常清空 + 回滚清空
    expect(tabsMock.restoreLastSession).toHaveBeenNthCalledWith(2, 1)
    expect(s.currentId).toBe(1)
    expect(s.switching).toBe(false)
    expect(toastError).toHaveBeenCalledWith('切换工作区失败')
  })
})

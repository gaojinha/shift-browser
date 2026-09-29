// tabs store 单测（R1 测试补课：创建/切换/关闭/休眠/串行恢复回归）
// 关键并发契约：
//  - _opSeq 过期令牌：close/switch 在途时发生更新的用户操作，过期回调不回写 activeTabId
//  - _switching 合并连点：上一次切换未返回时新点击直接忽略
//  - startPolling 每 8 轮（≈64s）触发一次 hibernate_idle_tabs
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import type { Tab } from '../types/tab'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

import { useTabsStore, setLockedState } from './tabs'
import { useGroupsStore } from './groups'
import { useToastStore } from './toast'

/** 构造合法 Tab（Rust 侧字段 + 前端运行时 groupId） */
function makeTab(id: string, over: Partial<Tab> = {}): Tab {
  return {
    id,
    url: `https://${id}.com/`,
    title: id,
    favicon: '',
    is_active: false,
    is_pinned: false,
    is_muted: false,
    hibernated: false,
    private: false,
    groupId: null,
    ...over,
  }
}

let intervalCb: (() => void) | null = null

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
  intervalCb = null
  setLockedState(false)
  // node 环境无 window/document：_markLoading 用 window.setTimeout，轮询用 window.setInterval
  vi.stubGlobal('window', {
    setTimeout: vi.fn(() => 0),
    setInterval: vi.fn((cb: () => void) => {
      intervalCb = cb
      return 1
    }),
    clearInterval: vi.fn(),
  })
  vi.stubGlobal('document', { hidden: false })
})

describe('loadTabs', () => {
  it('成功：填充列表、补 groupId 默认值、activeTabId 取 is_active', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_tabs') {
        return Promise.resolve([makeTab('a'), makeTab('b', { is_active: true })])
      }
      return Promise.resolve(undefined)
    })
    const s = useTabsStore()
    await s.loadTabs()
    expect(s.tabs.map((t) => t.id)).toEqual(['a', 'b'])
    expect(s.tabs[0].groupId).toBeNull()
    expect(s.activeTabId).toBe('b')
  })

  it('失败且未锁屏：toast 提示；锁屏期间：静默不弹', async () => {
    invokeMock.mockRejectedValue(new Error('db not ready'))
    const s = useTabsStore()
    const toast = useToastStore()
    await s.loadTabs()
    expect(toast.items.some((t) => t.message === '加载标签页失败')).toBe(true)

    setLockedState(true)
    const s2 = useTabsStore()
    const before = toast.items.length
    await s2.loadTabs()
    expect(toast.items.length).toBe(before)
  })
})

describe('createTab', () => {
  it('成功：入列、接管激活、重置导航态；旧标签全部去激活', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('old', { is_active: true })]
    s.activeTabId = 'old'
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_tab') return Promise.resolve(makeTab('new'))
      if (cmd === 'get_tab_groups') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    const tab = await s.createTab('https://new.com/')
    expect(tab?.id).toBe('new')
    expect(s.tabs.map((t) => t.id)).toEqual(['old', 'new'])
    expect(s.tabs[0].is_active).toBe(false)
    expect(s.activeTabId).toBe('new')
    expect(s.canGoBack).toBe(false)
    expect(s.canGoForward).toBe(false)
    expect(invokeMock).toHaveBeenCalledWith('create_tab', {
      url: 'https://new.com/',
      private: false,
      afterActive: false,
      defer: false,
    })
  })

  it('去重：轮询已带入同 id 标签时不重复 push', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('dup')]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_tab') return Promise.resolve(makeTab('dup', { title: '新版' }))
      if (cmd === 'get_tab_groups') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    await s.createTab('https://dup.com/')
    expect(s.tabs.filter((t) => t.id === 'dup')).toHaveLength(1)
    expect(s.tabs[0].title).toBe('新版')
  })

  it('afterActive：插到当前活动标签之后（与 Rust order 一致）', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a', { is_active: true }), makeTab('b')]
    s.activeTabId = 'a'
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_tab') return Promise.resolve(makeTab('c'))
      if (cmd === 'get_tab_groups') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    await s.createTab('https://c.com/', undefined, false, false, true)
    expect(s.tabs.map((t) => t.id)).toEqual(['a', 'c', 'b'])
  })

  it('失败（未锁屏）：返回 null 且 toast，不污染列表', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_tab') return Promise.reject(new Error('webview spawn fail'))
      return Promise.resolve(undefined)
    })
    const s = useTabsStore()
    const toast = useToastStore()
    const r = await s.createTab('https://x.com/')
    expect(r).toBeNull()
    expect(s.tabs).toHaveLength(0)
    expect(toast.items.some((t) => t.message === '创建标签页失败')).toBe(true)
  })
})

describe('closeTab', () => {
  it('成功：列表移除 + activeTabId 回写 Rust 指定的 next', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a'), makeTab('b', { is_active: true }), makeTab('c')]
    s.activeTabId = 'b'
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'close_tab') return Promise.resolve(makeTab('c', { is_active: true }))
      return Promise.resolve(undefined)
    })
    await s.closeTab('b')
    expect(s.tabs.map((t) => t.id)).toEqual(['a', 'c'])
    expect(s.activeTabId).toBe('c')
    expect(s.tabs[1].is_active).toBe(true)
  })

  it('过期令牌：关闭在途时用户已切走，activeTabId 不被回写（列表仍同步）', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a'), makeTab('b', { is_active: true }), makeTab('c')]
    s.activeTabId = 'b'
    let resolveClose: (v: Tab | null) => void = () => {}
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'close_tab') {
        return new Promise((r) => {
          resolveClose = r
        })
      }
      return Promise.resolve(undefined)
    })
    const p = s.closeTab('b')
    // 用户在 close 返回前又做了别的操作（_opSeq 递增）
    s._opSeq++
    resolveClose(makeTab('c', { is_active: true }))
    await p
    expect(s.tabs.map((t) => t.id)).toEqual(['a', 'c'])
    // 过期回调不回写：activeTabId 保持用户现场
    expect(s.activeTabId).toBe('b')
  })
})

describe('switchTab', () => {
  it('成功：激活态切换 + 触发真实导航态同步', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a', { is_active: true }), makeTab('b')]
    s.activeTabId = 'a'
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'switch_tab') return Promise.resolve(makeTab('b', { is_active: true }))
      if (cmd === 'get_nav_state') return Promise.resolve({ canGoBack: true, canGoForward: false })
      return Promise.resolve(undefined)
    })
    await s.switchTab('b')
    expect(s.activeTabId).toBe('b')
    expect(s.tabs[1].is_active).toBe(true)
    expect(s.tabs[0].is_active).toBe(false)
    // 修复契约（2026-09-22）：切标签后必须从 WebKit 同步真实导航状态
    expect(invokeMock).toHaveBeenCalledWith('get_nav_state', { id: 'b' })
    expect(s.canGoBack).toBe(true)
  })

  it('合并连点：上一次切换未返回时新点击直接忽略', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a'), makeTab('b'), makeTab('c')]
    let resolveSwitch: (v: Tab | null) => void = () => {}
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'switch_tab') {
        return new Promise((r) => {
          resolveSwitch = r
        })
      }
      return Promise.resolve(undefined)
    })
    const p1 = s.switchTab('b')
    await s.switchTab('c') // 应被吞掉
    const switchCalls = invokeMock.mock.calls.filter((c) => c[0] === 'switch_tab')
    expect(switchCalls).toHaveLength(1)
    resolveSwitch(makeTab('b', { is_active: true }))
    await p1
    expect(s.activeTabId).toBe('b')
  })

  it('过期回调：切换在途时用户已操作别的，不回写激活态', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a', { is_active: true }), makeTab('b')]
    s.activeTabId = 'a'
    let resolveSwitch: (v: Tab | null) => void = () => {}
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'switch_tab') {
        return new Promise((r) => {
          resolveSwitch = r
        })
      }
      return Promise.resolve(undefined)
    })
    const p = s.switchTab('b')
    s._opSeq++ // 期间发生了更新的用户操作
    resolveSwitch(makeTab('b', { is_active: true }))
    await p
    expect(s.activeTabId).toBe('a')
    expect(s.tabs[0].is_active).toBe(true)
  })

  it('回写返回体：延迟兑现后存量url已变，切过去直接用新数（标签栏/地址栏不两岔）', async () => {
    const s = useTabsStore()
    // 前端列表还是旧数（占位页），后端切时已兑现成真址并返回
    s.tabs = [
      makeTab('a', { is_active: true, url: 'https://a.com/', title: 'a' }),
      makeTab('b', { url: 'shift://newtab', title: '启动页' }),
    ]
    s.activeTabId = 'a'
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'switch_tab') {
        return Promise.resolve(
          makeTab('b', { is_active: true, url: 'https://real.com/', title: 'https://real.com/' }),
        )
      }
      if (cmd === 'get_nav_state') return Promise.resolve({ canGoBack: true, canGoForward: false })
      return Promise.resolve(undefined)
    })
    await s.switchTab('b')
    // 地址栏取值源（activeTab.url）必须已是真址
    expect(s.activeTab?.url).toBe('https://real.com/')
    // 分组归属不能丢（Rust 回来没 groupId 字段）
    expect(s.tabs[1].groupId).toBeNull()
    expect(s.tabs[1].is_active).toBe(true)
  })
})

describe('休眠轮询', () => {
  it('startPolling：每 4 轮触发一次 hibernate_idle_tabs；窗口隐藏时跳过整轮', async () => {
    const s = useTabsStore()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_tabs_version') return Promise.resolve(1)
      if (cmd === 'get_tabs') return Promise.resolve([])
      return Promise.resolve(undefined)
    })
    s.startPolling(15000)
    expect(intervalCb).toBeTruthy()
    // 跑 4 轮 → 恰好 1 次休眠检查
    for (let i = 0; i < 4; i++) await intervalCb!()
    const hibernateCalls = () => invokeMock.mock.calls.filter((c) => c[0] === 'hibernate_idle_tabs')
    expect(hibernateCalls()).toHaveLength(1)
    // 再跑 3 轮不触发；第 4 轮触发第二次
    for (let i = 0; i < 3; i++) await intervalCb!()
    expect(hibernateCalls()).toHaveLength(1)
    await intervalCb!()
    expect(hibernateCalls()).toHaveLength(2)
    // 窗口不可见：整轮跳过（连版本检查都不发）
    invokeMock.mockClear()
    ;(document as { hidden: boolean }).hidden = true
    await intervalCb!()
    expect(invokeMock).not.toHaveBeenCalled()
    s.stopPolling()
    // stopPolling 用全局 clearInterval（非 window.clearInterval），直接断言状态清空
    expect(s.pollTimer).toBeNull()
    expect(s._unlisten).toBeNull()
  })

  it('版本差分：后端无变更跳过全量同步，变了才同步', async () => {
    const s = useTabsStore()
    let version = 5
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_tabs_version') return Promise.resolve(version)
      if (cmd === 'get_tabs') return Promise.resolve([makeTab('a')])
      return Promise.resolve(undefined)
    })
    s.startPolling(15000)
    // 首轮 lastTabVersion 为 null 必同步
    await intervalCb!()
    expect(s.tabs.map((t) => t.id)).toEqual(['a'])
    expect(s.lastTabVersion).toBe(5)
    invokeMock.mockClear()
    // 版本不变 → 跳过 get_tabs
    await intervalCb!()
    expect(invokeMock.mock.calls.filter((c) => c[0] === 'get_tabs')).toHaveLength(0)
    // 版本变了 → 全量同步
    version = 6
    await intervalCb!()
    expect(invokeMock.mock.calls.filter((c) => c[0] === 'get_tabs')).toHaveLength(1)
    expect(s.lastTabVersion).toBe(6)
    s.stopPolling()
  })
})

describe('restoreSession（串行恢复回归基线）', () => {
  const session = (data: unknown) => ({
    id: 1,
    name: 's',
    workspace_id: null,
    data: JSON.stringify(data),
    created_at: '',
  })

  it('批量恢复：一次 create_tabs_batch 建一组，按位认领分组；坏位跳过', async () => {
    const s = useTabsStore()
    const groups = useGroupsStore()
    groups.groups = [{ id: 7, name: '工作', color: '#fff', collapsed: false, created_at: '' }]
    invokeMock.mockImplementation((cmd: string, args: unknown) => {
      if (cmd === 'create_tabs_batch') {
        const urls = (args as { urls: string[] }).urls
        // 第二位失败，验证按位对齐不串组
        return Promise.resolve(urls.map((u, i) => (i === 1 ? null : makeTab(`r${i}`, { url: u }))))
      }
      return Promise.resolve(undefined)
    })
    const ok = await s.restoreSession(
      session([
        { url: 'https://a.com/', title: 'A', groupId: 7 },
        { url: 'https://b.com/', title: 'B', groupId: 7 },
        { url: 'https://c.com/', title: 'C', groupId: null },
      ]),
    )
    expect(ok).toBe(true)
    const batchCalls = invokeMock.mock.calls.filter((c) => c[0] === 'create_tabs_batch')
    expect(batchCalls).toHaveLength(1)
    expect((batchCalls[0][1] as { urls: string[] }).urls).toEqual([
      'https://a.com/',
      'https://b.com/',
      'https://c.com/',
    ])
    expect(s.tabs).toHaveLength(2) // 坏位不进列表
    expect(s.tabs[0].groupId).toBe(7) // 分组存在 → 恢复
    expect(s.tabs[1].groupId).toBeNull() // 快照里本来就没分组
    // 批量走了，逐个没走
    expect(invokeMock.mock.calls.filter((c) => c[0] === 'create_tab')).toHaveLength(0)
  })

  it('批量失败回退逐个建：create_tab 按快照顺序调用', async () => {
    const s = useTabsStore()
    const groups = useGroupsStore()
    groups.groups = [{ id: 7, name: '工作', color: '#fff', collapsed: false, created_at: '' }]
    let n = 0
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'create_tabs_batch') return Promise.reject(new Error('旧版本没这命令'))
      if (cmd === 'create_tab') return Promise.resolve(makeTab(`r${++n}`))
      return Promise.resolve(undefined)
    })
    const ok = await s.restoreSession(
      session([
        { url: 'https://a.com/', title: 'A', groupId: 7 },
        { url: 'https://b.com/', title: 'B', groupId: 999 },
      ]),
    )
    expect(ok).toBe(true)
    const urls = invokeMock.mock.calls
      .filter((c) => c[0] === 'create_tab')
      .map((c) => (c[1] as { url: string }).url)
    expect(urls).toEqual(['https://a.com/', 'https://b.com/'])
    expect(s.tabs[0].groupId).toBe(7)
    expect(s.tabs[1].groupId).toBeNull()
  })

  it('空快照返回 false；坏 JSON 返回 false 且不建标签', async () => {
    const s = useTabsStore()
    expect(await s.restoreSession(session([]))).toBe(false)
    expect(await s.restoreSession({ id: 1, name: 'x', workspace_id: null, data: '{bad', created_at: '' })).toBe(false)
    expect(invokeMock).not.toHaveBeenCalledWith('create_tab', expect.anything())
  })
})

describe('saveNamedSession', () => {
  it('自动分组（负 id）不落快照；空标签列表返回 null', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a', { groupId: -1727000000000 }), makeTab('b', { groupId: 5 })]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'save_session') return Promise.resolve(42)
      return Promise.resolve(undefined)
    })
    const id = await s.saveNamedSession('工作现场')
    expect(id).toBe(42)
    const args = invokeMock.mock.calls.find((c) => c[0] === 'save_session')![1] as {
      name: string
      tabs: Array<{ url: string; groupId: number | null }>
    }
    expect(args.name).toBe('工作现场')
    expect(args.tabs[0].groupId).toBeNull() // 自动组 id 为负 → 不落库
    expect(args.tabs[1].groupId).toBe(5)

    const s2 = useTabsStore()
    s2.tabs = []
    expect(await s2.saveNamedSession('空')).toBeNull()
  })
})

describe('批量操作', () => {
  it('closeDuplicates：同 URL 只留一个，返回关闭数', async () => {
    const s = useTabsStore()
    s.tabs = [
      makeTab('a', { url: 'https://x.com/1' }),
      makeTab('b', { url: 'https://x.com/1' }),
      makeTab('c', { url: 'https://y.com/' }),
    ]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'close_tab') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
    const n = s.closeDuplicates()
    expect(n).toBe(1)
    await new Promise((r) => setTimeout(r, 0))
    expect(s.tabs.map((t) => t.id)).toEqual(['a', 'c'])
  })

  it('closeSameDomain：关闭与保留标签同域名的其他标签', async () => {
    const s = useTabsStore()
    s.tabs = [
      makeTab('a', { url: 'https://news.com/1' }),
      makeTab('b', { url: 'https://news.com/2' }),
      makeTab('c', { url: 'https://other.com/' }),
    ]
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'close_tab') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
    const n = s.closeSameDomain('a')
    expect(n).toBe(1)
    const closed = invokeMock.mock.calls
      .filter((c) => c[0] === 'close_tab')
      .map((c) => (c[1] as { id: string }).id)
    expect(closed).toEqual(['b'])
  })

  it('toggleMuteAll：有未静音则全静音（返回 true），否则全取消（返回 false）', async () => {
    const s = useTabsStore()
    s.tabs = [makeTab('a', { is_muted: false }), makeTab('b', { is_muted: true })]
    invokeMock.mockImplementation((cmd: string, args?: { id: string }) => {
      if (cmd === 'toggle_mute') {
        const t = s.tabs.find((x) => x.id === args?.id)
        return Promise.resolve({ ...t, is_muted: !t?.is_muted })
      }
      return Promise.resolve(undefined)
    })
    expect(s.toggleMuteAll()).toBe(true)
    await new Promise((r) => setTimeout(r, 0))
    expect(s.tabs.every((t) => t.is_muted)).toBe(true)
    expect(s.toggleMuteAll()).toBe(false)
    await new Promise((r) => setTimeout(r, 0))
    expect(s.tabs.every((t) => !t.is_muted)).toBe(true)
  })
})

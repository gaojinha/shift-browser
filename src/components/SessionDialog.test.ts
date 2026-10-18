// SessionDialog 组件测试（F-84 · 前端测试补课）
//
// 手法：mock @tauri-apps/api/core 与 ../utils/webview；tabs store 用真实 pinia
// 实例后 spy 其动作（组件与测试共享同一 store 实例）；toast 走真实 store 断言
// items。注意（F-88 同款坑）：整个弹窗正文在外层 UiDialog 的 reka portal 里，
// DOM 挂 document.body——查询/触发一律走 document（DOMWrapper 包装）。
//
// 跑法：npx vitest run src/components/SessionDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import SessionDialog from './SessionDialog.vue'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { setWebviewVisible } from '../utils/webview'
import type { Session } from '../types/session'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../utils/webview', () => ({ setWebviewVisible: vi.fn().mockResolvedValue(undefined) }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)
const webviewMock = vi.mocked(setWebviewVisible)

/** 本地时间构造 ISO（display 用本地时区回读，不受 CI 时区影响） */
function localIso(y: number, m: number, d: number, hh: number, mm: number): string {
  return new Date(y, m - 1, d, hh, mm).toISOString()
}

function mkSession(over: Partial<Session> = {}): Session {
  return {
    id: 1,
    name: '工作会话',
    workspace_id: null,
    data: JSON.stringify([{ url: 'https://a.com', title: 'A' }, { url: 'https://b.com', title: 'B' }]),
    created_at: localIso(2026, 10, 18, 9, 5),
    ...over,
  }
}

/** portal 在 document 上，包装成 DOMWrapper 才能 setValue/trigger */
function q(sel: string): DOMWrapper<Element> {
  const el = document.querySelector(sel)
  expect(el, `document 上找不到 ${sel}`).toBeTruthy()
  return new DOMWrapper(el!)
}
function qa(sel: string): DOMWrapper<Element>[] {
  return Array.from(document.querySelectorAll(sel)).map((el) => new DOMWrapper(el))
}

async function openDialog() {
  const w = mount(SessionDialog, { props: { show: false }, attachTo: document.body })
  await w.setProps({ show: true })
  await flushPromises()
  await flushPromises()
  return w
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  webviewMock.mockClear()
  invokeMock.mockResolvedValue([])
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('SessionDialog · 打开与列表', () => {
  it('打开时拉取 list_sessions，行渲染名称/标签数/格式化时间；webview 隐藏', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    await openDialog()
    expect(invokeMock).toHaveBeenCalledWith('list_sessions')
    expect(q('.ss-name').text()).toBe('工作会话')
    expect(q('.ss-meta').text()).toContain('2 个标签')
    expect(q('.ss-meta').text()).toContain('2026-10-18 09:05')
    expect(webviewMock).toHaveBeenCalledWith(false, 'session-dialog')
  })

  it('空列表 → 空态文案「还没有保存的会话」', async () => {
    await openDialog()
    expect(q('.ss-empty').text()).toBe('还没有保存的会话')
  })

  it('list_sessions 拒绝 → toast error「加载会话失败」', async () => {
    invokeMock.mockRejectedValue(new Error('boom'))
    const store = useToastStore()
    await openDialog()
    expect(store.items.some((t) => t.type === 'error' && t.message === '加载会话失败')).toBe(true)
  })

  it('关闭时重置输入（重开后输入框为空）并恢复 webview 可见', async () => {
    const w = await openDialog()
    await q('.ss-input').setValue('临时名')
    await w.setProps({ show: false })
    await flushPromises()
    expect(webviewMock).toHaveBeenLastCalledWith(true, 'session-dialog')
    // 关闭重置：重开后输入框不带旧值（portal DOM 已撤下，须重开验证）
    await w.setProps({ show: true })
    await flushPromises()
    expect((q('.ss-input').element as HTMLInputElement).value).toBe('')
  })

  it('data 快照损坏 → 标签数显示 0 而不崩', async () => {
    invokeMock.mockResolvedValue([mkSession({ data: 'not-json{' })])
    await openDialog()
    expect(q('.ss-meta').text()).toContain('0 个标签')
  })

  it('created_at 非法 → 原样回显不崩', async () => {
    invokeMock.mockResolvedValue([mkSession({ created_at: 'bad-date' })])
    await openDialog()
    expect(q('.ss-meta').text()).toContain('bad-date')
  })
})

describe('SessionDialog · 恢复', () => {
  it('恢复成功：先 clearAllTabs 再 restoreSession，success toast + 关闭', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'https://x.com', title: 'X' }] as never
    const clearSpy = vi.spyOn(tabs, 'clearAllTabs').mockResolvedValue([])
    const restoreSpy = vi.spyOn(tabs, 'restoreSession').mockResolvedValue(true)
    const w = await openDialog()
    await qa('.op-btn')[0]!.trigger('click')
    await flushPromises()
    expect(clearSpy).toHaveBeenCalledTimes(1)
    expect(restoreSpy).toHaveBeenCalledTimes(1)
    const toast = useToastStore()
    expect(toast.items.some((t) => t.type === 'success' && t.message.includes('工作会话'))).toBe(true)
    expect(w.emitted('close')).toBeTruthy()
  })

  it('restoreSession 返回 false（空/损坏快照）→ error toast，不关闭', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    const tabs = useTabsStore()
    vi.spyOn(tabs, 'clearAllTabs').mockResolvedValue([])
    vi.spyOn(tabs, 'restoreSession').mockResolvedValue(false)
    const w = await openDialog()
    await qa('.op-btn')[0]!.trigger('click')
    await flushPromises()
    const toast = useToastStore()
    expect(toast.items.some((t) => t.message === '会话内容为空或已损坏')).toBe(true)
    expect(w.emitted('close')).toBeFalsy()
  })

  it('恢复链路抛错 → error toast「恢复会话失败」', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'https://x.com', title: 'X' }] as never
    vi.spyOn(tabs, 'clearAllTabs').mockRejectedValue(new Error('x'))
    await openDialog()
    await qa('.op-btn')[0]!.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '恢复会话失败')).toBe(true)
  })

  it('当前无标签页时恢复不调 clearAllTabs（直接开）', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    const tabs = useTabsStore()
    const clearSpy = vi.spyOn(tabs, 'clearAllTabs').mockResolvedValue([])
    vi.spyOn(tabs, 'restoreSession').mockResolvedValue(true)
    await openDialog()
    await qa('.op-btn')[0]!.trigger('click')
    await flushPromises()
    expect(clearSpy).not.toHaveBeenCalled()
  })
})

describe('SessionDialog · 保存命名会话', () => {
  async function typeAndSave(name: string) {
    await q('.ss-input').setValue(name)
    await q('.ss-btn.accent').trigger('click')
    await flushPromises()
  }

  it('空名 / 纯空格 → error「请输入会话名称」，不调 saveNamedSession', async () => {
    const tabs = useTabsStore()
    const saveSpy = vi.spyOn(tabs, 'saveNamedSession').mockResolvedValue(9)
    await openDialog()
    await typeAndSave('   ')
    expect(useToastStore().items.some((t) => t.message === '请输入会话名称')).toBe(true)
    expect(saveSpy).not.toHaveBeenCalled()
  })

  it('无标签页 → error「当前没有标签页可保存」', async () => {
    const tabs = useTabsStore()
    tabs.tabs = []
    const saveSpy = vi.spyOn(tabs, 'saveNamedSession').mockResolvedValue(9)
    await openDialog()
    await typeAndSave('合法名')
    expect(useToastStore().items.some((t) => t.message === '当前没有标签页可保存')).toBe(true)
    expect(saveSpy).not.toHaveBeenCalled()
  })

  it('保存成功：输入清空、列表重拉、success toast', async () => {
    invokeMock.mockResolvedValueOnce([]).mockResolvedValueOnce([mkSession({ name: '新会话' })])
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'https://x.com', title: 'X' }] as never
    vi.spyOn(tabs, 'saveNamedSession').mockResolvedValue(7)
    await openDialog()
    await typeAndSave('新会话')
    expect(useToastStore().items.some((t) => t.type === 'success' && t.message.includes('新会话'))).toBe(true)
    expect((q('.ss-input').element as HTMLInputElement).value).toBe('')
    expect(q('.ss-name').text()).toBe('新会话')
  })

  it('saveNamedSession 返回 null → error「保存会话失败」，输入保留', async () => {
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'https://x.com', title: 'X' }] as never
    vi.spyOn(tabs, 'saveNamedSession').mockResolvedValue(null)
    await openDialog()
    await typeAndSave('会失败')
    expect(useToastStore().items.some((t) => t.message === '保存会话失败')).toBe(true)
    expect((q('.ss-input').element as HTMLInputElement).value).toBe('会失败')
  })

  it('输入框回车触发保存（同按钮路径）', async () => {
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'https://x.com', title: 'X' }] as never
    const saveSpy = vi.spyOn(tabs, 'saveNamedSession').mockResolvedValue(3)
    await openDialog()
    await q('.ss-input').setValue('回车保存')
    await q('.ss-input').trigger('keydown.enter')
    await flushPromises()
    expect(saveSpy).toHaveBeenCalledWith('回车保存')
  })
})

describe('SessionDialog · 删除（内层确认弹窗）', () => {
  it('点 ✕ 弹确认框：描述含会话名；外层关闭后未确认即不删', async () => {
    invokeMock.mockResolvedValue([mkSession()])
    const w = await openDialog()
    await q('.op-btn.danger').trigger('click')
    await flushPromises()
    const desc = Array.from(document.querySelectorAll('.ui-dialog-desc')).map((n) => n.textContent ?? '').join()
    expect(desc).toContain('工作会话')
    await w.setProps({ show: false })
    expect(invokeMock).not.toHaveBeenCalledWith('delete_session', expect.anything())
  })

  it('确认删除：delete_session 调用、行移除、success toast', async () => {
    invokeMock.mockResolvedValue([mkSession(), mkSession({ id: 2, name: '第二会话' })])
    await openDialog()
    expect(qa('.ss-row')).toHaveLength(2)
    await q('.op-btn.danger').trigger('click')
    await flushPromises()
    const okBtn = qa('.confirm-btn.ok').pop()!
    await okBtn.trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('delete_session', { id: 1 })
    expect(qa('.ss-row')).toHaveLength(1)
    expect(useToastStore().items.some((t) => t.message === '会话已删除')).toBe(true)
  })

  it('删除失败 → error toast「删除会话失败」，行保留', async () => {
    invokeMock.mockResolvedValueOnce([mkSession()]).mockRejectedValueOnce(new Error('db')) // list → delete
    await openDialog()
    await q('.op-btn.danger').trigger('click')
    await flushPromises()
    const okBtn = qa('.confirm-btn.ok').pop()!
    await okBtn.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '删除会话失败')).toBe(true)
    expect(qa('.ss-row')).toHaveLength(1)
  })
})

// DownloadsDialog 组件测试（F-85 · 前端测试补课）
//
// 手法：mock event/plugin-opener/webview；downloads 与 tabs store 用真实
// pinia 后 spy 动作；formatBytes/stateInfo/二次删除确认经 UI 断言。弹窗正文
// 在 UiDialog 的 reka portal 里挂 document.body——查询/触发一律走
// DOMWrapper(document)（F-88/F-84 同款坑）。
//
// 跑法：npx vitest run src/components/DownloadsDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import DownloadsDialog from './DownloadsDialog.vue'
import { useDownloadsStore } from '../stores/downloads'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { setWebviewVisible } from '../utils/webview'
import { listen } from '@tauri-apps/api/event'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import type { Download } from '../types/download'

vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }))
vi.mock('@tauri-apps/plugin-opener', () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
  revealItemInDir: vi.fn().mockResolvedValue(undefined),
}))
vi.mock('../utils/webview', () => ({ setWebviewVisible: vi.fn().mockResolvedValue(undefined) }))

const listenMock = vi.mocked(listen)
const webviewMock = vi.mocked(setWebviewVisible)
const openPathMock = vi.mocked(openPath)
const revealMock = vi.mocked(revealItemInDir)

function mkDownload(over: Partial<Download> = {}): Download {
  return {
    id: 1,
    filename: 'setup.tar.gz',
    url: 'https://example.com/setup.tar.gz',
    path: '/home/gao/Downloads/setup.tar.gz',
    total_bytes: 1024 * 1024,
    received_bytes: 512 * 1024,
    state: 'completed',
    created_at: '2026-10-18T09:00:00',
    ...over,
  }
}

function q(sel: string): DOMWrapper<Element> {
  const el = document.querySelector(sel)
  expect(el, `document 上找不到 ${sel}`).toBeTruthy()
  return new DOMWrapper(el!)
}
function qa(sel: string): DOMWrapper<Element>[] {
  return Array.from(document.querySelectorAll(sel)).map((el) => new DOMWrapper(el))
}

async function openDialog() {
  const w = mount(DownloadsDialog, { props: { show: false }, attachTo: document.body })
  await w.setProps({ show: true })
  await flushPromises()
  await flushPromises()
  return w
}

beforeEach(() => {
  setActivePinia(createPinia())
  webviewMock.mockClear()
  openPathMock.mockClear()
  revealMock.mockClear()
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('DownloadsDialog · 打开与列表', () => {
  it('打开时 loadDownloads + webview 隐藏；关闭恢复', async () => {
    const store = useDownloadsStore()
    const spy = vi.spyOn(store, 'loadDownloads').mockResolvedValue(undefined)
    const w = await openDialog()
    expect(spy).toHaveBeenCalledTimes(1)
    expect(webviewMock).toHaveBeenCalledWith(false, 'downloads-dialog')
    await w.setProps({ show: false })
    await flushPromises()
    expect(webviewMock).toHaveBeenLastCalledWith(true, 'downloads-dialog')
  })

  it('空列表 → 「还没有下载记录」；加载失败 → toast', async () => {
    await openDialog()
    expect(q('.empty').text()).toBe('还没有下载记录')
    const store = useDownloadsStore()
    vi.spyOn(store, 'loadDownloads').mockRejectedValue(new Error('db'))
    const w = mount(DownloadsDialog, { props: { show: false }, attachTo: document.body })
    await w.setProps({ show: true })
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '加载下载记录失败')).toBe(true)
  })

  it('行渲染：文件名/URL/进度文本（B→KB→MB）+ 进度条宽度', async () => {
    const store = useDownloadsStore()
    store.list = [
      mkDownload(),
      mkDownload({ id: 2, filename: 'tiny.bin', total_bytes: 0, received_bytes: 0, state: 'in_progress', path: null }),
      mkDownload({ id: 3, filename: 'small.bin', total_bytes: 900, received_bytes: 450, state: 'in_progress', path: null }),
      mkDownload({ id: 4, filename: 'kb.bin', total_bytes: 2048, received_bytes: 1024, state: 'in_progress', path: null }),
    ]
    await openDialog()
    expect(q('.item-name').text()).toBe('setup.tar.gz')
    expect(q('.item-url').text()).toBe('https://example.com/setup.tar.gz')
    const texts = qa('.progress-text').map((n) => n.text())
    expect(texts[0]).toContain('512.0 KB')
    expect(texts[0]).toContain('1.0 MB')
    expect(texts[0]).toContain('(50%)')
    expect(texts[1]).toContain('0 B / 0 B (0%)') // total=0 → 0%
    expect(texts[2]).toContain('450 B')
    expect(texts[3]).toContain('1.0 KB / 2.0 KB (50%)')
    const fill = q('.progress-fill')
    expect(fill.attributes('style')).toContain('width: 50%')
  })

  it('状态文案与配色类：completed/failed/cancelled/interrupted/默认下载中', async () => {
    const store = useDownloadsStore()
    store.list = [
      mkDownload(),
      mkDownload({ id: 2, state: 'failed', path: null }),
      mkDownload({ id: 3, state: 'cancelled', path: null }),
      mkDownload({ id: 4, state: 'interrupted', path: null }),
      mkDownload({ id: 5, state: 'in_progress', path: null }),
    ]
    await openDialog()
    const states = qa('.state')
    expect(states[0]!.text()).toBe('已完成')
    expect(states[0]!.classes()).toContain('state-done')
    expect(states[1]!.text()).toBe('失败')
    expect(states[1]!.classes()).toContain('state-fail')
    expect(states[2]!.text()).toBe('已取消')
    expect(states[2]!.classes()).toContain('state-cancel')
    expect(states[3]!.text()).toBe('已中断（重启）')
    expect(states[3]!.classes()).toContain('state-fail')
    expect(states[4]!.text()).toBe('下载中')
    expect(states[4]!.classes()).toContain('state-run')
  })
})

describe('DownloadsDialog · 打开 / 文件夹 / 重新下载', () => {
  it('completed 且有 path：显示「打开」「文件夹」，点击调 opener', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload()]
    await openDialog()
    const btns = qa('.item-actions .mini-btn')
    await btns[0]!.trigger('click')
    await flushPromises()
    expect(openPathMock).toHaveBeenCalledWith('/home/gao/Downloads/setup.tar.gz')
    await btns[1]!.trigger('click')
    await flushPromises()
    expect(revealMock).toHaveBeenCalledWith('/home/gao/Downloads/setup.tar.gz')
  })

  it('completed 但无 path：不显示打开/文件夹按钮', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload({ path: null })]
    await openDialog()
    expect(qa('.item-actions .mini-btn')).toHaveLength(1) // 只剩删除
  })

  it('failed 且有 url：显示「重新下载」→ createTab(url) + 关闭', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload({ state: 'failed', path: null })]
    const tabs = useTabsStore()
    const spy = vi.spyOn(tabs, 'createTab').mockResolvedValue(1 as never)
    const w = await openDialog()
    await q('.mini-btn').trigger('click') // 唯一的 mini-btn 是重新下载
    await flushPromises()
    expect(spy).toHaveBeenCalledWith('https://example.com/setup.tar.gz')
    expect(w.emitted('close')).toBeTruthy()
  })

  it('failed 无 url：不显示重新下载', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload({ state: 'failed', url: null, path: null })]
    await openDialog()
    const btns = qa('.item-actions .mini-btn')
    expect(btns).toHaveLength(1)
    expect(btns[0]!.classes()).toContain('danger') // 只剩删除
  })

  it('createTab 抛错 → toast「重新下载失败」不崩', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload({ state: 'interrupted', path: null })]
    const tabs = useTabsStore()
    vi.spyOn(tabs, 'createTab').mockRejectedValue(new Error('x'))
    await openDialog()
    await q('.mini-btn').trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '重新下载失败')).toBe(true)
  })
})

describe('DownloadsDialog · 删除（二次点击确认）', () => {
  it('第一次点进入待确认态（按钮变「确认删除」）不删；再点真删', async () => {
    const store = useDownloadsStore()
    store.list = [mkDownload()]
    const spy = vi.spyOn(store, 'deleteDownload').mockResolvedValue(undefined)
    await openDialog()
    const btn = q('.mini-btn.danger')
    await btn.trigger('click')
    await flushPromises()
    expect(spy).not.toHaveBeenCalled()
    expect(q('.mini-btn.danger').text()).toBe('确认删除')
    await q('.mini-btn.danger').trigger('click')
    await flushPromises()
    expect(spy).toHaveBeenCalledWith(1)
  })

  it('点别的行的删除会切换待确认目标；超时自动撤防（3s）', async () => {
    vi.useFakeTimers()
    try {
      const store = useDownloadsStore()
      store.list = [mkDownload(), mkDownload({ id: 2, filename: 'b.bin', path: null, state: 'failed' })]
      vi.spyOn(store, 'deleteDownload').mockResolvedValue(undefined)
      await openDialog()
      await qa('.mini-btn.danger')[0]!.trigger('click')
      expect(qa('.mini-btn.danger')[0]!.text()).toBe('确认删除')
      // 切到另一行 → 第一行撤防
      await qa('.mini-btn.danger')[1]!.trigger('click')
      expect(qa('.mini-btn.danger')[0]!.text()).toBe('删除')
      expect(qa('.mini-btn.danger')[1]!.text()).toBe('确认删除')
      // 超时自动撤防
      vi.advanceTimersByTime(3000)
      await flushPromises()
      expect(qa('.mini-btn.danger')[1]!.text()).toBe('删除')
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('DownloadsDialog · 事件接线', () => {
  it('挂载即订阅 DOWNLOADS_UPDATED；弹窗开着收到事件 → 重拉', async () => {
    const store = useDownloadsStore()
    const spy = vi.spyOn(store, 'loadDownloads').mockResolvedValue(undefined)
    const w = await openDialog()
    spy.mockClear()
    const handler = listenMock.mock.calls[0]![1] as () => void
    handler() // 弹窗开着
    await flushPromises()
    expect(spy).toHaveBeenCalledTimes(1)
    await w.setProps({ show: false })
    await flushPromises()
    spy.mockClear()
    handler() // 弹窗关着 → 不刷
    await flushPromises()
    expect(spy).not.toHaveBeenCalled()
  })

  it('卸载时解绑事件监听', async () => {
    const unlisten = vi.fn()
    listenMock.mockResolvedValueOnce(unlisten)
    const w = mount(DownloadsDialog, { props: { show: false }, attachTo: document.body })
    await flushPromises()
    w.unmount()
    expect(unlisten).toHaveBeenCalledTimes(1)
  })
})

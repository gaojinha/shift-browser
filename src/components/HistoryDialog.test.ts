// HistoryDialog 组件测试（F-83，王二狗）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store + mock Tauri IPC + UiDialog 打桩
//（真 reka-ui 底座在 jsdom 下 Portal 不落地，插槽内容不渲染）。
//
// 关键：组件 `watch(() => props.show)` **无 immediate** —— 挂载时 show=true 不触发加载，
// 必须先挂 show=false 再 `setProps({ show: true })` 才走 loadHistory 链路。
//
// 覆盖：
//  1.  显隐（show=false 不渲染 → 翻 true 渲染弹窗）
//  2.  打开即加载：get_recent_history {limit:200} + 日期分组渲染（今天/昨天/更早 + 计数）
//  3.  条目渲染：title（null 时回退 host）、url、访问次数、格式化时间
//  4.  空态：无历史 →「还没有浏览历史」；带搜索词无结果 →「没有匹配的历史记录」
//  5.  搜索：输入防抖 300ms 后调 search_history {query,limit:200}
//  6.  加载中：请求未落定显示「加载中…」
//  7.  点击条目 → tabsStore.createTab(url)
//  8.  删除单条：stopPropagation（不触发打开）+ delete_history_entry + 本地过滤；失败 toast
//  9.  清空流：按钮 disabled 条件 → 确认弹窗显隐 → 取消不调 → 确认调 clear_history + 空列表 + toast
//  10. 分组折叠：点组头切换 aria-expanded 与 body 显隐
//
// 跑法：npx vitest run src/components/HistoryDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { HistoryEntry } from '../types/history'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import HistoryDialog from './HistoryDialog.vue'

/** UiDialog stub：open 时渲染默认槽 + 页脚槽（清空确认用 footer），带 data-title 便于区分 */
const UiDialogStub = {
  name: 'UiDialog',
  props: {
    open: { type: Boolean, default: false },
    title: String,
    description: String,
    width: String,
  },
  emits: ['update:open'],
  template: `
    <div v-if="open" class="ui-dialog-stub" :data-title="title">
      <p v-if="description" class="ui-dialog-desc">{{ description }}</p>
      <div class="ui-dialog-body"><slot /></div>
      <div class="ui-dialog-footer"><slot name="footer" /></div>
    </div>
  `,
}

/** 本地正午的时间戳（避开时区/跨日边界，保证分组稳定落在今天/昨天） */
function localNoon(daysAgo: number): string {
  const d = new Date()
  d.setDate(d.getDate() - daysAgo)
  d.setHours(12, 0, 0, 0)
  return d.toISOString()
}

function entry(id: number, over: Partial<HistoryEntry> = {}): HistoryEntry {
  return {
    id,
    url: `https://site${id}.com/page`,
    title: `页面 ${id}`,
    visit_count: id,
    last_visit_at: localNoon(0),
    ...over,
  }
}

const TODAY = entry(1, { visit_count: 3 })
const YESTERDAY = entry(2, { last_visit_at: localNoon(1) })
const OLDER = entry(3, { last_visit_at: '2020-01-01T12:00:00.000Z' })
const ENTRIES = [TODAY, YESTERDAY, OLDER]

let pinia: Pinia
let wrapper: VueWrapper | null = null

function stubInvoke(recent: HistoryEntry[] = ENTRIES, search: HistoryEntry[] = ENTRIES) {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_recent_history') return Promise.resolve(recent)
    if (cmd === 'search_history') return Promise.resolve(search)
    return Promise.resolve(undefined)
  })
}

/**
 * 挂载弹窗：先 show=false 挂起，再 setProps({show:true}) 触发 watch（无 immediate）。
 * show=false 用例只挂载不翻开。
 */
async function mountDialog(show = true, recent: HistoryEntry[] = ENTRIES) {
  stubInvoke(recent)
  wrapper = mount(HistoryDialog, {
    props: { show: false },
    global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
    attachTo: document.body,
  })
  await flushPromises()
  if (show) {
    wrapper.setProps({ show: true })
    await flushPromises()
  }
  return wrapper
}

const mainDialog = (w: VueWrapper) => w.find('.ui-dialog-stub[data-title="浏览历史"]')
const confirmDialog = (w: VueWrapper) => w.find('.ui-dialog-stub[data-title="清空历史"]')
const items = (w: VueWrapper) => w.findAll('.history-item')

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
})

describe('HistoryDialog：显隐与加载', () => {
  it('show=false 不渲染；翻 true 渲染弹窗并调 get_recent_history {limit:200}', async () => {
    const w = await mountDialog(false)
    expect(mainDialog(w).exists()).toBe(false)
    expect(invokeMock).not.toHaveBeenCalledWith('get_recent_history', expect.anything())

    w.setProps({ show: true })
    await flushPromises()
    expect(mainDialog(w).exists()).toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('get_recent_history', { limit: 200 })
  })

  it('按日期分组渲染：今天/昨天/更早 + 组内计数', async () => {
    const w = await mountDialog(true)
    const headers = w.findAll('.history-group-header')
    expect(headers.map((h) => h.find('.history-group-label').text())).toEqual([
      '今天',
      '昨天',
      '更早',
    ])
    expect(headers.map((h) => h.find('.history-group-count').text())).toEqual(['1', '1', '1'])
    expect(items(w).length).toBe(3)
  })

  it('条目渲染：标题 / URL / 访问次数 / 格式化时间；title 为空回退 host', async () => {
    const noTitle = entry(9, { title: null, url: 'https://fallback.example.com/x', visit_count: 7 })
    const w = await mountDialog(true, [noTitle])
    const it0 = items(w)[0]
    // title 为 null → 回退 host（hostOf 取 host，不含协议）
    expect(it0.find('.history-title').text()).toBe('fallback.example.com')
    expect(it0.find('.history-url').text()).toBe('https://fallback.example.com/x')
    expect(it0.find('.history-count').text()).toBe('7 次')
    // 本地正午 → 格式化时间形如 YYYY-MM-DD HH:MM
    expect(it0.find('.history-time').text()).toMatch(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/)
  })

  it('空态：无历史显示「还没有浏览历史」', async () => {
    const w = await mountDialog(true, [])
    expect(w.find('.empty').text()).toBe('还没有浏览历史')
    expect(items(w).length).toBe(0)
  })

  it('加载中显示「加载中…」（请求未落定）', async () => {
    let release!: () => void
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_recent_history') {
        return new Promise<HistoryEntry[]>((res) => {
          release = () => res([])
        })
      }
      return Promise.resolve(undefined)
    })
    wrapper = mount(HistoryDialog, {
      props: { show: false },
      global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
      attachTo: document.body,
    })
    await flushPromises()
    wrapper.setProps({ show: true })
    await flushPromises()
    expect(wrapper.find('.loading-hint').exists()).toBe(true)
    release()
    await flushPromises()
    expect(wrapper.find('.loading-hint').exists()).toBe(false)
  })

  it('加载失败 → toast 报错「加载历史失败」', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_recent_history') return Promise.reject(new Error('db down'))
      return Promise.resolve(undefined)
    })
    const toast = useToastStore(pinia)
    const errSpy = vi.spyOn(toast, 'error')

    wrapper = mount(HistoryDialog, {
      props: { show: false },
      global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
      attachTo: document.body,
    })
    await flushPromises()
    wrapper.setProps({ show: true })
    await flushPromises()
    expect(errSpy).toHaveBeenCalledWith('加载历史失败')
  })
})

describe('HistoryDialog：搜索', () => {
  it('输入防抖 300ms 后调 search_history {query,limit:200}', async () => {
    const w = await mountDialog(true)
    invokeMock.mockClear()
    stubInvoke(ENTRIES, [entry(5, { title: '命中', url: 'https://hit.example.com' })])

    await w.find('input.history-search').setValue('rust')
    // 防抖未到：还没发搜索
    expect(invokeMock).not.toHaveBeenCalledWith('search_history', expect.anything())

    await new Promise((r) => setTimeout(r, 350))
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('search_history', { query: 'rust', limit: 200 })
    expect(items(w)[0].find('.history-title').text()).toBe('命中')
  })

  it('带搜索词且无结果 →「没有匹配的历史记录」', async () => {
    const w = await mountDialog(true)
    stubInvoke(ENTRIES, [])
    await w.find('input.history-search').setValue('zzz')
    await new Promise((r) => setTimeout(r, 350))
    await flushPromises()
    expect(w.find('.empty').text()).toBe('没有匹配的历史记录')
  })
})

describe('HistoryDialog：操作', () => {
  it('点击条目 → tabsStore.createTab(url)', async () => {
    const w = await mountDialog(true)
    const tabs = useTabsStore(pinia)
    const spy = vi.spyOn(tabs, 'createTab').mockResolvedValue(undefined as never)

    await items(w)[0].find('.history-main').trigger('click')
    expect(spy).toHaveBeenCalledWith(TODAY.url)
  })

  it('删除单条：stopPropagation + delete_history_entry + 本地过滤，且不触发打开', async () => {
    const w = await mountDialog(true)
    const tabs = useTabsStore(pinia)
    const openSpy = vi.spyOn(tabs, 'createTab').mockResolvedValue(undefined as never)

    await items(w)[0].find('.history-del').trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('delete_history_entry', { id: TODAY.id })
    expect(items(w).length).toBe(2)
    // stopPropagation：删除不应触发行打开
    expect(openSpy).not.toHaveBeenCalled()
  })

  it('删除失败 → toast「删除历史失败」、列表不动', async () => {
    const w = await mountDialog(true)
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'delete_history_entry') return Promise.reject(new Error('boom'))
      return Promise.resolve(undefined)
    })
    const toast = useToastStore(pinia)
    const errSpy = vi.spyOn(toast, 'error')

    await items(w)[0].find('.history-del').trigger('click')
    await flushPromises()
    expect(errSpy).toHaveBeenCalledWith('删除历史失败')
    expect(items(w).length).toBe(3)
  })

  it('清空按钮：列表非空可用，空列表禁用', async () => {
    const w = await mountDialog(true)
    expect(w.find('.clear-btn').attributes('disabled')).toBeUndefined()

    const w2 = await mountDialog(true, [])
    expect(w2.find('.clear-btn').attributes('disabled')).toBeDefined()
  })

  it('清空流：点清空 → 确认弹窗；取消 → 不调 clear_history 且列表不动', async () => {
    const w = await mountDialog(true)
    await w.find('.clear-btn').trigger('click')
    await flushPromises()

    expect(confirmDialog(w).exists()).toBe(true)
    expect(confirmDialog(w).text()).toContain('确定清空全部浏览历史？')

    await confirmDialog(w).find('.confirm-btn.cancel').trigger('click')
    await flushPromises()
    expect(confirmDialog(w).exists()).toBe(false)
    expect(invokeMock).not.toHaveBeenCalledWith('clear_history')
    expect(items(w).length).toBe(3)
  })

  it('清空流：确认 → clear_history + 列表清空 + toast「历史已清空」', async () => {
    const w = await mountDialog(true)
    const toast = useToastStore(pinia)
    const okSpy = vi.spyOn(toast, 'success')

    await w.find('.clear-btn').trigger('click')
    await flushPromises()
    await confirmDialog(w).find('.confirm-btn.ok').trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('clear_history')
    expect(okSpy).toHaveBeenCalledWith('历史已清空')
    expect(confirmDialog(w).exists()).toBe(false)
    // 列表清空 → 空态出现
    expect(w.find('.empty').text()).toBe('还没有浏览历史')
  })

  it('清空失败 → toast「清空历史失败」', async () => {
    const w = await mountDialog(true)
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'clear_history') return Promise.reject(new Error('boom'))
      return Promise.resolve(undefined)
    })
    const toast = useToastStore(pinia)
    const errSpy = vi.spyOn(toast, 'error')

    await w.find('.clear-btn').trigger('click')
    await flushPromises()
    await confirmDialog(w).find('.confirm-btn.ok').trigger('click')
    await flushPromises()
    expect(errSpy).toHaveBeenCalledWith('清空历史失败')
  })
})

describe('HistoryDialog：分组折叠', () => {
  it('点组头切换 aria-expanded 与组体显隐', async () => {
    const w = await mountDialog(true)
    const header = w.findAll('.history-group-header')[0]
    expect(header.attributes('aria-expanded')).toBe('true')

    await header.trigger('click')
    expect(w.findAll('.history-group-header')[0].attributes('aria-expanded')).toBe('false')
    // 组体 v-show=false → style.display none
    const body = w.find('.history-group-body')
    expect(body.attributes('style')).toContain('display: none')

    await w.findAll('.history-group-header')[0].trigger('click')
    expect(w.findAll('.history-group-header')[0].attributes('aria-expanded')).toBe('true')
  })
})

// AddressBar 组件测试（R4-2 组件测试补课 · 2026-10-14）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子）+
// mock Tauri IPC。地址解析走真实 resolveInputTarget（utils/address.test.ts
// 已单测），这里只测"组件接线"：回显、锁图标、回车分流、导航按钮、
// 建议下拉（防抖 + 键盘导航 + 直达项）。
//
// 跑法：npx vitest run src/components/AddressBar.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { nextTick } from 'vue'
import type { Tab } from '../types/tab'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('../utils/webview', () => ({
  // AddressBar 对返回值 .catch()（watch 里），mock 必须返回 promise
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

import { useTabsStore } from '../stores/tabs'
import { i18n } from '../i18n'
import AddressBar from './AddressBar.vue'

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

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
})
afterEach(() => vi.useRealTimers())

function mountWith(tabs: Tab[], activeTabId: string | null, patch: Record<string, unknown> = {}) {
  const tabsStore = useTabsStore()
  tabsStore.$patch({ tabs, activeTabId, ...patch })
  // 子组件 TranslateProgress 用 useI18n()，必须装 i18n 插件
  const wrapper = mount(AddressBar, { global: { plugins: [pinia, i18n] } })
  return { wrapper, tabsStore }
}

const addrInput = (w: ReturnType<typeof mount>) =>
  w.find('input[aria-label="地址栏"]')

describe('AddressBar：地址回显与安全标识', () => {
  it('挂载即回显活动标签地址（watch immediate）', () => {
    const { wrapper } = mountWith([makeTab('a', { url: 'https://example.com/' })], 'a')
    expect((addrInput(wrapper).element as HTMLInputElement).value).toBe('https://example.com/')
  })

  it('https 地址显示锁图标；http 不显示', () => {
    const { wrapper } = mountWith([makeTab('a', { url: 'https://sec.com/' })], 'a')
    expect(wrapper.find('.lock-icon').exists()).toBe(true)
    const w2 = mountWith([makeTab('a', { url: 'http://plain.com/' })], 'a').wrapper
    expect(w2.find('.lock-icon').exists()).toBe(false)
  })

  it('点锁图标 → emit openCerts', async () => {
    const { wrapper } = mountWith([makeTab('a', { url: 'https://sec.com/' })], 'a')
    await wrapper.find('.lock-icon').trigger('click')
    expect(wrapper.emitted('openCerts')).toBeTruthy()
  })
})

describe('AddressBar：回车导航分流', () => {
  it('显式协议 URL → navigateTab 原样直达', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'navigateTab').mockResolvedValue(undefined)
    await addrInput(wrapper).setValue('https://example.com')
    await addrInput(wrapper).trigger('keydown', { key: 'Enter' })
    expect(spy).toHaveBeenCalledWith('a', 'https://example.com')
  })

  it('带空格搜索词 → 默认引擎 bing 分流', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'navigateTab').mockResolvedValue(undefined)
    await addrInput(wrapper).setValue('hello world')
    await addrInput(wrapper).trigger('keydown', { key: 'Enter' })
    expect(spy).toHaveBeenCalledWith('a', 'https://www.bing.com/search?q=hello%20world')
  })

  it('空输入回车不导航', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'navigateTab').mockResolvedValue(undefined)
    await addrInput(wrapper).setValue('   ')
    await addrInput(wrapper).trigger('keydown', { key: 'Enter' })
    expect(spy).not.toHaveBeenCalled()
  })

  it('无活动标签回车不导航', async () => {
    const { wrapper, tabsStore } = mountWith([], null)
    const spy = vi.spyOn(tabsStore, 'navigateTab').mockResolvedValue(undefined)
    await addrInput(wrapper).setValue('https://example.com')
    await addrInput(wrapper).trigger('keydown', { key: 'Enter' })
    expect(spy).not.toHaveBeenCalled()
  })
})

describe('AddressBar：导航按钮', () => {
  it('刷新按钮 → reloadTab(active.id)', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'reloadTab').mockResolvedValue(undefined)
    await wrapper.find('.nav-btn[title="刷新"]').trigger('click')
    expect(spy).toHaveBeenCalledWith('a')
  })

  it('后退按钮：canGoBack=false 禁用；true 时点击 → goBack', async () => {
    const w1 = mountWith([makeTab('a')], 'a').wrapper
    expect(w1.find('.nav-btn[title="后退"]').attributes('disabled')).toBeDefined()
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a', { canGoBack: true })
    const spy = vi.spyOn(tabsStore, 'goBack').mockResolvedValue(undefined)
    await wrapper.find('.nav-btn[title="后退"]').trigger('click')
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('主页按钮：默认 homepage=newtab → createTab()（无标签也不哑火）', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'createTab').mockResolvedValue(null)
    await wrapper.find('.nav-btn[title="主页"]').trigger('click')
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy).toHaveBeenCalledWith()
  })
})

describe('AddressBar：建议下拉', () => {
  it('输入防抖拉建议：直达项置首 + 键盘选择 + Enter 打开', async () => {
    vi.useFakeTimers()
    invokeMock.mockResolvedValue([]) // search_bookmarks / search_history 均空
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'navigateTab').mockResolvedValue(undefined)
    await addrInput(wrapper).setValue('https://example.com')
    await addrInput(wrapper).trigger('input')
    await vi.advanceTimersByTimeAsync(200)
    await nextTick()
    const items = wrapper.findAll('.suggest-item')
    expect(items.length).toBe(1)
    expect(items[0].text()).toContain('打开')
    // ↓ 选中直达项
    await addrInput(wrapper).trigger('keydown', { key: 'ArrowDown' })
    await nextTick()
    expect(items[0].classes()).toContain('selected')
    // Enter 打开选中项（直达 = 输入原样）
    await addrInput(wrapper).trigger('keydown', { key: 'Enter' })
    expect(spy).toHaveBeenCalledWith('a', 'https://example.com')
    // 打开后下拉关闭
    await nextTick()
    expect(wrapper.find('.suggest-dropdown').exists()).toBe(false)
  })

  it('Esc 关闭下拉（webview 显隐恢复由 setWebviewVisible mock 承接）', async () => {
    vi.useFakeTimers()
    invokeMock.mockResolvedValue([])
    const { wrapper } = mountWith([makeTab('a')], 'a')
    await addrInput(wrapper).setValue('https://example.com')
    await addrInput(wrapper).trigger('input')
    await vi.advanceTimersByTimeAsync(200)
    await nextTick()
    expect(wrapper.find('.suggest-dropdown').exists()).toBe(true)
    await addrInput(wrapper).trigger('keydown', { key: 'Escape' })
    await nextTick()
    expect(wrapper.find('.suggest-dropdown').exists()).toBe(false)
  })

  it('书签命中排在直达之后', async () => {
    vi.useFakeTimers()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'search_bookmarks') {
        return Promise.resolve([{ id: 7, title: '例站', url: 'https://example.com' }])
      }
      return Promise.resolve([])
    })
    const { wrapper } = mountWith([makeTab('a')], 'a')
    await addrInput(wrapper).setValue('example')
    await addrInput(wrapper).trigger('input')
    await vi.advanceTimersByTimeAsync(200)
    await nextTick()
    const items = wrapper.findAll('.suggest-item')
    expect(items.length).toBe(2)
    expect(items[0].text()).toContain('打开')
    expect(items[1].text()).toContain('例站')
  })
})

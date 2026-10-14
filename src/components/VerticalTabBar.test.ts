// VerticalTabBar 组件测试（R4-6 包 2：键盘可达 · 2026-10-14）
//
// 手法同 TabBar.test.ts：真实挂载 + 真实 pinia（$patch 种子）+ mock Tauri IPC；
// 键盘断言用原生 dispatchEvent（bubbles 模拟冒泡到容器委托）。
//
// 跑法：npx vitest run src/components/VerticalTabBar.test.ts
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

import { useTabsStore } from '../stores/tabs'
import VerticalTabBar from './VerticalTabBar.vue'

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
// 键盘用例需要真实文档树（jsdom 对游离 DOM 的 focus() 无效）；逐测试清理
afterEach(() => {
  document.body.innerHTML = ''
})

function mountWith(tabs: Tab[], activeTabId: string | null) {
  const tabsStore = useTabsStore()
  tabsStore.$patch({ tabs, activeTabId })
  const wrapper = mount(VerticalTabBar, { attachTo: document.body, global: { plugins: [pinia] } })
  return { wrapper, tabsStore }
}

const press = (el: Element, key: string, opts: KeyboardEventInit = {}) => {
  el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...opts }))
}

describe('VerticalTabBar：渲染与点击', () => {
  it('渲染标签（title 回退 url）与 active 态', () => {
    const { wrapper } = mountWith([makeTab('a'), makeTab('b', { title: '' })], 'a')
    const items = wrapper.findAll('.vtab-item')
    expect(items.length).toBe(2)
    expect(items[0].classes()).toContain('active')
    expect(items[0].text()).toContain('a')
    expect(items[1].text()).toContain('https://b.com/')
  })

  it('点击标签 → switchTab；点关闭 → closeTab 且不冒泡切换', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const switchSpy = vi.spyOn(tabsStore, 'switchTab').mockResolvedValue(undefined)
    await wrapper.findAll('.vtab-item')[1].trigger('click')
    expect(switchSpy).toHaveBeenCalledWith('b')

    const closeSpy = vi.spyOn(tabsStore, 'closeTab').mockResolvedValue(undefined)
    await wrapper.findAll('.vtab-item')[1].find('.vtab-close').trigger('click')
    expect(closeSpy).toHaveBeenCalledWith('b')
    expect(switchSpy).toHaveBeenCalledTimes(1) // 关闭按钮点击被 stopPropagation，不再切换
  })
})

describe('VerticalTabBar：键盘可达（R4-6 包 2）', () => {
  it('tablist/tab 语义：role、aria-selected、roving tabindex', () => {
    const { wrapper } = mountWith([makeTab('a'), makeTab('b')], 'a')
    expect(wrapper.find('.vtab-list').attributes('role')).toBe('tablist')
    const items = wrapper.findAll('.vtab-item')
    expect(items[0].attributes('role')).toBe('tab')
    expect(items[0].attributes('aria-selected')).toBe('true')
    expect(items[0].attributes('tabindex')).toBe('0')
    expect(items[1].attributes('aria-selected')).toBe('false')
    expect(items[1].attributes('tabindex')).toBe('-1')
  })

  it('↑/↓ 在标签间移动焦点（循环）', async () => {
    const { wrapper } = mountWith([makeTab('a'), makeTab('b'), makeTab('c')], 'a')
    const items = wrapper.findAll('.vtab-item').map((t) => t.element as HTMLElement)
    items[0].focus()
    press(items[0], 'ArrowDown')
    await nextTick()
    expect(document.activeElement).toBe(items[1])
    press(items[2], 'ArrowDown') // 末尾 → 循环回首（事件 target 决定起点）
    await nextTick()
    expect(document.activeElement).toBe(items[0])
    press(items[0], 'ArrowUp') // 开头 → 循环回尾
    await nextTick()
    expect(document.activeElement).toBe(items[2])
  })

  it('Enter 激活聚焦标签（switchTab）', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'switchTab').mockResolvedValue(undefined)
    const items = wrapper.findAll('.vtab-item').map((t) => t.element as HTMLElement)
    items[1].focus()
    press(items[1], 'Enter')
    await nextTick()
    expect(spy).toHaveBeenCalledWith('b')
  })
})

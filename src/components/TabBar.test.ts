// TabBar 组件渲染/交互测试（R4-2 组件测试补课 · 2026-10-14）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子数据）+
// mock Tauri IPC（invoke/listen/webview 显隐）。动作类断言用 spyOn store，
// 保持单测边界（不真发 IPC）。mock 口径照 stores/tabs.test.ts 既有模式。
//
// 覆盖：标签渲染与激活态、点击切换（活动标签不重复切）、关闭按钮/中键/双击关、
// 固定标签置顶、分组头渲染/数量/折叠、折叠组标签不渲染、新标签按钮。
//
// 跑法：npx vitest run src/components/TabBar.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import { nextTick } from 'vue'
import type { Tab } from '../types/tab'
import type { TabGroup } from '../stores/groups'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('../utils/webview', () => ({
  // 统一口径：一律返回 promise（AddressBar 侧对它 .catch()；能 .catch() 的 mock 两处通用）
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

import { useTabsStore } from '../stores/tabs'
import { useGroupsStore } from '../stores/groups'
import TabBar from './TabBar.vue'

/** 构造合法 Tab（Rust 侧字段 + 前端运行时 groupId），同 stores/tabs.test.ts 口径 */
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

/** 种子：标签 + 分组都挂载前种好；返回挂载好的组件与 stores */
function mountWith(tabs: Tab[], activeTabId: string | null, groups: TabGroup[] = []) {
  const tabsStore = useTabsStore()
  tabsStore.$patch({ tabs, activeTabId })
  const groupsStore = useGroupsStore()
  groupsStore.$patch({ groups })
  const wrapper = mount(TabBar, { global: { plugins: [pinia] } })
  return { wrapper, tabsStore, groupsStore }
}

const testGroup: TabGroup = {
  id: 1,
  name: '工作',
  color: '#4a9eff',
  collapsed: false,
  created_at: '2026-01-01T00:00:00Z',
}

describe('TabBar：标签渲染', () => {
  it('渲染未分组标签：data-tab-id 与标题', () => {
    const { wrapper } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const tabs = wrapper.findAll('.tab')
    expect(tabs.length).toBe(2)
    expect(tabs[0].attributes('data-tab-id')).toBe('a')
    expect(tabs[0].text()).toContain('a')
  })

  it('活动标签带 active class', () => {
    const { wrapper } = mountWith([makeTab('a'), makeTab('b')], 'b')
    const b = wrapper.findAll('.tab').find((w) => w.attributes('data-tab-id') === 'b')!
    expect(b.classes()).toContain('active')
  })

  it('固定标签排最前（pinnedFirst），固定标签显示图钉', () => {
    const { wrapper } = mountWith([makeTab('n1'), makeTab('p1', { is_pinned: true })], 'n1')
    const tabs = wrapper.findAll('.tab')
    expect(tabs[0].attributes('data-tab-id')).toBe('p1')
    expect(tabs[0].find('.pin-icon').exists()).toBe(true)
    expect(tabs[1].find('.pin-icon').exists()).toBe(false)
  })
})

describe('TabBar：切换与关闭', () => {
  it('点击非活动标签 → switchTab(id)', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'switchTab').mockResolvedValue(undefined)
    await wrapper.findAll('.tab')[1].trigger('click')
    expect(spy).toHaveBeenCalledWith('b')
  })

  it('点击已活动标签不重复 switchTab', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'switchTab').mockResolvedValue(undefined)
    await wrapper.findAll('.tab')[0].trigger('click')
    expect(spy).not.toHaveBeenCalled()
  })

  it('点关闭按钮 × → closeTab(id)', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'closeTab').mockResolvedValue(undefined)
    await wrapper.findAll('.tab')[1].find('.close-btn').trigger('click')
    expect(spy).toHaveBeenCalledWith('b')
  })

  it('中键 auxclick(button=1) 关闭；左键 auxclick 不关', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'closeTab').mockResolvedValue(undefined)
    await wrapper.findAll('.tab')[1].trigger('auxclick', { button: 1 })
    expect(spy).toHaveBeenCalledWith('b')
    spy.mockClear()
    await wrapper.findAll('.tab')[1].trigger('auxclick', { button: 0 })
    expect(spy).not.toHaveBeenCalled()
  })

  it('双击标签 → closeTab(id)（浏览器习惯）', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a'), makeTab('b')], 'a')
    const spy = vi.spyOn(tabsStore, 'closeTab').mockResolvedValue(undefined)
    await wrapper.findAll('.tab')[1].trigger('dblclick')
    expect(spy).toHaveBeenCalledWith('b')
  })
})

describe('TabBar：分组', () => {
  it('分组头渲染组名与数量，组内标签渲染', () => {
    const { wrapper } = mountWith(
      [makeTab('a'), makeTab('g1', { groupId: 1 })],
      'a',
      [testGroup],
    )
    const head = wrapper.find('[data-group-id="1"]')
    expect(head.exists()).toBe(true)
    expect(head.find('.group-name').text()).toBe('工作')
    expect(head.find('.group-count').text()).toBe('1')
    // 未分组 1 个 + 组内 1 个
    expect(wrapper.findAll('.tab').length).toBe(2)
  })

  it('点折叠按钮 → updateGroup(id, {collapsed:true})', async () => {
    const { wrapper, groupsStore } = mountWith(
      [makeTab('g1', { groupId: 1 })],
      'g1',
      [testGroup],
    )
    const spy = vi.spyOn(groupsStore, 'updateGroup').mockResolvedValue(undefined)
    await wrapper.find('[data-group-id="1"] .group-toggle').trigger('click')
    expect(spy).toHaveBeenCalledWith(1, { collapsed: true })
  })

  it('折叠组的标签不渲染（组头仍在，可展开）', async () => {
    const { wrapper } = mountWith(
      [makeTab('a'), makeTab('g1', { groupId: 1 })],
      'a',
      [{ ...testGroup, collapsed: true }],
    )
    await nextTick()
    expect(wrapper.findAll('.tab').length).toBe(1)
    const head = wrapper.find('[data-group-id="1"]')
    expect(head.exists()).toBe(true)
    expect(head.find('.group-toggle').text()).toBe('▶')
  })
})

describe('TabBar：新标签按钮', () => {
  it('点 + → createTab()', async () => {
    const { wrapper, tabsStore } = mountWith([makeTab('a')], 'a')
    const spy = vi.spyOn(tabsStore, 'createTab').mockResolvedValue(null)
    await wrapper.find('.new-tab-btn').trigger('click')
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy).toHaveBeenCalledWith()
  })
})

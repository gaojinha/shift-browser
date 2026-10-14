// ShieldPanel 组件测试（B-04 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子）+ mock Tauri IPC
//（对齐 PrivacySection.test.ts 模式）。AppIcon 全局注册、UiSwitch 走 reka-ui 底座，
// 测试环境分别用 stub 注入。
// 覆盖：站点/状态渲染、本页计数（基线差值口径）、降盾往返（白名单）、
// 广告拦截与档位开关（sync 命令）、close 事件。
//
// 跑法：npx vitest run src/components/ShieldPanel.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { Tab } from '../types/tab'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

import { useSettingsStore } from '../stores/settings'
import { useShieldStore } from '../stores/shield'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { i18n } from '../i18n'
import ShieldPanel from './ShieldPanel.vue'

const AppIconStub = {
  name: 'AppIcon',
  props: { name: String, size: Number },
  template: '<i class="app-icon-stub" />',
}
/** UiSwitch stub：点击切换并透传 update:modelValue（真组件走 reka-ui 底座，测试不引入） */
const UiSwitchStub = {
  name: 'UiSwitch',
  props: { modelValue: Boolean, label: String },
  emits: ['update:modelValue'],
  template: '<button class="ui-switch-stub" @click="$emit(\'update:modelValue\', !modelValue)" />',
}

let pinia: Pinia

const STATS = { adblock_injections: 15, cleaned_params: 6, privacy_injections: 3 }

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_privacy_stats') return Promise.resolve({ ...STATS })
    if (cmd === 'get_privacy_whitelist') return Promise.resolve([])
    return Promise.resolve(undefined)
  })
})

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

function seedTab() {
  const tabsStore = useTabsStore()
  tabsStore.$patch({ tabs: [makeTab('example')], activeTabId: 'example' })
}

function mountPanel() {
  return mount(ShieldPanel, {
    global: {
      plugins: [pinia, i18n],
      components: { AppIcon: AppIconStub },
      // 局部导入组件须用 stubs 替换（components 选项只对全局注册生效）
      stubs: { UiSwitch: UiSwitchStub },
    },
  })
}

describe('ShieldPanel：渲染与状态', () => {
  it('显示站点域名与防护状态（默认盾 UP）', async () => {
    seedTab()
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('get_privacy_whitelist'))
    expect(wrapper.find('.shield-site').text()).toBe('example.com')
    expect(wrapper.find('.shield-state').text()).toBe('防护中')
  })

  it('站点在白名单中：显示已降盾 + 救援按钮变恢复 + 计数区切换为暂停提示', async () => {
    seedTab()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_privacy_stats') return Promise.resolve({ ...STATS })
      if (cmd === 'get_privacy_whitelist') return Promise.resolve(['example.com'])
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(wrapper.find('.shield-state').text()).toBe('已降盾'))
    expect(wrapper.find('.rescue-btn').text()).toBe('恢复本页防护')
    expect(wrapper.find('.stat-paused').exists()).toBe(true)
  })
})

describe('ShieldPanel：本页计数（基线差值口径）', () => {
  it('delta = 当前统计 - 基线（近似口径）', async () => {
    seedTab()
    const shieldStore = useShieldStore()
    shieldStore.$patch({
      baseline: { adblock_injections: 10, cleaned_params: 4, privacy_injections: 2 },
      baselineUrl: 'https://example.com/',
    })
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(wrapper.find('.stat-num').text()).toBe('5'))
    expect(wrapper.find('.stat-sub').text()).toContain('本页清参 2 · 隐私注入 1')
  })
})

describe('ShieldPanel：交互', () => {
  it('点击降盾按钮：写白名单（移除当前域名并保留其它项）', async () => {
    seedTab()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_privacy_stats') return Promise.resolve({ ...STATS })
      if (cmd === 'get_privacy_whitelist') return Promise.resolve(['example.com', 'other.com'])
      return Promise.resolve(undefined)
    })
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(wrapper.find('.rescue-btn').text()).toBe('恢复本页防护'))
    await wrapper.find('.rescue-btn').trigger('click')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_privacy_whitelist', { domains: ['other.com'] }),
    )
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: 'success', message: '本页防护已恢复' }),
      ]),
    )
  })

  it('广告拦截开关：写设置 + set_adblock_enabled 命令', async () => {
    seedTab()
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('get_privacy_stats'))
    const switches = wrapper.findAll('.ui-switch-stub')
    await switches[0].trigger('click')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_adblock_enabled', { enabled: false }),
    )
    expect(useSettingsStore().get('adblock_enabled')).toBe('false')
  })

  it('档位切换：set_privacy_shield_level', async () => {
    seedTab()
    const wrapper = mountPanel()
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('get_privacy_stats'))
    await wrapper.find('select#shield-level').setValue('strict')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_privacy_shield_level', { level: 'strict' }),
    )
  })

  it('关闭按钮：emit close', async () => {
    seedTab()
    const wrapper = mountPanel()
    await wrapper.find('.close-btn').trigger('click')
    expect(wrapper.emitted('close')).toBeTruthy()
  })
})

// GeneralSection 组件渲染/交互测试（R4-2 第二批 · 2026-10-14）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子设置值）+
// mock Tauri IPC（invoke/listen）。动作类断言用 invokeMock 调用记录，
// 不真发 IPC；默认浏览器状态用 invoke mock 返回值驱动。
//
// 覆盖：默认浏览器状态渲染与设默认（成功/失败 toast）、主页 URL 保存、
// 搜索引擎/标签休眠/翻译源 SegControl 切换写 store + sync 命令、
// 垂直标签开关（含 set_vertical_tabs 同步）、启动查版本开关、
// 翻译目标语言/翻译超时保存。
//
// 跑法：npx vitest run src/components/settings/GeneralSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

import { useSettingsStore } from '../../stores/settings'
import { useToastStore } from '../../stores/toast'
import GeneralSection from './GeneralSection.vue'

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'default_browser_status') {
      return Promise.resolve({
        is_default: false,
        current: 'firefox.desktop',
        desktop_id: 'shift-browser.desktop',
        desktop_ready: true,
      })
    }
    if (cmd === 'set_default_browser') {
      return Promise.resolve({
        is_default: true,
        current: 'shift-browser.desktop',
        desktop_id: 'shift-browser.desktop',
        desktop_ready: true,
      })
    }
    return Promise.resolve(undefined)
  })
})

function mountWith(patch: Record<string, string> = {}) {
  const settingsStore = useSettingsStore()
  if (Object.keys(patch).length) settingsStore.$patch({ settings: { ...patch } })
  const wrapper = mount(GeneralSection, { global: { plugins: [pinia] } })
  return { wrapper, settingsStore }
}

/** 按 aria-label 找 SegControl 内指定 radio（文本匹配，避免猜下标） */
function radio(
  wrapper: VueWrapper,
  groupLabel: string,
  match: (text: string) => boolean,
): VueWrapper {
  const group = wrapper.find(`[role="radiogroup"][aria-label="${groupLabel}"]`)
  const radios = group.findAll('[role="radio"]')
  const hit = radios.find((r) => match(r.text()))
  if (!hit) throw new Error(`radio not found in group "${groupLabel}"`)
  return hit
}

/** 等待 invoke 被调用 n 次（onMounted 等异步） */
async function flushInvoke(times: number) {
  await vi.waitFor(() => expect(invokeMock.mock.calls.length).toBeGreaterThanOrEqual(times), {
    timeout: 1000,
  })
}

describe('默认浏览器', () => {
  it('is_default=false + current 非空：显示当前默认值，按钮可点', async () => {
    const { wrapper } = mountWith()
    await flushInvoke(1) // onMounted → default_browser_status
    expect(invokeMock).toHaveBeenCalledWith('default_browser_status')
    expect(wrapper.text()).toContain('当前默认：firefox.desktop')
    const btn = wrapper.find('button.mini-btn.primary')
    expect(btn.text()).toContain('设为默认浏览器')
    expect(btn.attributes('disabled')).toBeUndefined()
  })

  it('is_default=true：按钮禁用、显示已是默认', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'default_browser_status') {
        return Promise.resolve({
          is_default: true,
          current: 'shift-browser.desktop',
          desktop_id: 'shift-browser.desktop',
          desktop_ready: true,
        })
      }
      return Promise.resolve(undefined)
    })
    const { wrapper } = mountWith()
    await flushInvoke(1)
    expect(wrapper.text()).toContain('当前已是默认浏览器')
    expect(wrapper.find('button.mini-btn.primary').attributes('disabled')).toBeDefined()
  })

  it('点击设默认：成功后状态回写 + success toast', async () => {
    const { wrapper } = mountWith()
    await flushInvoke(1)
    await wrapper.find('button.mini-btn.primary').trigger('click')
    expect(invokeMock).toHaveBeenCalledWith('set_default_browser')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: 'success', message: '已设为默认浏览器' }),
      ]),
    )
    expect(wrapper.text()).toContain('当前已是默认浏览器')
  })

  it('设默认失败：error toast、状态不变', async () => {
    const { wrapper } = mountWith()
    await flushInvoke(1)
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'set_default_browser') return Promise.reject('E_INTERNAL: 权限不足')
      return Promise.resolve(undefined)
    })
    await wrapper.find('button.mini-btn.primary').trigger('click')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([expect.objectContaining({ type: 'error' })]),
    )
    expect(wrapper.text()).not.toContain('当前已是默认浏览器')
  })
})

describe('主页 URL', () => {
  it('change 写入 settings + invoke set_setting（带 trim）', async () => {
    const { wrapper, settingsStore } = mountWith({ homepage: '  https://example.com  ' })
    const input = wrapper.find('#homepage-input')
    await input.setValue('https://example.com')
    await input.trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'homepage',
      value: 'https://example.com',
    }))
    expect(settingsStore.get('homepage')).toBe('https://example.com')
  })

  it('未设置时渲染默认 newtab', () => {
    const { wrapper } = mountWith()
    expect((wrapper.find('#homepage-input').element as HTMLInputElement).value).toBe('newtab')
  })
})

describe('搜索引擎 SegControl', () => {
  it('默认无设置：必应选中', () => {
    const { wrapper } = mountWith()
    const checked = wrapper.find(
      '[role="radiogroup"][aria-label="搜索引擎"] [role="radio"][aria-checked="true"]',
    )
    expect(checked.text()).toContain('必应')
  })

  it('切到 谷歌：写 store + set_setting + refresh_newtab', async () => {
    const { wrapper, settingsStore } = mountWith()
    await radio(wrapper, '搜索引擎', (t) => t.includes('谷歌')).trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('refresh_newtab'))
    expect(settingsStore.get('search_engine')).toBe('google')
    expect(invokeMock).toHaveBeenCalledWith('set_setting', { key: 'search_engine', value: 'google' })
    expect(invokeMock).toHaveBeenCalledWith('refresh_newtab')
  })
})

describe('标签休眠', () => {
  it('默认 10 分钟选中；默认休眠数阈值渲染 8', () => {
    const { wrapper } = mountWith()
    const checked = wrapper.find(
      '[role="radiogroup"][aria-label="标签休眠"] [role="radio"][aria-checked="true"]',
    )
    expect(checked.text()).toContain('10')
    expect((wrapper.find('#hibernate-count').element as HTMLInputElement).value).toBe('8')
  })

  it('切到 30 分钟：写 store + set_setting', async () => {
    const { wrapper, settingsStore } = mountWith()
    await radio(wrapper, '标签休眠', (t) => t.includes('30')).trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'tab_hibernate_minutes',
      value: '30',
    }))
    expect(settingsStore.get('tab_hibernate_minutes')).toBe('30')
  })

  it('休眠标签数阈值 change 写入', async () => {
    const { wrapper, settingsStore } = mountWith()
    const input = wrapper.find('#hibernate-count')
    await input.setValue('12')
    await input.trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'tab_hibernate_count',
      value: '12',
    }))
    expect(settingsStore.get('tab_hibernate_count')).toBe('12')
  })
})

describe('垂直标签栏 / 启动查版本 开关', () => {
  it('默认未开启：垂直标签 switch 未选中（启动查版本默认开）', () => {
    const { wrapper } = mountWith()
    expect(wrapper.find('[role="switch"][aria-label="垂直标签栏"]').attributes('aria-checked')).toBe('false')
    expect(wrapper.find('[role="switch"][aria-label="启动查版本"]').attributes('aria-checked')).toBe('true')
  })

  it('开启垂直标签栏：写 store + set_vertical_tabs(active=true)', async () => {
    const { wrapper, settingsStore } = mountWith()
    const vertical = wrapper.find('[role="switch"][aria-label="垂直标签栏"]')
    expect(vertical.attributes('aria-checked')).toBe('false')
    await vertical.trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_vertical_tabs', { active: true }))
    expect(settingsStore.get('vertical_tabs')).toBe('true')
    expect(invokeMock).toHaveBeenCalledWith('set_setting', { key: 'vertical_tabs', value: 'true' })
  })

  it('关闭启动查版本：写 store（默认 true → false）', async () => {
    const { wrapper, settingsStore } = mountWith()
    const upd = wrapper.find('[role="switch"][aria-label="启动查版本"]')
    expect(upd.attributes('aria-checked')).toBe('true')
    await upd.trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'update_check_enabled',
      value: 'false',
    }))
    expect(settingsStore.get('update_check_enabled')).toBe('false')
  })
})

describe('翻译源 SegControl', () => {
  it('默认 Google 选中', () => {
    const { wrapper } = mountWith()
    const checked = wrapper.find(
      '[role="radiogroup"][aria-label="翻译源"] [role="radio"][aria-checked="true"]',
    )
    expect(checked.text()).toContain('Google')
  })

  it('切到 Bing：写 store + set_setting', async () => {
    const { wrapper, settingsStore } = mountWith()
    await radio(wrapper, '翻译源', (t) => t.includes('Bing')).trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'translate_engine',
      value: 'bing',
    }))
    expect(settingsStore.get('translate_engine')).toBe('bing')
  })
})

describe('翻译目标语言', () => {
  it('默认 zh-CN；change 写入', async () => {
    const { wrapper, settingsStore } = mountWith()
    const select = wrapper.find('#translate-target')
    expect(select.element.value).toBe('zh-CN')
    await select.setValue('en')
    await select.trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'translate_target',
      value: 'en',
    }))
    expect(settingsStore.get('translate_target')).toBe('en')
  })
})

describe('翻译超时', () => {
  it('默认 15；change 写入', async () => {
    const { wrapper, settingsStore } = mountWith()
    const input = wrapper.find('#translate-timeout')
    expect(input.element.value).toBe('15')
    await input.setValue('30')
    await input.trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_setting', {
      key: 'translate_timeout',
      value: '30',
    }))
    expect(settingsStore.get('translate_timeout')).toBe('30')
  })
})

// WebmcpCard 组件测试（F-12 前端组件第三批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + real SettingSwitch/UiSwitch（reka 底座在 jsdom
// 可挂，见 .learnings/2026-10-15-reka-ui-mounts-in-jsdom.md）+ 真实 pinia（toast）
// + mock Tauri IPC 与 AppIcon。
// 覆盖：挂载读取开关初值 / 开启态显示「试验中」徽标与卡片态类 / 切换调用 set 命令并提示 /
// 切换失败回滚 UI 并报错 / 读取失败不崩（保持关闭）。
//
// 跑法：npx vitest run src/components/settings/WebmcpCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeOkMock = vi.fn()
vi.mock('../../utils/invoke', () => ({
  invokeOk: (...args: unknown[]) => invokeOkMock(...args),
  errText: (e: unknown) => String(e),
}))

import { useToastStore } from '../../stores/toast'
import WebmcpCard from './WebmcpCard.vue'

const AppIconStub = {
  name: 'AppIcon',
  props: { name: String, size: Number },
  template: '<i class="app-icon-stub" />',
}

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeOkMock.mockReset()
})

async function mountCard(initial: boolean | Error = false, opts: { rejectGet?: boolean } = {}) {
  invokeOkMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_webmcp_polyfill') {
      if (opts.rejectGet) return Promise.reject('读取失败')
      return Promise.resolve(initial as boolean)
    }
    return Promise.resolve(undefined)
  })
  const wrapper = mount(WebmcpCard, {
    global: { plugins: [pinia], components: { AppIcon: AppIconStub } },
  })
  await flushPromises()
  return { wrapper, toast: useToastStore() }
}

describe('WebmcpCard', () => {
  it('挂载时读取开关初值', async () => {
    const { wrapper } = await mountCard(false)
    expect(invokeOkMock).toHaveBeenCalledWith('get_webmcp_polyfill')
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('false')
  })

  it('已开启：显示「试验中」徽标', async () => {
    const { wrapper } = await mountCard(true)
    expect(wrapper.find('.exp-badge').exists()).toBe(true)
    expect(wrapper.find('.field').classes()).toContain('experiment-tag')
  })

  it('关闭态：无徽标', async () => {
    const { wrapper } = await mountCard(false)
    expect(wrapper.find('.exp-badge').exists()).toBe(false)
    expect(wrapper.find('.field').classes()).not.toContain('experiment-tag')
  })

  it('切换开启：调用 set 命令并提示', async () => {
    const { wrapper, toast } = await mountCard(false)
    await wrapper.find('[role="switch"]').trigger('click')
    await flushPromises()

    expect(invokeOkMock).toHaveBeenCalledWith('set_webmcp_polyfill', { enabled: true })
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('true')
    expect(toast.items.some((t) => t.message.includes('已开启'))).toBe(true)
  })

  it('切换关闭：调用 set 命令并提示', async () => {
    const { wrapper, toast } = await mountCard(true)
    await wrapper.find('[role="switch"]').trigger('click')
    await flushPromises()

    expect(invokeOkMock).toHaveBeenCalledWith('set_webmcp_polyfill', { enabled: false })
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('false')
    expect(toast.items.some((t) => t.message.includes('已关闭'))).toBe(true)
  })

  it('切换失败：回滚开关并报错', async () => {
    const { wrapper, toast } = await mountCard(false)
    invokeOkMock.mockRejectedValueOnce('后端拒绝')

    await wrapper.find('[role="switch"]').trigger('click')
    await flushPromises()

    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('false') // 已回滚
    expect(toast.items.some((t) => t.type === 'error' && t.message.includes('切换失败'))).toBe(true)
  })

  it('读取失败不崩，保持关闭态', async () => {
    const { wrapper } = await mountCard(false, { rejectGet: true })
    expect(wrapper.find('.card').exists()).toBe(true)
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('false')
  })
})

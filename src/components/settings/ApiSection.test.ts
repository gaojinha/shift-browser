// ApiSection 组件测试（F-86 · 前端测试补课）
//
// 手法：settings store 真实 pinia；mock invoke（create_dashboard_ticket /
// rotate_api_token）与 plugin-opener；window.confirm / navigator.clipboard
// 由 jsdom 环境打桩。危险开关断言 onSet 落库映射（'true'/'false' 字符串）。
//
// 跑法：npx vitest run src/components/settings/ApiSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ApiSection from './ApiSection.vue'
import { useSettingsStore } from '../../stores/settings'
import { useToastStore } from '../../stores/toast'
import { openUrl } from '@tauri-apps/plugin-opener'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)
const openUrlMock = vi.mocked(openUrl)

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue('ticket-123')
  openUrlMock.mockClear()
  vi.stubGlobal('confirm', vi.fn().mockReturnValue(true))
  Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } })
})

afterEach(() => {
  vi.unstubAllGlobals()
})

function switches() {
  // SettingSwitch 底座是 reka role=switch（包装成 DOMWrapper 才有 attributes()）
  return Array.from(document.querySelectorAll('[role="switch"]')).map((el) => new DOMWrapper(el))
}

const MOUNTS: import('@vue/test-utils').VueWrapper[] = []

function mountSection() {
  // attachTo 必须加：否则 DOM 脱离文档树，document.querySelector 全查空
  const w = mount(ApiSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}

// attachTo 的实例必须卸载，否则跨用例残留污染 document 查询
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
  vi.unstubAllGlobals()
})

describe('ApiSection · 危险开关与端口', () => {
  it('五个开关 + 端口 + 白名单渲染；开关 checked 随设置值', async () => {
    const store = useSettingsStore()
    store.$patch({ settings: { api_allow_eval: 'true', api_readonly: 'false' } })
    mountSection()
    await flushPromises()
    const sw = switches()
    expect(sw).toHaveLength(5) // eval/readonly/agent_actions/lan/model_gateway
    expect(sw[0]!.attributes('aria-checked')).toBe('true')
    expect(sw[1]!.attributes('aria-checked')).toBe('false')
    expect((document.querySelector('#api-http-port') as HTMLInputElement).value).toBe('19701') // 默认端口
  })

  it('开关 change → onSet 落库映射 true/false 字符串', async () => {
    const store = useSettingsStore()
    mountSection()
    await flushPromises()
    // reka 底座吃真实 MouseEvent（trigger 造的），裸 Event('click') 触不到
    const sw = Array.from(document.querySelectorAll('[role="switch"]'))[2]!
    await new DOMWrapper(sw).trigger('click')
    await flushPromises()
    expect(store.settings['api_agent_actions']).toBe('true')
  })

  it('端口与白名单 change 落库', async () => {
    const store = useSettingsStore()
    mountSection()
    await flushPromises()
    const port = document.querySelector('#api-http-port') as HTMLInputElement
    port.value = '28080'
    await new DOMWrapper(port).trigger('change')
    await flushPromises()
    expect(store.settings['api_http_port']).toBe('28080')
    const ips = document.querySelector('#api-allowed-ips') as HTMLInputElement
    ips.value = '192.168.1.0/24'
    await new DOMWrapper(ips).trigger('change')
    await flushPromises()
    expect(store.settings['api_allowed_ips']).toBe('192.168.1.0/24')
  })
})

describe('ApiSection · 状态面板门票', () => {
  it('成功：create_dashboard_ticket → openUrl 带 port+ticket（encodeURIComponent）+ success toast', async () => {
    const store = useSettingsStore()
    store.$patch({ settings: { api_http_port: '28080' } })
    invokeMock.mockResolvedValue('t k/1')
    mountSection()
    await flushPromises()
    const btns = Array.from(document.querySelectorAll('.api-actions .selfcheck-btn'))
    ;(btns[0] as HTMLElement).click()
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('create_dashboard_ticket')
    expect(openUrlMock).toHaveBeenCalledWith('http://127.0.0.1:28080/?ticket=t%20k%2F1')
    expect(useToastStore().items.some((t) => t.type === 'success' && t.message.includes('状态面板'))).toBe(true)
  })

  it('失败 → toast「打开状态面板失败」，按钮解除禁用', async () => {
    invokeMock.mockRejectedValue(new Error('no ticket'))
    mountSection()
    await flushPromises()
    const btns = Array.from(document.querySelectorAll('.api-actions .selfcheck-btn'))
    ;(btns[0] as HTMLElement).click()
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '打开状态面板失败')).toBe(true)
    expect((btns[0] as HTMLButtonElement).disabled).toBe(false)
  })
})

describe('ApiSection · token 轮换', () => {
  it('confirm 确认后轮换：rotate_api_token → 新 token 显示 + success toast；复制走 clipboard', async () => {
    invokeMock.mockResolvedValue('new-token-abc')
    mountSection()
    await flushPromises()
    const btns = Array.from(document.querySelectorAll('.api-actions .selfcheck-btn'))
    ;(btns[1] as HTMLElement).click()
    await flushPromises()
    expect(confirm).toHaveBeenCalled()
    expect(invokeMock).toHaveBeenCalledWith('rotate_api_token')
    expect(useToastStore().items.some((t) => t.message.includes('API token 已轮换'))).toBe(true)
    const code = document.querySelector('.api-token-result code')
    expect(code?.textContent).toBe('new-token-abc')
    // 复制
    await (document.querySelector('.api-token-result .selfcheck-btn') as HTMLElement).click()
    await flushPromises()
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith('new-token-abc')
    expect(useToastStore().items.some((t) => t.message === '新 token 已复制')).toBe(true)
  })

  it('confirm 取消 → 不调 rotate', async () => {
    vi.stubGlobal('confirm', vi.fn().mockReturnValue(false))
    mountSection()
    await flushPromises()
    const btns = Array.from(document.querySelectorAll('.api-actions .selfcheck-btn'))
    ;(btns[1] as HTMLElement).click()
    await flushPromises()
    expect(invokeMock).not.toHaveBeenCalledWith('rotate_api_token')
    expect(document.querySelector('.api-token-result')).toBeNull()
  })

  it('轮换失败 → toast「轮换 API token 失败」', async () => {
    invokeMock.mockRejectedValue(new Error('db'))
    mountSection()
    await flushPromises()
    const btns = Array.from(document.querySelectorAll('.api-actions .selfcheck-btn'))
    ;(btns[1] as HTMLElement).click()
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '轮换 API token 失败')).toBe(true)
  })
})

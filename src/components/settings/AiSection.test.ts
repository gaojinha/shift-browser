// AiSection 组件测试（F-86 · 前端测试补课）
//
// 手法：settings store 真实 pinia（$patch 设值模拟已存设置）；invoke 与
// invokeOk 分开 mock（invoke=ai_list_providers，invokeOk=opencode 探测/模型）；
// 子页签 McpSection/WebmcpCard stub 隔离（各有独立测试文件）。
// 敏感项断言：API Key 输入恒 type=password、change 后清空不回显、placeholder
// 只暴露"已配置"与否，绝不出现明文。
//
// 跑法：npx vitest run src/components/settings/AiSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import AiSection from './AiSection.vue'
import { useSettingsStore } from '../../stores/settings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../../utils/invoke', () => ({
  invokeOk: vi.fn(),
  errText: (e: unknown) => (e instanceof Error ? e.message : String(e)),
}))

import { invoke } from '@tauri-apps/api/core'
import { invokeOk } from '../../utils/invoke'
const invokeMock = vi.mocked(invoke)
const invokeOkMock = vi.mocked(invokeOk)

function mountSection(initialTab?: 'model' | 'mcp' | 'labs') {
  return mount(AiSection, {
    props: initialTab ? { initialTab } : {},
    global: { stubs: { McpSection: { template: '<div class="mcp-stub"/>' }, WebmcpCard: { template: '<div class="labs-stub"/>' } } },
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeOkMock.mockReset()
  invokeMock.mockResolvedValue([]) // ai_list_providers 默认空
})

describe('AiSection · 页签', () => {
  it('三页签渲染，默认激活「模型服务」；MCP/试验未访问不挂载（懒挂载）', async () => {
    const w = mountSection()
    await flushPromises()
    const tabs = w.findAll('.ai-tab')
    expect(tabs).toHaveLength(3)
    expect(tabs[0]!.text()).toContain('模型服务')
    expect(tabs[0]!.attributes('aria-selected')).toBe('true')
    expect(w.find('.mcp-stub').exists()).toBe(false)
    expect(w.find('.labs-stub').exists()).toBe(false)
  })

  it('切换页签：aria-selected 跟随；MCP 首次激活才挂载，切回仍保留（visited）', async () => {
    const w = mountSection()
    await flushPromises()
    await w.findAll('.ai-tab')[1]!.trigger('click')
    await flushPromises()
    expect(w.findAll('.ai-tab')[1]!.attributes('aria-selected')).toBe('true')
    expect(w.find('.mcp-stub').exists()).toBe(true)
    await w.findAll('.ai-tab')[0]!.trigger('click')
    await flushPromises()
    // v-show 隐藏但 v-if visited=true 已挂载过 → stub 仍在
    expect(w.find('.mcp-stub').exists()).toBe(true)
  })

  it('initialTab prop 直达对应页签', async () => {
    const w = mountSection('labs')
    await flushPromises()
    expect(w.findAll('.ai-tab')[2]!.attributes('aria-selected')).toBe('true')
    expect(w.find('.labs-stub').exists()).toBe(true)
  })
})

describe('AiSection · 模型服务（AGNES 分支）', () => {
  it('ai_list_providers 有数据渲染 options；空列表回落「AGNES 云端」单选项', async () => {
    invokeMock.mockResolvedValue([
      { id: 'agnes', name: 'AGNES 云端', configured: true, description: '' },
      { id: 'local', name: '本机 Ollama', configured: false, description: '' },
    ])
    let w = mountSection()
    await flushPromises()
    const opts = w.findAll('#ai-provider option')
    expect(opts.map((o) => o.text())).toEqual(['AGNES 云端', '本机 Ollama'])

    invokeMock.mockResolvedValue([])
    w = mountSection()
    await flushPromises()
    expect(w.findAll('#ai-provider option')).toHaveLength(1)
    expect(w.findAll('#ai-provider option')[0]!.text()).toBe('AGNES 云端')
  })

  it('切换 provider → settingsStore.set("ai_provider", …)；local 值显示本机 Ollama 提示', async () => {
    // select 里得先有 local 选项（providers 为空时只有 AGNES 单选项，setValue 无处可落）
    invokeMock.mockResolvedValue([
      { id: 'agnes', name: 'AGNES 云端', configured: true, description: '' },
      { id: 'local', name: '本机 Ollama', configured: false, description: '' },
    ])
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    await w.find('#ai-provider').setValue('local')
    await flushPromises()
    expect(store.settings['ai_provider']).toBe('local')
    expect(w.text()).toContain('本机 Ollama')
  })

  it('AGNES 字段：模型 select / 超时输入 change 落 settingsStore.set', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    await w.find('#ai-model-agnes').setValue('agnes-2.5-pro')
    await flushPromises()
    expect(store.settings['ai_model']).toBe('agnes-2.5-pro')
    await w.find('#ai-timeout').setValue('180')
    await flushPromises()
    expect(store.settings['ai_timeout']).toBe('180')
  })

  it('API Key 恒 type=password；change 后写入且输入清空不回显', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    const key = w.find('#ai-key')
    expect(key.attributes('type')).toBe('password')
    await key.setValue('sk-secret-123')
    await key.trigger('change')
    await flushPromises()
    expect(store.settings['ai_api_key']).toBe('sk-secret-123')
    // 敏感红线：输入框清空，不回显明文
    expect((w.find('#ai-key').element as HTMLInputElement).value).toBe('')
    expect(w.html()).not.toContain('sk-secret-123')
  })

  it('已配置 key 时 placeholder 只提示「已配置（不回显）」，明文不出现在任何属性里', async () => {
    const store = useSettingsStore()
    store.$patch({ settings: { ai_api_key: 'sk-already-set' } })
    const w = mountSection()
    await flushPromises()
    const ph = w.find('#ai-key').attributes('placeholder') ?? ''
    expect(ph).toContain('已配置（不回显）')
    expect(w.html()).not.toContain('sk-already-set')
  })

  it('API 地址 trim 后落库', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    await w.find('#ai-base-url').setValue('  https://api.example.com/v1  ')
    await w.find('#ai-base-url').trigger('change')
    await flushPromises()
    expect(store.settings['ai_base_url']).toBe('https://api.example.com/v1')
  })
})

describe('AiSection · 本机 opencode 分支', () => {
  function setOc() {
    const store = useSettingsStore()
    store.$patch({ settings: { ai_provider: 'opencode' } })
  }

  it('探测成功：invokeOk 调 probe/list_models，显示「已连接 baseUrl」与模型选项', async () => {
    setOc()
    invokeOkMock.mockImplementation((cmd: string) => {
      if (cmd === 'opencode_probe')
        return Promise.resolve({ ok: true, baseUrl: 'http://127.0.0.1:4096', model: 'beast', askAgentPresent: true })
      if (cmd === 'opencode_list_models')
        return Promise.resolve([{ modelRef: 'p/m', providerId: 'p', modelId: 'm', name: 'Beast', status: 'ok', enabled: true, deprecated: false }])
      return Promise.resolve(undefined)
    })
    const w = mountSection()
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('opencode_probe')
    expect(invokeOkMock).toHaveBeenCalledWith('opencode_list_models')
    expect(w.text()).toContain('已连接 http://127.0.0.1:4096')
    expect(w.findAll('#oc-model option')).toHaveLength(2) // 跟随默认 + 1 模型
  })

  it('探测失败：显示错误与重试按钮；重试 force 重拉', async () => {
    setOc()
    invokeOkMock.mockRejectedValue(new Error('serve 未启动'))
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('serve 未启动')
    const retry = w.find('.oc-retry')
    expect(retry.exists()).toBe(true)
    invokeOkMock.mockClear()
    invokeOkMock.mockResolvedValue({ ok: true, baseUrl: 'http://127.0.0.1:4096', model: '', askAgentPresent: true })
    await retry.trigger('click')
    await flushPromises()
    expect(invokeOkMock).toHaveBeenCalledWith('opencode_probe')
    expect(w.text()).toContain('已连接')
  })

  it('probe 无 askAgent：显示「未检测到只读 plan agent」警示', async () => {
    setOc()
    invokeOkMock.mockResolvedValue({ ok: true, baseUrl: 'http://127.0.0.1:4096', model: '', askAgentPresent: false })
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('未检测到只读 plan agent')
  })

  it('local 分支：模型输入 change 落 ai_model', async () => {
    const store = useSettingsStore()
    store.$patch({ settings: { ai_provider: 'local' } })
    const w = mountSection()
    await flushPromises()
    await w.find('#ai-model-local').setValue('  llama3.1  ')
    await w.find('#ai-model-local').trigger('change')
    await flushPromises()
    expect(store.settings['ai_model']).toBe('llama3.1')
  })
})

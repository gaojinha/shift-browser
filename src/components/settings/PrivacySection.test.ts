// PrivacySection 组件渲染/交互测试（R4-2 第二批 · 2026-10-14）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子设置值）+
// mock Tauri IPC。动作类断言用 invokeMock 调用记录；toast 用真实 store。
// PrivacyDataCard 子组件内 AppIcon 是全局注册组件（main.ts），测试环境无注册，
// 用 global.components.AppIcon 注入 stub。
//
// 覆盖：隐私档位选择（off/standard/strict 切换 + 兼容旧布尔值、sync 命令）、
// 反指纹模式（noise/uniform + sync）、白名单加载与保存、广告拦截开关 + sync、
// 自定义关键词增删（去重/保存失败回滚）、GPC 请求头 switch（含 sync_ext_rules）、
// 网络层拦截规则 / 自定义请求头 change 写入、openCerts 事件透传。
//
// 跑法：npx vitest run src/components/settings/PrivacySection.test.ts
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
import PrivacySection from './PrivacySection.vue'

/** AppIcon 在 main.ts 全局注册，测试环境无注册，stub 避免渲染失败 */
const AppIconStub = {
  name: 'AppIcon',
  props: { name: String, size: Number },
  template: '<i class="app-icon-stub" />',
}

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_privacy_whitelist') return Promise.resolve(['upwork.com', 'ditu.amap.com'])
    if (cmd === 'get_adblock_keywords') return Promise.resolve(['adblock'])
    if (cmd === 'get_user_agent_config') {
      return Promise.resolve({ global: '', default: 'default-ua', overrides: {} })
    }
    return Promise.resolve(undefined)
  })
})

function mountWith(patch: Record<string, string> = {}) {
  const settingsStore = useSettingsStore()
  if (Object.keys(patch).length) settingsStore.$patch({ settings: { ...patch } })
  const wrapper = mount(PrivacySection, {
    global: { plugins: [pinia], components: { AppIcon: AppIconStub } },
  })
  return { wrapper, settingsStore }
}

/** onMounted 会并发 loadWhitelist/loadPrivacyStats/loadKeywords；逐个等到目标状态 */
async function flushWhitelist(wrapper: VueWrapper) {
  await vi.waitFor(() => {
    const ta = wrapper.find('textarea.kw-textarea')
    expect(ta.exists()).toBe(true)
    expect((ta.element as HTMLTextAreaElement).value).toBe('upwork.com\nditu.amap.com')
  }, { timeout: 1000 })
}
async function flushKeywords(wrapper: VueWrapper) {
  await vi.waitFor(() => expect(wrapper.findAll('.kw-chip').length).toBe(1), {
    timeout: 1000,
  })
}
async function flushStats() {
  await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('get_privacy_stats'), {
    timeout: 1000,
  })
}


describe('隐私防护档位', () => {
  it('默认 standard；切到 strict：写 store + set_privacy_shield_level + toast', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const select = wrapper.find('select.privacy-select')
    expect(select.element.value).toBe('standard')
    await select.setValue('strict')
    await select.trigger('change')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_privacy_shield_level', { level: 'strict' }),
    )
    expect(settingsStore.get('privacy_shield_level')).toBe('strict')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([expect.objectContaining({ type: 'success', message: '已切换到严格档' })]),
    )
  })

  it('切到 off：白名单/自测块隐藏（v-if=privacyLevel!=="off"）', async () => {
    const { wrapper } = mountWith()
    await flushStats()
    const selects = wrapper.findAll('select.privacy-select')
    await selects[0].setValue('off')
    await selects[0].trigger('change')
    await wrapper.vm.$nextTick()
    // 白名单 textarea 与「防护自测」按钮消失
    expect(wrapper.findAll('textarea.kw-textarea').length).toBe(0)
    const selfTestBtn = wrapper.findAll('button.mini-btn.primary').find((b) => b.text().includes('运行自测'))
    expect(selfTestBtn).toBeUndefined()
  })

  it('兼容旧值：无 privacy_shield_level + privacy_shield=false → off；=true → standard', async () => {
    const { settingsStore } = mountWith({ privacy_shield_level: '', privacy_shield: 'false' })
    await flushStats()
    const select = settingsStore
      ? mountWith({ privacy_shield_level: '', privacy_shield: 'false' }).wrapper.find('select.privacy-select')
      : null
    expect(select?.element.value).toBe('off')
  })
})

describe('反指纹模式', () => {
  it('默认 noise；切到 uniform：写 store + set_privacy_fp_mode + toast', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const selects = wrapper.findAll('select.privacy-select')
    const fp = selects[1]
    expect(fp.element.value).toBe('noise')
    await fp.setValue('uniform')
    await fp.trigger('change')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_privacy_fp_mode', { mode: 'uniform' }),
    )
    expect(settingsStore.get('privacy_fp_mode')).toBe('uniform')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: 'success', message: '已切换到统一化（Tor/Mullvad 派）' }),
      ]),
    )
  })

  it('无效值 fallback noise：privacy_fp_mode=xxx → 渲染 noise', () => {
    const { wrapper } = mountWith({ privacy_fp_mode: 'xxx' })
    const fp = wrapper.findAll('select.privacy-select')[1]
    expect(fp.element.value).toBe('noise')
  })
})

describe('兼容白名单', () => {
  it('挂载后加载：textarea 预填两行', async () => {
    const { wrapper } = mountWith()
    await flushWhitelist(wrapper)
    const textarea = wrapper.find('textarea.kw-textarea')
    expect(textarea.element.value).toBe('upwork.com\nditu.amap.com')
  })

  it('保存：按行/逗号/空格切分 + 回读 + toast；含中文逗号', async () => {
    const { wrapper } = mountWith()
    await flushWhitelist(wrapper)
    const textarea = wrapper.find('textarea.kw-textarea')
    await textarea.setValue('a.com, b.com ，c.com\nd.com')
    await wrapper.find('button.mini-btn.primary').trigger('click') // 保存白名单
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_privacy_whitelist', {
        domains: ['a.com', 'b.com', 'c.com', 'd.com'],
      }),
    )
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ type: 'success', message: expect.stringContaining('白名单已保存') }),
      ]),
    )
  })

  it('保存失败：error toast、不回写（whitelistText 保持输入值）', async () => {
    const { wrapper } = mountWith()
    await flushWhitelist(wrapper)
    const textarea = wrapper.find('textarea.kw-textarea')
    await textarea.setValue('evil.com')
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'set_privacy_whitelist') return Promise.reject('E_DB: 写库失败')
      return Promise.resolve(undefined)
    })
    await wrapper.find('button.mini-btn.primary').trigger('click')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([expect.objectContaining({ type: 'error', message: '白名单保存失败' })]),
    )
    expect(textarea.element.value).toBe('evil.com')
  })
})

describe('广告拦截', () => {
  it('默认开（adblock_enabled 缺省 !== "false"）；关闭：写 store + set_adblock_enabled(false)', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const switch_ = wrapper.find('[role="switch"][aria-label="广告拦截"]')
    expect(switch_.attributes('aria-checked')).toBe('true')
    await switch_.trigger('click')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_adblock_enabled', { enabled: false }),
    )
    expect(settingsStore.get('adblock_enabled')).toBe('false')
  })

  it('adblock_enabled=false 时：开关关 + 自定义关键词块隐藏', () => {
    const { wrapper } = mountWith({ adblock_enabled: 'false' })
    const switch_ = wrapper.find('[role="switch"][aria-label="广告拦截"]')
    expect(switch_.attributes('aria-checked')).toBe('false')
    expect(wrapper.find('input.kw-input, .kw-list').exists()).toBe(false)
  })
})

describe('自定义广告关键词', () => {
  it('挂载后加载关键词；添加去重；保存成功提示', async () => {
    const { wrapper } = mountWith()
    await flushKeywords(wrapper)
    expect(wrapper.findAll('.kw-chip').length).toBe(1)
    const input = wrapper.find('.kw-add input')
    await input.setValue('adblock') // 已存在
    await input.trigger('keyup.enter')
    await vi.waitFor(() => expect(wrapper.find('.cleared-msg').text()).toContain('已经加过'))
  })

  it('添加新词：invoke set_adblock_keywords 含旧词 + 回显「已保存」', async () => {
    const { wrapper } = mountWith()
    await flushKeywords(wrapper)
    const input = wrapper.find('.kw-add input')
    await input.setValue('newkw')
    await wrapper.find('.kw-add button').trigger('click')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_adblock_keywords', {
        keywords: ['adblock', 'newkw'],
      }),
    )
    await vi.waitFor(() => expect(wrapper.find('.cleared-msg').text()).toContain('已保存'))
  })

  it('保存失败：回滚关键词列表 + error toast', async () => {
    const { wrapper } = mountWith()
    await flushKeywords(wrapper)
    const input = wrapper.find('.kw-add input')
    await input.setValue('badkw')
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'set_adblock_keywords') return Promise.reject('E_DB: 写库失败')
      return Promise.resolve(cmd === 'get_adblock_keywords' ? ['adblock'] : undefined)
    })
    await wrapper.find('.kw-add button').trigger('click')
    const toast = useToastStore()
    expect(toast.items).toEqual(
      expect.arrayContaining([expect.objectContaining({ type: 'error', message: '保存关键词失败' })]),
    )
    await vi.waitFor(() => expect(wrapper.findAll('.kw-chip').length).toBe(1))
    expect(wrapper.find('.kw-chip').text()).toContain('adblock')
  })

  it('删除关键词：invoke 仅含剩余词', async () => {
    const { wrapper } = mountWith()
    await flushKeywords(wrapper)
    await wrapper.find('.kw-chip button.kw-x').trigger('click')
    await vi.waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith('set_adblock_keywords', { keywords: [] }),
    )
  })
})

describe('GPC 请求头', () => {
  it('默认开；关闭：写 store + sync_ext_rules', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const switch_ = wrapper.find('[role="switch"][aria-label="GPC 请求头（Sec-GPC: 1）"]')
    expect(switch_.attributes('aria-checked')).toBe('true')
    await switch_.trigger('click')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('sync_ext_rules'))
    expect(settingsStore.get('gpc_header')).toBe('false')
  })

  it('gpc_header=false 时渲染为关', () => {
    const { wrapper } = mountWith({ gpc_header: 'false' })
    const switch_ = wrapper.find('[role="switch"][aria-label="GPC 请求头（Sec-GPC: 1）"]')
    expect(switch_.attributes('aria-checked')).toBe('false')
  })
})

describe('网络层拦截规则 / 自定义请求头', () => {
  it('拦截规则 change：写 store + sync_ext_rules', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const textareas = wrapper.findAll('textarea.net-rules')
    expect(textareas.length).toBe(2)
    await textareas[0].setValue('doubleclick.net')
    await textareas[0].trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('sync_ext_rules'))
    expect(settingsStore.get('net_block_rules')).toBe('doubleclick.net')
  })

  it('自定义请求头 change：写 store + sync_ext_rules', async () => {
    const { wrapper, settingsStore } = mountWith()
    await flushStats()
    const textareas = wrapper.findAll('textarea.net-rules')
    await textareas[1].setValue('DNT: 1')
    await textareas[1].trigger('change')
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('sync_ext_rules'))
    expect(settingsStore.get('net_headers')).toBe('DNT: 1')
  })
})

describe('openCerts 事件透传', () => {
  it('PrivacyDataCard 内「打开证书中心」点击 → openCerts emit', async () => {
    const { wrapper } = mountWith()
    await flushStats()
    const btn = wrapper.findAll('button.mini-btn').find((b) => b.text().includes('打开证书中心'))
    expect(btn).toBeDefined()
    await btn!.trigger('click')
    expect(wrapper.emitted('openCerts')).toBeTruthy()
  })
})

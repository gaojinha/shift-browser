// PrivacyDataCard 组件测试（F-87 · 前端测试补课）
//
// 手法：mock invoke（get/set_user_agent*、clear_browsing_data）；settings
// store 真实 pinia（clearHistory spy）；toast 走真实 store。attachTo + 卸载
// 清理（F-86 教训）。
//
// 跑法：npx vitest run src/components/settings/PrivacyDataCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import PrivacyDataCard from './PrivacyDataCard.vue'
import { useSettingsStore } from '../../stores/settings'
import { useToastStore } from '../../stores/toast'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)

const MOUNTS: VueWrapper[] = []
function mountCard() {
  const w = mount(PrivacyDataCard, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_user_agent_config')
      return Promise.resolve({ global: 'Mozilla/5.0 custom', default: 'Mozilla/5.0 default', overrides: { 'a.com': 'UA-a' } })
    return Promise.resolve(undefined)
  })
})

describe('PrivacyDataCard · User-Agent 管理', () => {
  it('挂载 get_user_agent_config；输入框回填 global；覆盖列表渲染 host', async () => {
    const w = mountCard()
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('get_user_agent_config')
    expect((w.find('.ua-input').element as HTMLInputElement).value).toBe('Mozilla/5.0 custom')
    expect(w.find('.ua-ov-item .mono').text()).toBe('a.com')
  })

  it('保存 UA：set_user_agent {ua} + toast + 重拉 + msg', async () => {
    const w = mountCard()
    await flushPromises()
    await w.find('.ua-input').setValue('Mozilla/5.0 new')
    // 按文案定位：.history-actions 首个 mini-btn 是「打开证书中心」，不是保存
    const save = w.findAll('button').find((b) => b.text() === '保存')!
    await save.trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('set_user_agent', { ua: 'Mozilla/5.0 new' })
    expect(useToastStore().items.some((t) => t.message === 'User-Agent 已更新')).toBe(true)
    expect(w.find('.cleared-msg').text()).toContain('已保存')
  })

  it('预设按钮：Windows/移动版填入输入框；默认预设回填 config.default', async () => {
    const w = mountCard()
    await flushPromises()
    const presets = w.findAll('.ua-presets .mini-btn')
    await presets[1]!.trigger('click') // Windows 版
    expect((w.find('.ua-input').element as HTMLInputElement).value).toContain('Windows NT 10.0')
    await presets[2]!.trigger('click') // 移动版
    expect((w.find('.ua-input').element as HTMLInputElement).value).toContain('Android 14')
    await presets[0]!.trigger('click') // 默认
    expect((w.find('.ua-input').element as HTMLInputElement).value).toBe('Mozilla/5.0 default')
  })

  it('站点覆盖：空 host → 「请填站点域名」不调；成功后清输入 + 重拉', async () => {
    const w = mountCard()
    await flushPromises()
    const smInputs = w.findAll('.ua-input.sm')
    await smInputs[1]!.setValue('UA-b')
    await w.find('.ua-override-row .mini-btn').trigger('click')
    await flushPromises()
    expect(w.find('.cleared-msg').text()).toContain('请填站点域名')
    expect(invokeMock).not.toHaveBeenCalledWith('set_user_agent_override', expect.anything())

    await smInputs[0]!.setValue('b.com')
    await w.find('.ua-override-row .mini-btn').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('set_user_agent_override', { host: 'b.com', ua: 'UA-b' })
    expect((smInputs[0]!.element as HTMLInputElement).value).toBe('')
  })

  it('覆盖删除：set_user_agent_override {host, ua:""} + 重拉', async () => {
    const w = mountCard()
    await flushPromises()
    await w.find('.btn-x').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('set_user_agent_override', { host: 'a.com', ua: '' })
    expect(invokeMock).toHaveBeenCalledWith('get_user_agent_config')
  })

  it('证书中心按钮 emit("openCerts")', async () => {
    const w = mountCard()
    await flushPromises()
    await w.findAll('.mini-btn')[0]!.trigger('click')
    expect(w.emitted('openCerts')).toHaveLength(1)
  })
})

describe('PrivacyDataCard · 历史与清理', () => {
  it('清除历史：settingsStore.clearHistory + msg（2s 后自动消失）', async () => {
    vi.useFakeTimers()
    try {
      const store = useSettingsStore()
      const spy = vi.spyOn(store, 'clearHistory').mockResolvedValue(undefined)
      const w = mountCard()
      await flushPromises()
      await w.find('.danger-btn').trigger('click')
      await flushPromises()
      expect(spy).toHaveBeenCalledTimes(1)
      expect(w.find('.cleared-msg').text()).toBe('历史记录已清除')
      vi.advanceTimersByTime(2000)
      await flushPromises()
      expect(w.find('.cleared-msg').exists()).toBe(false)
    } finally {
      vi.useRealTimers()
    }
  })

  it('清除浏览数据：默认四项勾选 → clear_browsing_data 全参 + 中文回执 + toast', async () => {
    // 按命令分发：整体覆盖 mock 会把 mount 时的 get_user_agent_config 也换掉，
    // overrides 变 undefined 直接炸渲染（v-if Object.keys(...)）
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'clear_browsing_data') return Promise.resolve(['history', 'cookies', 'cache', 'storage'])
      if (cmd === 'get_user_agent_config')
        return Promise.resolve({ global: '', default: 'Mozilla/5.0 default', overrides: {} })
      return Promise.resolve(undefined)
    })
    const w = mountCard()
    await flushPromises()
    await w.findAll('.danger-btn')[1]!.trigger('click') // 立即清除
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('clear_browsing_data', {
      scopes: ['history', 'cookies', 'cache', 'storage'],
    })
    expect(w.text()).toContain('已清除：历史、Cookie、缓存、本地存储')
    expect(useToastStore().items.some((t) => t.message === '已清除浏览数据')).toBe(true)
  })

  it('全不选 → 「请至少选择一项」不调命令', async () => {
    const w = mountCard()
    await flushPromises()
    const boxes = w.findAll('.clear-scopes input[type="checkbox"]')
    for (const b of boxes.filter((x) => (x.element as HTMLInputElement).checked)) await b.setValue(false)
    await w.findAll('.danger-btn')[1]!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('请至少选择一项')
    expect(invokeMock).not.toHaveBeenCalledWith('clear_browsing_data', expect.anything())
  })

  it('清除失败 → 「清除失败：…」', async () => {
    invokeMock.mockRejectedValue(new Error('db 锁'))
    const w = mountCard()
    await flushPromises()
    await w.findAll('.danger-btn')[1]!.trigger('click')
    await flushPromises()
    expect(w.text()).toContain('清除失败：')
  })
})

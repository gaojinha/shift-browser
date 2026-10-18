// TranslatePanel 组件测试（F-82，王二狗）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia（settings 种子 engine/target）+
// mock Tauri IPC + mock utils/webview + mock navigator.clipboard。
// 兄弟 TranslateProgress.test.ts 覆盖的是「整页汉化进度条」，本面板是「粘贴机翻」，
// 两者不同组件，无重叠。
//
// 覆盖：
//  1.  显隐（show=false 不渲染 / 真值渲染）+ 遮罩点击 self 关闭
//  2.  头部元信息：翻译源（Google/Bing 随设置）+ 目标语言标签（含未知语言回退原文）
//  3.  字符计数 + 翻译按钮禁用条件（空输入 / loading）
//  4.  翻译成功 → 译文渲染；r.ok=false → 错误文案；ok 但 data 空 →「翻译失败」
//  5.  invoke 抛错 → humanizeError 文案
//  6.  loading 期间按钮文案「翻译中…」+ 禁用 + 重复提交被挡
//  7.  清空按钮重置三态
//  8.  翻译当前页面 → aiStore.translatePage(tabId) + emit close；失败 → 错误展示
//  9.  复制译文 → clipboard.writeText +「已复制」1.2s 复原；clipboard 失败走 execCommand 兜底
//  10. 键盘：Esc 关闭 / Ctrl(⌘)+Enter 翻译 / isComposing 全交给输入法
//  11. watch(show)：隐藏/恢复 webview + 打开时自动聚焦输入框
//
// 跑法：npx vitest run src/components/TranslatePanel.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
const setWebviewVisibleMock = vi.fn(() => Promise.resolve())
vi.mock('../utils/webview', () => ({
  setWebviewVisible: (...args: unknown[]) => setWebviewVisibleMock(...args),
}))

import { useSettingsStore } from '../stores/settings'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'
import TranslatePanel from './TranslatePanel.vue'

let pinia: Pinia
let wrapper: VueWrapper | null = null
const writeTextMock = vi.fn(() => Promise.resolve())

/** 挂载面板；settings 可种子 translate_engine / translate_target */
function mountPanel(show = true, settings: Record<string, string> = {}) {
  const settingsStore = useSettingsStore(pinia)
  settingsStore.$patch({ settings: { ...settings }, loaded: true })
  wrapper = mount(TranslatePanel, {
    props: { show },
    global: { plugins: [pinia] },
    // attachTo：组件用 document.querySelector('.tr-textarea').focus() 做自动聚焦，
    // 必须真实挂到 document 才能命中元素
    attachTo: document.body,
  })
  return wrapper
}

const overlay = (w: VueWrapper) => w.find('.tr-overlay')
const textarea = (w: VueWrapper) => w.find('textarea.tr-textarea')
const translateBtn = (w: VueWrapper) => w.find('.tr-btn-primary')

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
  setWebviewVisibleMock.mockClear()
  writeTextMock.mockClear()
  writeTextMock.mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: writeTextMock },
    configurable: true,
  })
})

afterEach(() => {
  wrapper?.unmount()
  wrapper = null
  vi.useRealTimers()
})

describe('TranslatePanel：显隐与头部', () => {
  it('show=false 不渲染；show=true 渲染面板', () => {
    expect(mountPanel(false).find('.tr-panel').exists()).toBe(false)
    expect(mountPanel(true).find('.tr-panel').exists()).toBe(true)
  })

  it('遮罩点击 self → emit close；点面板内部不关', async () => {
    const w = mountPanel(true)
    await w.find('.tr-panel').trigger('click')
    expect(w.emitted('close')).toBeFalsy()
    await overlay(w).trigger('click')
    expect(w.emitted('close')).toBeTruthy()
  })

  it('头部元信息：默认 Google→中文（简体）；设置切换 Bing/en 生效', () => {
    const w = mountPanel(true)
    expect(w.find('.tr-meta').text()).toBe('Google 翻译 · →中文（简体）')

    const w2 = mountPanel(true, { translate_engine: 'bing', translate_target: 'en' })
    expect(w2.find('.tr-meta').text()).toBe('Bing 翻译 · →英语')
  })

  it('目标语言未知值回退原文；空值回退 zh-CN', () => {
    const w = mountPanel(true, { translate_target: 'xx-YY' })
    expect(w.find('.tr-meta').text()).toContain('→xx-YY')
    const w2 = mountPanel(true, { translate_target: '' })
    expect(w2.find('.tr-meta').text()).toContain('→中文（简体）')
  })

  it('footer 显示 TRANSLATE_PANEL_HINT 分工说明', () => {
    const w = mountPanel(true)
    expect(w.find('.tr-footer').text()).toContain('粘贴翻译走机翻')
  })
})

describe('TranslatePanel：输入与按钮', () => {
  it('字符计数随输入变化', async () => {
    const w = mountPanel(true)
    expect(w.find('.tr-count').text()).toBe('0 字符')
    await textarea(w).setValue('hello')
    expect(w.find('.tr-count').text()).toBe('5 字符')
  })

  it('翻译按钮：空输入禁用；有输入可用；loading 时禁用', async () => {
    const w = mountPanel(true)
    expect(translateBtn(w).attributes('disabled')).toBeDefined()

    await textarea(w).setValue('你好')
    expect(translateBtn(w).attributes('disabled')).toBeUndefined()

    // loading 中禁用 + 文案
    let release!: (v: unknown) => void
    invokeMock.mockReturnValue(new Promise((res) => (release = res)))
    await translateBtn(w).trigger('click')
    expect(translateBtn(w).text()).toBe('⏳ 翻译中…')
    expect(translateBtn(w).attributes('disabled')).toBeDefined()
    release({ ok: true, data: 'hi' })
    await flushPromises()
  })

  it('空白输入不触发翻译（trim 后为空）', async () => {
    const w = mountPanel(true)
    await textarea(w).setValue('   ')
    // 按钮仍是禁用态
    expect(translateBtn(w).attributes('disabled')).toBeDefined()
    await translateBtn(w).trigger('click')
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('TranslatePanel：翻译结果', () => {
  it('成功：按 {text,target} 调 translate_text，渲染译文', async () => {
    invokeMock.mockResolvedValue({ ok: true, data: 'Hello world' })
    const w = mountPanel(true)
    await textarea(w).setValue('你好世界')
    await translateBtn(w).trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('translate_text', { text: '你好世界', target: 'zh-CN' })
    expect(w.find('.tr-result').text()).toBe('Hello world')
    expect(w.find('.tr-empty').exists()).toBe(false)
  })

  it('r.ok=false：错误显示 r.msg', async () => {
    invokeMock.mockResolvedValue({ ok: false, msg: '限流了' })
    const w = mountPanel(true)
    await textarea(w).setValue('x')
    await translateBtn(w).trigger('click')
    await flushPromises()
    expect(w.find('.tr-error').text()).toBe('限流了')
  })

  it('r.ok=true 但 data 为空：回退「翻译失败」', async () => {
    invokeMock.mockResolvedValue({ ok: true, data: '' })
    const w = mountPanel(true)
    await textarea(w).setValue('x')
    await translateBtn(w).trigger('click')
    await flushPromises()
    expect(w.find('.tr-error').text()).toBe('翻译失败')
  })

  it('invoke 抛错：humanizeError 文案（E_TIMEOUT 人话）', async () => {
    invokeMock.mockRejectedValue({ code: 'E_TIMEOUT', message: 'timeout' })
    const w = mountPanel(true)
    await textarea(w).setValue('x')
    await translateBtn(w).trigger('click')
    await flushPromises()
    // humanizeError 会把 code 映射为人话，并把原始 message 附在后面
    expect(w.find('.tr-error').text()).toContain('timeout')
  })

  it('译文为空且非 loading 时显示占位「译文会显示在这里」', async () => {
    const w = mountPanel(true)
    expect(w.find('.tr-empty').text()).toBe('译文会显示在这里')
  })

  it('清空按钮重置输入/译文/错误', async () => {
    invokeMock.mockResolvedValue({ ok: true, data: '译文' })
    const w = mountPanel(true)
    await textarea(w).setValue('原文')
    await translateBtn(w).trigger('click')
    await flushPromises()
    expect(w.find('.tr-result').exists()).toBe(true)

    const clearBtn = w.findAll('.tr-btn').find((b) => b.text() === '清空')!
    await clearBtn.trigger('click')
    expect((textarea(w).element as HTMLTextAreaElement).value).toBe('')
    expect(w.find('.tr-result').exists()).toBe(false)
    expect(w.find('.tr-empty').text()).toBe('译文会显示在这里')
  })
})

describe('TranslatePanel：整页翻译与复制', () => {
  it('翻译当前页面 → aiStore.translatePage(activeTabId) + emit close', async () => {
    const w = mountPanel(true)
    const tabs = useTabsStore(pinia)
    tabs.$patch({ tabs: [{ id: 't1', url: 'https://a.com', title: 'a' }], activeTabId: 't1' } as never)
    const ai = useAIStore(pinia)
    const spy = vi.spyOn(ai, 'translatePage').mockResolvedValue(undefined)

    const pageBtn = w.findAll('.tr-btn').find((b) => b.text().includes('翻译当前页面'))!
    await pageBtn.trigger('click')
    await flushPromises()

    expect(spy).toHaveBeenCalledWith('t1')
    expect(w.emitted('close')).toBeTruthy()
  })

  it('整页翻译失败 → 面板内展示错误、不关闭', async () => {
    const w = mountPanel(true)
    const ai = useAIStore(pinia)
    vi.spyOn(ai, 'translatePage').mockRejectedValue({ code: 'E_NET', message: 'offline' })

    const pageBtn = w.findAll('.tr-btn').find((b) => b.text().includes('翻译当前页面'))!
    await pageBtn.trigger('click')
    await flushPromises()

    expect(w.find('.tr-error').text()).toContain('offline')
    expect(w.emitted('close')).toBeFalsy()
  })

  it('复制译文：clipboard.writeText + 「已复制」1.2s 后复原', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
    invokeMock.mockResolvedValue({ ok: true, data: 'Hello' })
    const w = mountPanel(true)
    await textarea(w).setValue('你好')
    await translateBtn(w).trigger('click')
    await flushPromises()

    const copyBtn = w.find('.tr-copy')
    await copyBtn.trigger('click')
    await flushPromises()
    expect(writeTextMock).toHaveBeenCalledWith('Hello')
    expect(w.find('.tr-copy').text()).toBe('✅ 已复制')

    vi.advanceTimersByTime(1200)
    await flushPromises()
    expect(w.find('.tr-copy').text()).toBe('📋 复制')
  })

  it('clipboard 失败 → execCommand 兜底仍标记已复制', async () => {
    writeTextMock.mockRejectedValueOnce(new Error('denied'))
    const execMock = vi.fn(() => true)
    Object.defineProperty(document, 'execCommand', { value: execMock, configurable: true })

    invokeMock.mockResolvedValue({ ok: true, data: 'Hello' })
    const w = mountPanel(true)
    await textarea(w).setValue('你好')
    await translateBtn(w).trigger('click')
    await flushPromises()
    await w.find('.tr-copy').trigger('click')
    await flushPromises()

    expect(execMock).toHaveBeenCalledWith('copy')
    expect(w.find('.tr-copy').text()).toBe('✅ 已复制')
  })

  it('无译文时不渲染复制按钮', () => {
    const w = mountPanel(true)
    expect(w.find('.tr-copy').exists()).toBe(false)
  })
})

describe('TranslatePanel：键盘', () => {
  it('Esc → emit close', async () => {
    const w = mountPanel(true)
    await overlay(w).trigger('keydown', { key: 'Escape' })
    expect(w.emitted('close')).toBeTruthy()
  })

  it('Ctrl+Enter → 触发翻译', async () => {
    invokeMock.mockResolvedValue({ ok: true, data: 'hi' })
    const w = mountPanel(true)
    await textarea(w).setValue('你好')
    await overlay(w).trigger('keydown', { key: 'Enter', ctrlKey: true })
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('translate_text', { text: '你好', target: 'zh-CN' })
  })

  it('⌘+Enter（metaKey）同样触发翻译', async () => {
    invokeMock.mockResolvedValue({ ok: true, data: 'hi' })
    const w = mountPanel(true)
    await textarea(w).setValue('hello')
    await overlay(w).trigger('keydown', { key: 'Enter', metaKey: true })
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('translate_text', { text: 'hello', target: 'zh-CN' })
  })

  it('组字中（isComposing）：Esc 与 Ctrl+Enter 都交给输入法', async () => {
    const w = mountPanel(true)
    await textarea(w).setValue('ni')
    await overlay(w).trigger('keydown', { key: 'Escape', isComposing: true })
    await overlay(w).trigger('keydown', { key: 'Enter', ctrlKey: true, isComposing: true })
    await flushPromises()
    expect(w.emitted('close')).toBeFalsy()
    expect(invokeMock).not.toHaveBeenCalled()
  })
})

describe('TranslatePanel：webview 避让与聚焦', () => {
  it('watch(show) 无 immediate：翻 true → setWebviewVisible(false) 且自动聚焦；翻 false → 恢复', async () => {
    // 挂载态不触发（无 immediate）
    const w = mountPanel(false)
    expect(setWebviewVisibleMock).not.toHaveBeenCalled()

    w.setProps({ show: true })
    await nextTick()
    await nextTick()
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(false, 'translate-panel')
    const active = document.activeElement as HTMLElement | null
    expect(active?.classList.contains('tr-textarea')).toBe(true)

    setWebviewVisibleMock.mockClear()
    w.setProps({ show: false })
    await nextTick()
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(true, 'translate-panel')
  })
})

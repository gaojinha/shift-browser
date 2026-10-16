// AISidebar 组件测试（F-37 前端补课）
//
// 手法：@vue/test-utils 真实挂载（真实 pinia ai/tabs/settings store）+ mock Tauri
// （core invoke/Channel、event listen）+ stub navigator.clipboard。
// 聚焦可测面：show 开关与 webview 避让、topOffset 布局常量四组合、汉化进度条、
// 发送守卫（空/loading/失败回填/随手问分流）、Enter 口径、停止模式分流、
// 代码块复制事件委托、Agent 动作/多步确认卡片、技能按钮数据驱动。
//
// 跑法：npx vitest run src/components/AISidebar.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { nextTick } from 'vue'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import AISidebar from './AISidebar.vue'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import { EVENTS, SKILLS } from '../constants'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class Channel {
    constructor(public callback: (e: unknown) => void) {}
  },
}))
const listenMock = vi.fn()
vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}))

const writeTextMock = vi.fn().mockResolvedValue(undefined)

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation(() => Promise.resolve(undefined as unknown))
  listenMock.mockReset()
  listenMock.mockImplementation(() => Promise.resolve(vi.fn()))
  writeTextMock.mockReset()
  writeTextMock.mockResolvedValue(undefined)
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText: writeTextMock },
    configurable: true,
  })
})

async function mountSidebar(props: { show: boolean } = { show: true }) {
  const w = mount(AISidebar, {
    props,
    global: { plugins: [pinia] },
  })
  await flushPromises()
  return w
}

/** 取出组件 onMounted 时注册的 Tauri 事件处理器（按事件名） */
function getListener(event: string) {
  const call = listenMock.mock.calls.find((c) => c[0] === event)
  return call?.[1] as ((e: { payload: unknown }) => void) | undefined
}

describe('AISidebar：开关与 webview 避让', () => {
  it('show=false 不渲染面板；翻 true 渲染并通知 webview 让宽', async () => {
    const w = await mountSidebar({ show: false })
    expect(w.find('.ai-sidebar').exists()).toBe(false)
    await w.setProps({ show: true })
    await flushPromises()
    expect(w.find('.ai-sidebar').exists()).toBe(true)
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === 'set_webview_ai_panel' && (c[1] as Record<string, unknown>)?.active === true,
      ),
    ).toBe(true)
    w.unmount()
  })

  it('打开时同步活动标签上下文给 AI', async () => {
    const w = await mountSidebar({ show: false })
    const tabsStore = useTabsStore()
    tabsStore.$patch({
      tabs: [
        { id: 't1', url: 'https://example.com/a', title: 'Example', is_active: true } as never,
      ],
      activeTabId: 't1',
    })
    const aiStore = useAIStore()
    await w.setProps({ show: true })
    await flushPromises()
    expect(aiStore.pageContext).toEqual({ url: 'https://example.com/a', title: 'Example' })
    w.unmount()
  })

  it('有待发问时打开即发问（takePendingPrompt 消费并清空）', async () => {
    const w = await mountSidebar({ show: false })
    const aiStore = useAIStore()
    const send = vi.spyOn(aiStore, 'sendMessage').mockResolvedValue(undefined)
    aiStore.queuePrompt('  问问这页  ')
    await w.setProps({ show: true })
    await flushPromises()
    expect(send).toHaveBeenCalledWith('问问这页')
    expect(aiStore.pendingPrompt).toBeNull()
    w.unmount()
  })

  it('关闭按钮与遮罩点击都发 close', async () => {
    const w = await mountSidebar()
    await w.find('.ai-close').trigger('click')
    expect(w.emitted('close')?.length).toBe(1)
    await w.find('.ai-overlay').trigger('click')
    expect(w.emitted('close')?.length).toBe(2)
    w.unmount()
  })
})

describe('AISidebar：布局偏移与汉化进度', () => {
  it('内容区顶部偏移四组合（与 LAYOUT 常量同算法，防漂移）', async () => {
    // 注意：$patch 对对象是深合并，每轮两个键都显式给，防上一轮残留
    const cases: Array<[Record<string, string>, string]> = [
      [{ vertical_tabs: 'false', show_bookmarks_bar: 'true' }, '100px'], // 水平 + 书签栏：38+36+26
      [{ vertical_tabs: 'false', show_bookmarks_bar: 'false' }, '74px'], // 水平无书签栏：38+36
      [{ vertical_tabs: 'true', show_bookmarks_bar: 'true' }, '62px'], // 垂直 + 书签栏：0+36+26
      [{ vertical_tabs: 'true', show_bookmarks_bar: 'false' }, '36px'], // 垂直无书签栏
    ]
    for (const [seed, top] of cases) {
      const w = await mountSidebar()
      useSettingsStore().$patch({ settings: { ...seed } })
      await nextTick()
      const style = (w.find('.ai-sidebar').element as HTMLElement).style.top
      expect(style).toBe(top)
      w.unmount()
    }
  })

  it('汉化进度条宽度按段数算（四舍五入、封顶 100、total=0 为 0）', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    aiStore.$patch({
      zhBusy: true,
      zhProgress: { done: 1, total: 3, batch: 1, batches: 2, preview: 'Submit' },
    })
    await nextTick()
    let fill = w.find('.zh-bar-fill').element as HTMLElement
    expect(fill.style.width).toBe('33%')
    aiStore.$patch({
      zhProgress: { done: 9, total: 3, batch: 2, batches: 2, preview: '' },
    })
    await nextTick()
    fill = w.find('.zh-bar-fill').element as HTMLElement
    expect(fill.style.width).toBe('100%')
    aiStore.$patch({
      zhProgress: { done: 0, total: 0, batch: 0, batches: 0, preview: '' },
    })
    await nextTick()
    fill = w.find('.zh-bar-fill').element as HTMLElement
    expect(fill.style.width).toBe('0%')
    w.unmount()
  })
})

describe('AISidebar：发送守卫', () => {
  it('空输入不发、loading 不发且不清框、失败回填原文', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    const send = vi.spyOn(aiStore, 'sendMessage').mockResolvedValue(undefined)
    const ta = w.find('textarea')
    // 空 → 发送按钮禁用
    expect((w.find('.ai-send').element as HTMLButtonElement).disabled).toBe(true)
    // 纯空白 → 不发、不清
    await ta.setValue('   ')
    await w.find('.ai-send').trigger('click')
    expect(send).not.toHaveBeenCalled()
    expect((ta.element as HTMLTextAreaElement).value).toBe('   ')
    // loading → 不发、不清（发送按钮已换成停止，Enter 仍走 onSend 守卫）
    aiStore.$patch({ loading: true })
    await ta.setValue('草稿')
    expect(w.find('.ai-send').exists()).toBe(false)
    expect(w.find('.ai-stop').exists()).toBe(true)
    await ta.trigger('keydown', { key: 'Enter', shiftKey: false, isComposing: false })
    expect(send).not.toHaveBeenCalled()
    expect((ta.element as HTMLTextAreaElement).value).toBe('草稿')
    // 正常发送 → 清输入框
    aiStore.$patch({ loading: false })
    await ta.setValue('hello')
    await w.find('.ai-send').trigger('click')
    expect(send).toHaveBeenCalledWith('hello')
    expect((ta.element as HTMLTextAreaElement).value).toBe('')
    // 失败 → 原文回填
    send.mockImplementation(async () => {
      aiStore.error = '网络错误'
    })
    await ta.setValue('重要问题')
    await w.find('.ai-send').trigger('click')
    expect((ta.element as HTMLTextAreaElement).value).toBe('重要问题')
    w.unmount()
  })

  it('随手问模式走本机 askPage，不走云端 sendMessage', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    const askPage = vi.spyOn(aiStore, 'askPage').mockResolvedValue(undefined)
    const send = vi.spyOn(aiStore, 'sendMessage').mockResolvedValue(undefined)
    aiStore.$patch({ webqaMode: true })
    await w.find('textarea').setValue('本地问题')
    await w.find('.ai-send').trigger('click')
    expect(askPage).toHaveBeenCalledWith('本地问题')
    expect(send).not.toHaveBeenCalled()
    w.unmount()
  })

  it('Enter 发送、Shift+Enter 不发、组字中不发', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    const send = vi.spyOn(aiStore, 'sendMessage').mockResolvedValue(undefined)
    const ta = w.find('textarea')
    await ta.setValue('q')
    await ta.trigger('keydown', { key: 'Enter', shiftKey: false, isComposing: true })
    expect(send).not.toHaveBeenCalled()
    await ta.trigger('keydown', { key: 'Enter', shiftKey: true, isComposing: false })
    expect(send).not.toHaveBeenCalled()
    await ta.trigger('keydown', { key: 'Enter', shiftKey: false, isComposing: false })
    expect(send).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('停止按钮按模式分流：云端 ai_cancel_stream / 本机 opencode_cancel', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    aiStore.$patch({ loading: true, streamingId: 'req-1', webqaMode: false })
    await flushPromises()
    // onStop 不 await store 动作，断言前 flush 让 invoke 微任务跑完
    await w.find('.ai-stop').trigger('click')
    await flushPromises()
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === 'ai_cancel_stream' && (c[1] as Record<string, unknown>)?.requestId === 'req-1',
      ),
    ).toBe(true)
    // 如实记录口径：云端 stopStreaming 只清 streamingId + 发取消命令，
    // loading 由后端流结束事件兜底重置（本地 stopWebqa 才同步置 false）
    expect(aiStore.streamingId).toBeNull()

    invokeMock.mockClear()
    aiStore.$patch({ loading: true, streamingId: 'req-2', webqaMode: true })
    await flushPromises()
    await w.find('.ai-stop').trigger('click')
    await flushPromises()
    expect(
      invokeMock.mock.calls.some(
        (c) => c[0] === 'opencode_cancel' && (c[1] as Record<string, unknown>)?.requestId === 'req-2',
      ),
    ).toBe(true)
    w.unmount()
  })
})

describe('AISidebar：消息区', () => {
  it('空态提示；user 消息纯文本（HTML 不解析）；assistant 走 Markdown；清空对话', async () => {
    const w = await mountSidebar()
    expect(w.find('.ai-empty').exists()).toBe(true)
    const aiStore = useAIStore()
    aiStore.$patch({
      messages: [
        { role: 'user', content: '<img src=x onerror=alert(1)>' },
        { role: 'assistant', content: 'ok **bold**' },
      ],
    })
    await nextTick()
    expect(w.find('.ai-empty').exists()).toBe(false)
    const userMsg = w.find('.ai-msg.user')
    expect(userMsg.text()).toContain('<img src=x onerror=alert(1)>')
    expect(userMsg.find('img').exists()).toBe(false)
    const asstMsg = w.find('.ai-msg.assistant')
    expect(asstMsg.find('strong').exists()).toBe(true)
    expect(asstMsg.html()).not.toContain('onerror')
    await w.find('.ai-clear').trigger('click')
    expect(aiStore.messages.length).toBe(0)
    await nextTick()
    expect(w.find('.ai-empty').exists()).toBe(true)
    w.unmount()
  })

  it('代码块复制：data-code 解码写剪贴板，成功反馈"已复制 ✓"', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    const code = 'const x = 1 // <>&"'
    aiStore.$patch({
      messages: [{ role: 'assistant', content: '```js\n' + code + '\n```' }],
    })
    await nextTick()
    const btn = w.find('.code-copy')
    expect(btn.exists()).toBe(true)
    expect(btn.attributes('data-code')).toBe(encodeURIComponent(code))
    await btn.trigger('click')
    expect(writeTextMock).toHaveBeenCalledWith(code)
    expect(btn.text()).toBe('已复制 ✓')
    w.unmount()
  })

  it('复制失败（剪贴板被拒且 execCommand 不可用）反馈"复制失败"', async () => {
    writeTextMock.mockRejectedValueOnce(new Error('denied'))
    const w = await mountSidebar()
    const aiStore = useAIStore()
    aiStore.$patch({
      messages: [{ role: 'assistant', content: '```js\nconst y = 2\n```' }],
    })
    await nextTick()
    const btn = w.find('.code-copy')
    await btn.trigger('click')
    expect(btn.text()).toBe('复制失败')
    w.unmount()
  })
})

describe('AISidebar：Agent 确认卡片', () => {
  it('动作请求 → 卡片展示 → 拒绝/允许回调带 requestId', async () => {
    const w = await mountSidebar()
    const handler = getListener(EVENTS.AGENT_ACTION_REQUEST)
    expect(handler).toBeTruthy()
    handler!({ payload: { requestId: 'r1', description: '点击「确认」按钮' } })
    await flushPromises()
    expect(w.find('.agent-confirm').exists()).toBe(true)
    expect(w.find('.agent-confirm-desc').text()).toBe('点击「确认」按钮')
    await w.find('.agent-confirm-deny').trigger('click')
    expect(
      invokeMock.mock.calls.some(
        (c) =>
          c[0] === 'agent_confirm_action' &&
          (c[1] as Record<string, unknown>)?.requestId === 'r1' &&
          (c[1] as Record<string, unknown>)?.approved === false,
      ),
    ).toBe(true)
    expect(w.find('.agent-confirm').exists()).toBe(false)
    // 再来一次允许
    handler!({ payload: { requestId: 'r2', description: '输入搜索词' } })
    await flushPromises()
    await w.find('.agent-confirm-allow').trigger('click')
    expect(
      invokeMock.mock.calls.some(
        (c) =>
          c[0] === 'agent_confirm_action' &&
          (c[1] as Record<string, unknown>)?.requestId === 'r2' &&
          (c[1] as Record<string, unknown>)?.approved === true,
      ),
    ).toBe(true)
    w.unmount()
  })

  it('多步任务请求 → 步骤列表渲染 → 允许全部回调', async () => {
    const w = await mountSidebar()
    const handler = getListener(EVENTS.AGENT_MULTI_STEP_REQUEST)
    expect(handler).toBeTruthy()
    handler!({
      payload: {
        requestId: 'm1',
        description: '两步完成结账',
        steps: [
          { step: 1, action: 'click', selector: '#buy' },
          { step: 2, action: 'type', selector: '#addr', value: '北京' },
        ],
      },
    })
    await flushPromises()
    const items = w.findAll('.multi-step-item')
    expect(items.length).toBe(2)
    expect(items[0].text()).toContain('#buy')
    expect(items[1].text()).toContain('北京')
    await w.find('.agent-confirm-allow').trigger('click')
    expect(
      invokeMock.mock.calls.some(
        (c) =>
          c[0] === 'agent_confirm_multi_step' &&
          (c[1] as Record<string, unknown>)?.requestId === 'm1' &&
          (c[1] as Record<string, unknown>)?.approved === true,
      ),
    ).toBe(true)
    w.unmount()
  })
})

describe('AISidebar：技能按钮', () => {
  it('按钮由 SKILLS 表数据驱动；点击分发对应 store 动作', async () => {
    const w = await mountSidebar()
    const aiStore = useAIStore()
    const summarize = vi.spyOn(aiStore, 'summarizeWithContext').mockResolvedValue(undefined)
    const translate = vi.spyOn(aiStore, 'translatePage').mockResolvedValue(undefined)
    const explain = vi.spyOn(aiStore, 'explainWithContext').mockResolvedValue(undefined)
    const btns = w.findAll('.ai-action')
    expect(btns.length).toBe(SKILLS.length)
    await btns[0].trigger('click') // summarize_page
    expect(summarize).toHaveBeenCalledTimes(1)
    await btns[1].trigger('click') // translate_page
    expect(translate).toHaveBeenCalledTimes(1)
    await btns[2].trigger('click') // explain_page
    expect(explain).toHaveBeenCalledTimes(1)
    w.unmount()
  })

  it('汉化进行中 translate_page 按钮禁用', async () => {
    const w = await mountSidebar()
    useAIStore().$patch({ zhBusy: true })
    await nextTick()
    const translateBtn = w.findAll('.ai-action')[1].element as HTMLButtonElement
    expect(translateBtn.disabled).toBe(true)
    w.unmount()
  })
})

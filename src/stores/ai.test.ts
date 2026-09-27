// ai store 单测（R1 测试补课：流式回退 / 工具路径 / 错误契约）
// 契约覆盖：
//  - 有 MCP 工具 → 非流式 ai_chat；无工具 → ai_chat_stream + Channel 事件
//  - delta 增量追加、done 完整兜底、error 事件置错并丢空占位
//  - 「已停止生成」不算错误；invoke reject 走 errText（兼容字符串与 CommandError 对象）
//  - 掩码 API key（含 ****）绝不对后端回传
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
// 捕获 store 内 new Channel() 实例，测试手动触发 onmessage
let lastChannel: { onmessage: ((ev: unknown) => void) | null } | null = null
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
  Channel: class {
    onmessage: ((ev: unknown) => void) | null = null
    constructor() {
      // eslint-disable-next-line @typescript-eslint/no-this-alias
      lastChannel = this
    }
  },
}))

import { useAIStore } from './ai'
import { useSettingsStore } from './settings'

/** 默认 invoke 行为：快照失败（走降级）、无工具；按命令覆盖 */
function mockDefaultInvoke() {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'content_snapshot') return Promise.reject('no active tab')
    if (cmd === 'mcp_list_tools') return Promise.resolve([])
    if (cmd === 'ai_chat_stream') return Promise.resolve(undefined)
    return Promise.resolve(undefined)
  })
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  lastChannel = null
  mockDefaultInvoke()
})

describe('sendMessage 路径选择', () => {
  it('有 MCP 工具 → 走非流式 ai_chat，回复写入 assistant 占位', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 'reader', serverName: 's', description: 'd' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('工具路径回复')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('总结一下')
    expect(invokeMock).toHaveBeenCalledWith(
      'ai_chat',
      expect.objectContaining({ provider: 'agnes', model: 'agnes-2.0-flash' }),
    )
    expect(invokeMock).not.toHaveBeenCalledWith('ai_chat_stream', expect.anything())
    expect(ai.messages).toEqual([
      { role: 'user', content: '总结一下' },
      { role: 'assistant', content: '工具路径回复' },
    ])
    expect(ai.loading).toBe(false)
    expect(ai.streamingId).toBeNull()
  })

  it('无工具 → 走流式 ai_chat_stream，delta 追加 + done 完整兜底', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') {
        // onmessage 在 invoke 前已挂好，同步触发事件
        lastChannel?.onmessage?.({ type: 'delta', content: '你好' })
        lastChannel?.onmessage?.({ type: 'delta', content: '，世界' })
        lastChannel?.onmessage?.({ type: 'done', full: '你好，世界！' })
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    expect(invokeMock).toHaveBeenCalledWith(
      'ai_chat_stream',
      expect.objectContaining({ requestId: expect.any(String) }),
    )
    expect(ai.messages[1]).toEqual({ role: 'assistant', content: '你好，世界！' })
    expect(ai.loading).toBe(false)
    expect(ai.streamingId).toBeNull()
  })

  it('空消息与 loading 中的并发发送被忽略', async () => {
    const ai = useAIStore()
    await ai.sendMessage('   ')
    expect(ai.messages).toHaveLength(0)
    ai.loading = true
    await ai.sendMessage('不应发出')
    expect(ai.messages).toHaveLength(0)
    expect(invokeMock).not.toHaveBeenCalledWith('ai_chat_stream', expect.anything())
  })
})

describe('流式错误与停止', () => {
  it('error 事件：置 error、丢空占位、loading 归位', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') {
        lastChannel?.onmessage?.({ type: 'error', message: 'AI 服务错误 500' })
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    // 人话化后含原文（内部 hiccup 话术 + 原消息括注）
    expect(ai.error).toContain('AI 服务错误 500')
    // 空 assistant 占位被丢弃，只剩用户消息
    expect(ai.messages).toEqual([{ role: 'user', content: 'hi' }])
    expect(ai.loading).toBe(false)
    expect(ai.streamingId).toBeNull()
  })

  it('「已停止生成」不算错误：不置 error，仍丢空占位', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') {
        lastChannel?.onmessage?.({ type: 'error', message: '已停止生成' })
        return Promise.resolve(undefined)
      }
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    expect(ai.error).toBeNull()
    expect(ai.messages).toEqual([{ role: 'user', content: 'hi' }])
  })

  it('invoke reject 裸字符串（旧命令形态）→ 人话话术 + 原文括注', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') return Promise.reject('网络不可达')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    expect(ai.error).toContain('网络不可达')
    expect(ai.messages).toEqual([{ role: 'user', content: 'hi' }])
    expect(ai.loading).toBe(false)
  })

  it('invoke reject CommandError 对象（新契约）→ error 取 message 字段而非 [object Object]', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') {
        return Promise.reject({ code: 'E_AUTH', message: '未配置 API Key' })
      }
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    // 新契约对象的人话化：E_AUTH 话术 + 原消息括注，不再是 [object Object]
    expect(ai.error).toContain('未配置 API Key')
  })

  it('stopStreaming：按 requestId 调 ai_cancel_stream，取消失败静默', async () => {
    let resolveStream: (v: unknown) => void = () => {}
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') {
        return new Promise((r) => { resolveStream = r }) // 流式挂起不返回
      }
      if (cmd === 'ai_cancel_stream') return Promise.reject('already gone')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    const sending = ai.sendMessage('hi')
    // 等 ai_chat_stream 被调用（streamingId 在此之前已设置）
    await vi.waitFor(() => {
      expect(invokeMock.mock.calls.some((c) => c[0] === 'ai_chat_stream')).toBe(true)
    })
    const streamCall = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat_stream')!
    const reqId = (streamCall[1] as { requestId: string }).requestId
    expect(ai.streamingId).toBe(reqId)
    await ai.stopStreaming()
    const cancelCalls = invokeMock.mock.calls.filter((c) => c[0] === 'ai_cancel_stream')
    expect(cancelCalls).toHaveLength(1)
    expect(cancelCalls[0]).toEqual(['ai_cancel_stream', { requestId: reqId }])
    expect(ai.streamingId).toBeNull()
    // 释放悬挂的 sendMessage
    resolveStream(undefined)
    await sending
  })
})

describe('请求参数契约', () => {
  it('掩码 API key（含 ****）不回传；快照成功时系统提示词含语义摘要', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') {
        return Promise.resolve({
          title: '示例页',
          url: 'https://example.com',
          lang: 'zh',
          text: '正文内容',
          interactive: { links: 2, buttons: 1, inputs: 0 },
          forms: [],
          links: [],
          webmcp_tools: [],
          aria: [],
        })
      }
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const settings = useSettingsStore()
    settings.settings.ai_api_key = 'abcd****wxyz'
    const ai = useAIStore()
    await ai.sendMessage('分析这个页面')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    expect(call).toBeTruthy()
    const args = call![1] as {
      apiKey?: string
      baseUrl?: string
      messages: Array<{ role: string; content: string }>
    }
    // 掩码 key 必须被吞掉（后端回退环境变量）
    expect(args.apiKey).toBeUndefined()
    // baseUrl 有默认值时透传
    expect(args.baseUrl).toBe('https://apihub.agnes-ai.com/v1')
    // 系统提示词含页面语义快照（标题 + 可交互元素统计）
    expect(args.messages[0].role).toBe('system')
    expect(args.messages[0].content).toContain('示例页')
    expect(args.messages[0].content).toContain('链接 2 个')
    // 历史不含最后的 assistant 占位
    expect(args.messages.some((m) => m.role === 'assistant' && m.content === '')).toBe(false)
  })
})

describe('pendingPrompt 暂存', () => {
  it('queuePrompt/takePendingPrompt 往返，取出后清空；空白不入队', () => {
    const ai = useAIStore()
    ai.queuePrompt('  解释这个页面  ')
    expect(ai.pendingPrompt).toBe('解释这个页面')
    expect(ai.takePendingPrompt()).toBe('解释这个页面')
    expect(ai.takePendingPrompt()).toBeNull()
    ai.queuePrompt('   ')
    expect(ai.pendingPrompt).toBeNull()
  })
})

// ── 网页随手问（P3）：本机 opencode 链路 ──
// 契约覆盖：
//  - opencode_ask 按结构体传参（req.xxx），与 AGNES 的位置参数路径互不干扰
//  - session 事件回收 sessionId 并用于后续追问（多轮上下文）
//  - delta 增量追加、done 用 full 权威兜底
//  - captureSelection 三态：有选区 / rejected 敏感区 / hibernated 休眠
//  - probe 报"只读 agent 不可用"时前端能看见（后端已拦下发问，这里只管透传）
describe('网页随手问：askPage 走本机 opencode', () => {
  it('按结构体传参并回传选区上下文', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({
          has_selection: true,
          text: '选中的正文',
          title: '页面标题',
          url: 'https://example.com/a',
          hibernated: false,
        })
      if (cmd === 'opencode_ask') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.captureSelection('tab-1')
    expect(ai.selection?.text).toBe('选中的正文')

    await ai.askPage('这段讲了什么')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'opencode_ask')
    expect(call).toBeTruthy()
    const args = call![1] as { req: Record<string, unknown>; onEvent: unknown }
    expect(args.req.question).toBe('这段讲了什么')
    expect(args.req.context).toBe('选中的正文')
    expect(args.req.pageTitle).toBe('页面标题')
    expect(args.req.pageUrl).toBe('https://example.com/a')
    expect(args.onEvent).toBeTruthy()
  })

  it('session 事件回收 sessionId，后续追问带上它（多轮上下文）', async () => {
    mockDefaultInvoke()
    const ai = useAIStore()
    // 先手动收尾第一问（真实运行里由 done/error 事件驱动，测试里只关注 sessionId 回收）
    const p1 = ai.askPage('第一问')
    lastChannel?.onmessage?.({ type: 'session', sessionId: 'ses_abc' })
    expect(ai.webqaSessionId).toBe('ses_abc')
    lastChannel?.onmessage?.({ type: 'done', full: '答', sessionId: 'ses_abc' })
    await p1

    await ai.askPage('第二问')
    const calls = invokeMock.mock.calls.filter((c) => c[0] === 'opencode_ask')
    const args = calls[calls.length - 1][1] as { req: Record<string, unknown> }
    expect(args.req.sessionId).toBe('ses_abc')
  })

  it('50 轮封顶：超轮另开新会话，计数归零', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'opencode_ask') return Promise.resolve(undefined)
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.webqaSessionId = 'ses_old'
    ai.webqaRounds = 50
    await ai.askPage('第 51 问')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'opencode_ask')
    const args = call![1] as { req: Record<string, unknown> }
    // 旧会话丢弃，本轮空会话开新，计数从 1 起
    expect(args.req.sessionId).toBeNull()
    expect(ai.webqaRounds).toBe(1)
    // 未超轮照常复用（mock 不发 done 事件，手动放行 loading）
    ai.loading = false
    ai.webqaSessionId = 'ses_keep'
    await ai.askPage('追问')
    const calls = invokeMock.mock.calls.filter((c) => c[0] === 'opencode_ask')
    const args2 = calls[calls.length - 1][1] as { req: Record<string, unknown> }
    expect(args2.req.sessionId).toBe('ses_keep')
    expect(ai.webqaRounds).toBe(2)
  })

  it('delta 增量追加、done 以 full 权威覆盖', async () => {
    mockDefaultInvoke()
    const ai = useAIStore()
    await ai.askPage('问')
    lastChannel?.onmessage?.({ type: 'delta', content: '前半' })
    lastChannel?.onmessage?.({ type: 'delta', content: '后半' })
    expect(ai.messages[ai.messages.length - 1].content).toBe('前半后半')
    // full 是权威值（delta 可能重复到达）
    lastChannel?.onmessage?.({ type: 'done', full: '前半后半（修正）', sessionId: 'ses_x' })
    expect(ai.messages[ai.messages.length - 1].content).toBe('前半后半（修正）')
    expect(ai.loading).toBe(false)
    expect(ai.webqaSessionId).toBe('ses_x')
  })

  it('敏感区 rejected：明确报错且不带入任何选区内容', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({ has_selection: false, rejected: true, code: 'secret_region' })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.captureSelection('tab-1')
    expect(ai.selection).toBeNull()
    expect(ai.error).toContain('敏感')
  })

  it('hibernated：提示唤醒，不当成"用户没选东西"', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({ has_selection: false, hibernated: true, hints: ['标签已休眠，切回可再读'] })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.captureSelection('tab-1')
    expect(ai.selection).toBeNull()
    expect(ai.error).toContain('休眠')
  })

  it('真无选区：不报错（正常情形）', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({ has_selection: false, hibernated: false })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.captureSelection('tab-1')
    expect(ai.selection).toBeNull()
    expect(ai.error).toBeNull()
  })

  it('probe 结果透传：只读 agent 缺失时前端能看见风险标记', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'opencode_probe')
        return Promise.resolve({
          ok: true,
          baseUrl: 'http://127.0.0.1:4096',
          model: 'opencode/big-pickle',
          defaultAgent: '个人助手',
          askAgent: 'plan',
          askAgentPresent: false,
          askAgentFallbackRisk: true,
          agents: ['build', '个人助手'],
          privacy: 'L2（问题 + 选区/段落 + 页面标题/网址）',
        })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.probeOpencode()
    expect(ai.webqaProbe?.askAgent).toBe('plan')
    expect(ai.webqaProbe?.askAgentFallbackRisk).toBe(true)
  })

  it('exitWebqa 清会话与选区（下次提问开独立会话）', async () => {
    mockDefaultInvoke()
    const ai = useAIStore()
    const p = ai.askPage('问')
    lastChannel?.onmessage?.({ type: 'session', sessionId: 'ses_1' })
    await p
    ai.webqaMode = true
    ai.exitWebqa()
    expect(ai.webqaMode).toBe(false)
    expect(ai.webqaSessionId).toBeNull()
    expect(ai.selection).toBeNull()
    expect(ai.messages).toHaveLength(0)
  })
})

// ── 整页汉化：翻译长在页面上，不是译到对话框 ──
//  - 收集 → 分批 → 逐批翻译 → 写回，四步按序走通
//  - 单批失败不整页回滚：已翻的留下，没翻的留原文
//  - 模型把译文原样吐回来（没翻）时跳过，别白改页面
//  - 无可翻内容时明确告知，不静默
//  - 还原走 restore 模式
describe('整页汉化', () => {
  const SEGS = [
    { id: 't0', kind: 't', text: 'Submit', chars: 6 },
    { id: 't1', kind: 't', text: 'Cancel', chars: 6 },
  ]
  const ATTRS = [{ id: 'x0', attr: 'title', orig: 'Close window' }]

  function mockHappy() {
    mockDefaultInvoke()
    // mock 必须照抄后端**真实**返回结构（webzh.rs 的 {ok,msg,data} 包装）。
    // 之前 mock 返回裸 {segments...}，前端也按裸的写，测试全绿、真机全挂——
    // 因为 invokeOk 不解包，前端拿到的其实是 {ok,msg,data}。mock 一旦"美化"
    // 契约，就只是在自证清白。
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true,
          msg: null,
          data: {
            segments: SEGS,
            attrs: ATTRS,
            total: 2,
            attrTotal: 1,
            skipped: 7,
            title: 'Demo Page',
          },
        })
      if (cmd === 'opencode_translate_model')
        // CommandResult<String>：成功时是裸字符串，不是 {ok,data} 包装
        return Promise.resolve('opencode/big-pickle')
      if (cmd === 'opencode_translate_warmup')
        // 默认不预热：每批 session_id 应为 null（Rust 按需建会话）
        return Promise.resolve([])
      if (cmd === 'translate_memory_lookup')
        // CommandResult<Vec<BatchItem>>：裸数组；空 = 全 miss
        return Promise.resolve([])
      if (cmd === 'translate_memory_remember') return Promise.resolve(0)
      if (cmd === 'plan_translate_batches')
        // 真实结构：{ok, data: BatchItem[][]}（data 是批次的数组的数组）
        return Promise.resolve({
          ok: true,
          data: [[{ id: 't0', text: 'Submit' }, { id: 't1', text: 'Cancel' }, { id: 'x0', text: 'Close window' }]],
        })
      if (cmd === 'opencode_translate_batch')
        return Promise.resolve({
          ok: true,
          msg: null,
          data: [
            { id: 't0', text: '提交' },
            { id: 't1', text: '取消' },
            { id: 'x0', text: '关闭窗口' },
          ],
          session_id: 'ses_zh',
        })
      if (cmd === 'page_apply_translation')
        return Promise.resolve({ ok: true, msg: null, data: { applied: 3, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
  }

  it('四步按序走通并写回页面', async () => {
    mockHappy()
    const ai = useAIStore()
    await ai.translatePage('tab-1')

    const calls = invokeMock.mock.calls.map((c) => c[0])
    expect(calls).toContain('page_collect_translatable')
    expect(calls).toContain('plan_translate_batches')
    expect(calls).toContain('opencode_translate_batch')
    expect(calls.indexOf('page_collect_translatable')).toBeLessThan(
      calls.indexOf('opencode_translate_batch'),
    )
    expect(calls.indexOf('opencode_translate_batch')).toBeLessThan(
      calls.indexOf('page_apply_translation'),
    )
    expect(ai.zhApplied).toBe(true)
    expect(ai.zhBusy).toBe(false)
  })

  it('收尾删翻译会话：用过的会话 id 统一清理', async () => {
    mockHappy()
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const clean = invokeMock.mock.calls.find((c) => c[0] === 'opencode_translate_cleanup')
    expect(clean).toBeTruthy()
    // mock 每批发 ses_zh：去重后只删一次
    expect((clean[1] as { sessions: string[] }).sessions).toEqual(['ses_zh'])
  })

  it('写回时按 mode=apply 传译文对', async () => {
    mockHappy()
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const ap = invokeMock.mock.calls.find((c) => c[0] === 'page_apply_translation')
    expect(ap?.[1]?.mode).toBe('apply')
    const pairs = ap?.[1]?.payload?.pairs
    expect(pairs).toEqual([
      { id: 't0', orig: 'Submit', translated: '提交' },
      { id: 't1', orig: 'Cancel', translated: '取消' },
    ])
    // 属性条目走 attrs，不能混进文本 pairs
    expect(ap?.[1]?.payload?.attrs).toEqual([
      { attr: 'title', orig: 'Close window', translated: '关闭窗口' },
    ])
  })

  it('译文与原文相同时跳过，不白改页面', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({ ok: true, msg: null, data: { segments: SEGS, attrs: [], total: 2, attrTotal: 0, skipped: 0, title: 'T' } })
      if (cmd === 'plan_translate_batches') return Promise.resolve({ ok: true, data: [SEGS] })
      if (cmd === 'opencode_translate_batch')
        // 模型原样吐回英文：等于没翻
        return Promise.resolve({ ok: true, msg: null, data: [{ id: 't0', text: 'Submit' }, { id: 't1', text: 'Cancel' }] })
      if (cmd === 'page_apply_translation') return Promise.resolve({ ok: true, msg: null, data: { applied: 0, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    // 一条都没翻出来就不该去动页面
    const calls = invokeMock.mock.calls.map((c) => c[0])
    expect(calls).not.toContain('page_apply_translation')
    expect(ai.zhApplied).toBe(false)
    expect(ai.error).toContain('一句都没翻出来')
  })

  it('单批全失败仍写回已翻好的，页面不回滚', async () => {
    mockDefaultInvoke()
    let batch = 0
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({ ok: true, msg: null, data: { segments: SEGS, attrs: [], total: 2, attrTotal: 0, skipped: 0, title: 'T' } })
      if (cmd === 'plan_translate_batches')
        return Promise.resolve({ ok: true, data: [[SEGS[0]], [SEGS[1]]] })
      if (cmd === 'opencode_translate_batch') {
        batch++
        // 第二批翻不出来（返回空），第一批正常
        return Promise.resolve(
          batch === 1 ? { ok: true, data: [{ id: 't0', text: '提交' }], session_id: 's' } : { ok: true, data: [] },
        )
      }
      if (cmd === 'page_apply_translation') return Promise.resolve({ ok: true, msg: null, data: { applied: 1, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    // 已翻的那部分照样写进页面
    expect(ai.zhApplied).toBe(true)
    const last = ai.messages[ai.messages.length - 1].content
    expect(last).toContain('已汉化本页')
    expect(last).toContain('保留原文')
  })

  it('无可翻内容时明确报错且不调翻译', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({ ok: true, msg: null, data: { segments: [], attrs: [], total: 0, attrTotal: 0, skipped: 30, title: 'T' } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const calls = invokeMock.mock.calls.map((c) => c[0])
    expect(calls).not.toContain('opencode_translate_batch')
    expect(ai.error).toContain('没找到需要翻译')
    expect(ai.zhBusy).toBe(false)
  })

  it('还原走 mode=restore 并清掉已汉化标记', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_apply_translation') return Promise.resolve({ ok: true, msg: null, data: { restored: 5 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.zhApplied = true
    await ai.restorePage('tab-1')
    const ap = invokeMock.mock.calls.find((c) => c[0] === 'page_apply_translation')
    expect(ap?.[1]?.mode).toBe('restore')
    expect(ai.zhApplied).toBe(false)
  })

  it('每批独立会话：预热池为空时 session_id 恒为 null（不复用长会话）', async () => {
    mockHappy()
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const batchCalls = invokeMock.mock.calls.filter((c) => c[0] === 'opencode_translate_batch')
    expect(batchCalls.length).toBeGreaterThan(0)
    for (const c of batchCalls) {
      expect(c[1].req.session_id).toBeNull()
    }
  })

  it('预热会话被每批一次性消费，绝不复用成第二批', async () => {
    mockHappy()
    const zh: Record<string, string> = { t0: '提交', t1: '取消' }
    invokeMock.mockImplementation(
      (cmd: string, args?: { req?: { items: Array<{ id: string }> } }) => {
        if (cmd === 'page_collect_translatable')
          return Promise.resolve({
            ok: true, msg: null,
            data: { segments: SEGS, attrs: [], total: 2, attrTotal: 0, skipped: 0, title: 'T' },
          })
        if (cmd === 'opencode_translate_warmup')
          return Promise.resolve(['ses_w0', 'ses_w1', 'ses_w2'])
        if (cmd === 'translate_memory_lookup') return Promise.resolve([])
        if (cmd === 'translate_memory_remember') return Promise.resolve(0)
        // 两批：第一批消费 ses_w0，第二批消费 ses_w1；ses_w2 闲置也不许回头复用
        if (cmd === 'plan_translate_batches')
          return Promise.resolve({ ok: true, data: [[SEGS[0]], [SEGS[1]]] })
        if (cmd === 'opencode_translate_batch') {
          const data = args!.req!.items.map((i) => ({ id: i.id, text: zh[i.id] }))
          return Promise.resolve({ ok: true, msg: null, data })
        }
        if (cmd === 'page_apply_translation')
          return Promise.resolve({ ok: true, msg: null, data: { applied: 1, mismatched: 0 } })
        return Promise.resolve(undefined)
      },
    )
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const sids = invokeMock.mock.calls
      .filter((c) => c[0] === 'opencode_translate_batch')
      .map((c) => c[1].req.session_id)
    expect(sids).toContain('ses_w0')
    expect(sids).toContain('ses_w1')
    // 两个会话各出现一次
    expect(sids.filter((s) => s === 'ses_w0')).toHaveLength(1)
    expect(sids.filter((s) => s === 'ses_w1')).toHaveLength(1)
  })

  it('翻译记忆：命中段直接写回、不进批次；新译文收尾入记忆', async () => {
    mockHappy()
    const MARK = { 0: { tag: 'a', a: [['href', '/x']] } }
    invokeMock.mockImplementation(
      (cmd: string, args?: { req?: { items: Array<{ id: string }> } }) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true, msg: null,
          data: {
            // t0 带 markers：缓存译文写回时结构表必须透传
            segments: [
              { id: 't0', kind: 'b', text: 'Submit', chars: 6, markers: MARK },
              SEGS[1],
            ],
            attrs: [], total: 2, attrTotal: 0, skipped: 0, title: 'T',
          },
        })
      if (cmd === 'opencode_translate_model') return Promise.resolve('opencode/big-pickle')
      if (cmd === 'opencode_translate_warmup') return Promise.resolve([])
      if (cmd === 'translate_memory_lookup')
        return Promise.resolve([{ id: 't0', text: '提交' }])
      if (cmd === 'translate_memory_remember') return Promise.resolve(1)
      if (cmd === 'plan_translate_batches')
        // 只有未命中的 t1 进批次
        return Promise.resolve({ ok: true, data: [[{ id: 't1', text: 'Cancel' }]] })
      if (cmd === 'opencode_translate_batch') {
        // 防御：命中的 t0 绝不能再发给模型
        expect(args!.req!.items.map((i) => i.id)).not.toContain('t0')
        return Promise.resolve({ ok: true, msg: null, data: [{ id: 't1', text: '取消' }] })
      }
      if (cmd === 'page_apply_translation')
        return Promise.resolve({ ok: true, msg: null, data: { applied: 2, mismatched: 0 } })
      return Promise.resolve(undefined)
      },
    )
    const ai = useAIStore()
    await ai.translatePage('tab-1')

    // 第一次写回就是缓存命中的 t0，且 markers 跟着走
    const applyCalls = invokeMock.mock.calls.filter((c) => c[0] === 'page_apply_translation')
    const firstPairs = applyCalls[0][1].payload.pairs
    expect(firstPairs).toEqual([
      { id: 't0', orig: 'Submit', translated: '提交', markers: MARK },
    ])
    // remember 只收新译文 t1，缓存命中的 t0 不重复写
    const remCall = invokeMock.mock.calls.find((c) => c[0] === 'translate_memory_remember')
    const remIds = remCall![1].req.pairs.map((p: { id: string }) => p.id)
    expect(remIds).toEqual(['t1'])
    // 汇报里告诉用户多少处来自本地记忆
    expect(ai.messages[ai.messages.length - 1].content).toContain('本地翻译记忆')
  })

  it('翻译记忆：全命中时不建会话、不调模型，直接秒翻', async () => {
    mockHappy()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true, msg: null,
          data: { segments: SEGS, attrs: [], total: 2, attrTotal: 0, skipped: 0, title: 'T' },
        })
      if (cmd === 'opencode_translate_model') return Promise.resolve('m1')
      if (cmd === 'translate_memory_lookup')
        return Promise.resolve([
          { id: 't0', text: '提交' },
          { id: 't1', text: '取消' },
        ])
      if (cmd === 'page_apply_translation')
        return Promise.resolve({ ok: true, msg: null, data: { applied: 2, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const cmds = invokeMock.mock.calls.map((c) => c[0])
    expect(cmds).not.toContain('opencode_translate_batch')
    expect(cmds).not.toContain('opencode_translate_warmup')
    expect(cmds).not.toContain('plan_translate_batches')
    expect(cmds).not.toContain('translate_memory_remember')
  })

  it('拆半重试：整批缺项时只补翻缺的，补回的照样写页面', async () => {
    mockHappy()
    const FOUR = ['a', 'b', 'c', 'd'].map((x, i) => ({ id: `t${i}`, text: `Word${x}` }))
    invokeMock.mockImplementation((cmd: string, args?: { req?: { items: Array<{ id: string }> } }) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true, msg: null,
          data: {
            segments: FOUR.map((f) => ({ id: f.id, kind: 't', text: f.text, chars: 5 })),
            attrs: [], total: 4, attrTotal: 0, skipped: 0, title: 'T',
          },
        })
      if (cmd === 'opencode_translate_model') return Promise.resolve('m1')
      if (cmd === 'opencode_translate_warmup') return Promise.resolve([])
      if (cmd === 'translate_memory_lookup') return Promise.resolve([])
      if (cmd === 'translate_memory_remember') return Promise.resolve(4)
      if (cmd === 'plan_translate_batches') return Promise.resolve({ ok: true, data: [FOUR] })
      if (cmd === 'opencode_translate_batch') {
        const ids = args!.req!.items.map((i) => i.id).sort()
        // 首次整批：模型只吐回 t0/t1；重试两瓣各补一个
        if (ids.join(',') === 't0,t1,t2,t3')
          return Promise.resolve({
            ok: true, msg: null,
            data: [{ id: 't0', text: '甲' }, { id: 't1', text: '乙' }],
          })
        if (ids.join(',') === 't2')
          return Promise.resolve({ ok: true, msg: null, data: [{ id: 't2', text: '丙' }] })
        if (ids.join(',') === 't3')
          return Promise.resolve({ ok: true, msg: null, data: [{ id: 't3', text: '丁' }] })
        return Promise.resolve({ ok: true, msg: null, data: [] })
      }
      if (cmd === 'page_apply_translation')
        return Promise.resolve({ ok: true, msg: null, data: { applied: 4, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const appliedIds = invokeMock.mock.calls
      .filter((c) => c[0] === 'page_apply_translation')
      .flatMap((c) => c[1].payload.pairs.map((p: { id: string }) => p.id))
      .sort()
    // 四个全写回：后两个是拆半重试补回来的
    expect(appliedIds).toEqual(['t0', 't1', 't2', 't3'])
    // 汇报里没有"保留原文"
    expect(ai.messages[ai.messages.length - 1].content).not.toContain('保留原文')
  })
})

// 超长段分段翻译（高哥 2026-09-26 提的"超大文件可以分段翻译"）：
// Rust 把超长段切成 `原id.N` 子块分别翻译，前端必须按 base id 拼回整段再写回。
describe('整页汉化：超长段分段翻译', () => {
  it('子块译文按 base id 拼成整段写回', async () => {
    const LONG = 'x'.repeat(5000)
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true, msg: null,
          data: {
            segments: [{ id: 't0', kind: 't', text: LONG, chars: 5000 }],
            attrs: [],
            total: 1,
            attrTotal: 0,
            skipped: 0,
            title: 'Long',
          },
        })
      if (cmd === 'plan_translate_batches')
        // Rust 切成的子块：t0.0 / t0.1 / t0.2
        return Promise.resolve({
          ok: true,
          data: [[{ id: 't0.0', text: 'AAAA' }, { id: 't0.1', text: 'BBBB' }, { id: 't0.2', text: 'CCCC' }]],
        })
      if (cmd === 'opencode_translate_batch')
        return Promise.resolve({
          ok: true, msg: null,
          data: [
            { id: 't0.0', text: '第一块' },
            { id: 't0.1', text: '第二块' },
            { id: 't0.2', text: '第三块' },
          ],
          session_id: 's',
        })
      if (cmd === 'page_apply_translation') return Promise.resolve({ ok: true, msg: null, data: { applied: 1, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const ap = invokeMock.mock.calls.find((c) => c[0] === 'page_apply_translation')
    const pairs = ap?.[1]?.payload?.pairs
    // 写回必须是【一个】整段 id，不是三个子块
    expect(pairs).toHaveLength(1)
    expect(pairs[0].id).toBe('t0')
    expect(pairs[0].orig).toBe(LONG)
    expect(pairs[0].translated).toBe('第一块第二块第三块')
  })

  it('只有部分子块翻出来时也拼已翻的，不补原文', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'page_collect_translatable')
        return Promise.resolve({
          ok: true, msg: null,
          data: {
            segments: [{ id: 't0', kind: 't', text: 'y'.repeat(5000), chars: 5000 }],
            attrs: [], total: 1, attrTotal: 0, skipped: 0, title: 'Long',
          },
        })
      if (cmd === 'plan_translate_batches')
        return Promise.resolve({ ok: true, data: [[{ id: 't0.0', text: 'A' }, { id: 't0.1', text: 'B' }]] })
      if (cmd === 'opencode_translate_batch')
        return Promise.resolve({ ok: true, msg: null, data: [{ id: 't0.0', text: '翻出来的' }] })
      if (cmd === 'page_apply_translation') return Promise.resolve({ ok: true, msg: null, data: { applied: 1, mismatched: 0 } })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translatePage('tab-1')
    const ap = invokeMock.mock.calls.find((c) => c[0] === 'page_apply_translation')
    expect(ap?.[1]?.payload?.pairs[0].translated).toBe('翻出来的')
  })
})


// 翻译选中内容（高哥 2026-09-26）：只翻选区、贴进对话框，不动页面 DOM
describe('翻译选中内容', () => {
  it('读到选区就翻译并贴进对话，绝不碰页面 DOM', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({
          has_selection: true,
          text: 'The server returned an error.',
          title: 'Demo',
          url: 'https://x',
        })
      if (cmd === 'opencode_translate_batch')
        return Promise.resolve({
          ok: true,
          msg: null,
          data: [{ id: 's0', text: '服务器返回了一个错误。' }],
        })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translateSelection('tab-1')
    const last = ai.messages[ai.messages.length - 1]
    expect(last.role).toBe('assistant')
    expect(last.content).toContain('服务器返回')
    const calls = invokeMock.mock.calls.map((c) => c[0])
    expect(calls).not.toContain('page_apply_translation')
    expect(calls).not.toContain('page_collect_translatable')
  })

  it('选不中就给提示、不发翻译请求', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection') return Promise.resolve({ has_selection: false })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.translateSelection('tab-1')
    expect(ai.error).toContain('没读到选中内容')
    expect(invokeMock.mock.calls.map((c) => c[0])).not.toContain('opencode_translate_batch')
  })
})

// 问答上下文三级降级：选区 → 正文全文 → 语义快照
describe('问答三级降级', () => {
  it('有选区直接用选区，不碰后两级', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({
          has_selection: true,
          text: '选中的一段',
          title: '页标题',
          url: 'https://example.com/a',
        })
      if (cmd === 'content_qa_prompt')
        return Promise.resolve({ prompt: 'PROMPT', chars: 5, truncated: false })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    const r = await ai.buildQaPrompt('这段讲啥', 'tab-1')
    expect(r?.source).toBe('选区')
    expect(r?.prompt).toBe('PROMPT')
    expect(ai.qaSource).toBe('选区')
    const calls = invokeMock.mock.calls.map((c) => c[0])
    expect(calls).not.toContain('content_extract')
    expect(calls).not.toContain('content_snapshot')
    // 组装参数透传来源与问题
    const qa = invokeMock.mock.calls.find((c) => c[0] === 'content_qa_prompt')
    expect(qa?.[1]).toMatchObject({ source: '选区', question: '这段讲啥' })
  })

  it('无选区时用正文全文', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection') return Promise.resolve({ has_selection: false })
      if (cmd === 'content_extract')
        return Promise.resolve({ title: '文标题', text: '正文内容正文内容' })
      if (cmd === 'content_qa_prompt')
        return Promise.resolve({ prompt: 'P2', chars: 8, truncated: false })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    const r = await ai.buildQaPrompt('总结一下', 'tab-1')
    expect(r?.source).toBe('正文全文')
    expect(ai.qaSource).toBe('正文全文')
  })

  it('前两级都空时用语义快照并标可能不全', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection') return Promise.resolve({ has_selection: false })
      if (cmd === 'content_extract') return Promise.reject('抓取失败')
      if (cmd === 'content_snapshot')
        return Promise.resolve({
          title: '快照页',
          url: 'https://example.com/a',
          text: '快照正文',
          interactive: { links: 0, buttons: 0, inputs: 0 },
          forms: [],
          links: [],
          webmcp_tools: [],
          aria: [],
        })
      if (cmd === 'content_qa_prompt')
        return Promise.resolve({ prompt: 'P3', chars: 4, truncated: false })
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    const r = await ai.buildQaPrompt('问', 'tab-1')
    expect(r?.source).toBe('语义快照')
    const qa = invokeMock.mock.calls.find((c) => c[0] === 'content_qa_prompt')
    expect(qa?.[1]).toMatchObject({ source: '语义快照', coverage: '可能不全' })
  })

  it('三级全空返回 null 并提示', async () => {
    mockDefaultInvoke()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection') return Promise.resolve({ has_selection: false })
      if (cmd === 'content_extract') return Promise.reject('x')
      if (cmd === 'content_snapshot') return Promise.reject('no tab')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    const r = await ai.buildQaPrompt('问', 'tab-1')
    expect(r).toBeNull()
    expect(ai.error).toContain('没读到可用内容')
    expect(ai.qaSource).toBeNull()
  })

  it('空问题直接返回 null，不读任何上下文', async () => {
    mockDefaultInvoke()
    const ai = useAIStore()
    const r = await ai.buildQaPrompt('   ', 'tab-1')
    expect(r).toBeNull()
    expect(invokeMock).not.toHaveBeenCalledWith('read_page_selection', expect.anything())
  })
})

// 带上下文发送：显示存原文，模型侧用组装提示词；总结走阅读管线，失败回退老一句
describe('带上下文发送', () => {
  it('sendMessage 覆盖：ai_chat 收到组装提示词，对话框存原文', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('请总结', '组装好的长提示词')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    // 模型侧最后一问是组装词
    expect(args.messages[args.messages.length - 1]).toEqual({
      role: 'user',
      content: '组装好的长提示词',
    })
    // 显示侧仍是原文
    expect(ai.messages[0]).toEqual({ role: 'user', content: '请总结' })
  })

  it('不传覆盖时行为不变：模型侧即原文', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('原文')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    expect(args.messages[args.messages.length - 1].content).toBe('原文')
  })

  it('summarizeWithContext：走阅读管线组装，来源标正文全文', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'content_summary_prompt')
        return Promise.resolve({ title: 'T', url: 'U', prompt: '组装摘要提示词', chars: 10, truncated: false })
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    await ai.summarizeWithContext()
    expect(invokeMock).toHaveBeenCalledWith('content_summary_prompt', {
      url: 'https://example.com/a',
    })
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    expect(args.messages[args.messages.length - 1].content).toBe('组装摘要提示词')
    expect(ai.qaSource).toBe('正文全文')
  })

  it('summarizeWithContext：管线失败回退老一句，不抛给用户', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'content_summary_prompt') return Promise.reject('抓取失败')
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    await ai.summarizeWithContext()
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    // 回退老一句：模型侧即显示原文
    expect(args.messages[args.messages.length - 1].content).toContain('5 条以内')
    expect(ai.error).toBeNull()
  })

  it('explainWithContext：三级组装后发送，显示存原文', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection')
        return Promise.resolve({
          has_selection: true,
          text: '选中的一段',
          title: '页标题',
          url: 'https://example.com/a',
        })
      if (cmd === 'content_qa_prompt')
        return Promise.resolve({ prompt: '组装解释提示词', chars: 7, truncated: false })
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    await ai.explainWithContext('tab-1')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    expect(args.messages[args.messages.length - 1].content).toBe('组装解释提示词')
    expect(ai.messages[0].content).toContain('通俗易懂')
    expect(ai.qaSource).toBe('选区')
  })

  it('explainWithContext：组装失败回退老一句', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'read_page_selection') return Promise.resolve({ has_selection: false })
      if (cmd === 'content_extract') return Promise.reject('x')
      if (cmd === 'content_snapshot') return Promise.reject('no tab')
      if (cmd === 'mcp_list_tools') {
        return Promise.resolve([{ name: 't', serverName: 's', description: '' }])
      }
      if (cmd === 'ai_chat') return Promise.resolve('ok')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    ai.setPageContext('https://example.com/a', '页标题')
    await ai.explainWithContext('tab-1')
    const call = invokeMock.mock.calls.find((c) => c[0] === 'ai_chat')
    const args = call![1] as { messages: Array<{ role: string; content: string }> }
    expect(args.messages[args.messages.length - 1].content).toContain('通俗易懂')
    expect(ai.error).toBeNull()
  })
})

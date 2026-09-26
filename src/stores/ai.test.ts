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
    expect(ai.error).toBe('AI 服务错误 500')
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

  it('invoke reject 裸字符串（旧命令形态）→ error 取原消息', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'content_snapshot') return Promise.reject('x')
      if (cmd === 'mcp_list_tools') return Promise.resolve([])
      if (cmd === 'ai_chat_stream') return Promise.reject('网络不可达')
      return Promise.resolve(undefined)
    })
    const ai = useAIStore()
    await ai.sendMessage('hi')
    expect(ai.error).toBe('网络不可达')
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
    expect(ai.error).toBe('未配置 API Key')
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
      if (cmd === 'read_selection')
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
      if (cmd === 'read_selection')
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
      if (cmd === 'read_selection')
        return Promise.resolve({ has_selection: false, hibernated: true, hints: '标签已休眠，切回可再读' })
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
      if (cmd === 'read_selection')
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

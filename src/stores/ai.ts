// AI 侧边栏状态管理（Pinia）
// 对接 AGNES AI（Rust ai_chat_stream 流式命令，OpenAI 兼容）
import { defineStore } from 'pinia'
import { invoke, Channel } from '@tauri-apps/api/core'
import { errText, invokeOk } from '../utils/invoke'
import { useSettingsStore } from './settings'
import type { Snapshot } from '../types/content'

export interface AIMessage {
  role: 'user' | 'assistant'
  content: string
}

/** 后端流式事件（与 ai.rs StreamEvent 对应） */
interface StreamEvent {
  type: 'delta' | 'done' | 'error'
  content?: string
  full?: string
  message?: string
}

/** 网页随手问：本机 opencode 事件（与 webqa.rs WebqaEvent 对应） */
interface WebqaEvent {
  type: 'session' | 'delta' | 'done' | 'error'
  sessionId?: string
  content?: string
  full?: string
  message?: string
}

/** opencode 探测结果（后端 opencode_probe） */
export interface OpencodeProbe {
  ok: boolean
  baseUrl: string
  model: string
  defaultAgent: string
  askAgent: string
  /** askAgent 是否真的存在。false = 会被静默回退到有编辑权限的默认 agent */
  askAgentPresent: boolean
  askAgentFallbackRisk: boolean
  agents: string[]
  privacy: string
}

interface AIState {
  messages: AIMessage[]
  loading: boolean
  /** 当前页面上下文（URL + 标题，供 AI 分析） */
  pageContext: { url: string; title: string } | null
  error: string | null
  /** 当前流式请求 id（用于停止生成） */
  streamingId: string | null
  /** 可用 MCP 工具（阶段 1.3：有工具时对话走 function calling） */
  tools: Array<{ name: string; serverName: string; description: string }>
  /** 工具是否已加载 */
  toolsLoaded: boolean
  /** 命令栏「问 AI」暂存的提问：AI 侧栏打开时消费（阶段 3.1） */
  pendingPrompt: string | null
  // ── 网页随手问（P2/P3）：本机 opencode ──
  /** 选区/段落正文（来自 read_selection，L2 隐私档的"上下文"） */
  selection: { text: string; title: string; url: string } | null
  /** opencode 会话 id：多轮追问靠它保持上下文；空则首次提问时新建 */
  webqaSessionId: string | null
  /** opencode 连通性探测结果 */
  webqaProbe: OpencodeProbe | null
  /** 「网页随手问」模式：走本机 opencode，不走 AGNES 云端 */
  webqaMode: boolean
}

export const useAIStore = defineStore('ai', {
  state: (): AIState => ({
    messages: [],
    loading: false,
    pageContext: null,
    error: null,
    streamingId: null,
    tools: [],
    toolsLoaded: false,
    pendingPrompt: null,
    selection: null,
    webqaSessionId: null,
    webqaProbe: null,
    webqaMode: false,
  }),

  actions: {
    /** 设置当前页面上下文（调用方在打开侧边栏/切换标签时更新） */
    setPageContext(url: string, title: string) {
      this.pageContext = { url, title }
    },

    /** 命令栏「问 AI」：暂存提问，AI 侧栏打开时消费（阶段 3.1） */
    queuePrompt(prompt: string) {
      this.pendingPrompt = prompt.trim() || null
    },

    /** 取出并清空暂存提问（返回 null 表示无待发问） */
    takePendingPrompt(): string | null {
      const p = this.pendingPrompt
      this.pendingPrompt = null
      return p
    },

    /** 加载可用 MCP 工具（阶段 1.3） */
    async loadTools() {
      if (this.toolsLoaded) return
      try {
        this.tools = await invokeOk('mcp_list_tools')
      } catch {
        this.tools = []
      }
      this.toolsLoaded = true
    },

    /** 发送消息（流式：先占位空回复，逐块追加） */
    async sendMessage(content: string) {
      const text = content.trim()
      if (!text || this.loading) return
      this.messages.push({ role: 'user', content: text })
      // 先推一个空 assistant 消息占位，流式增量往里面追加
      this.messages.push({ role: 'assistant', content: '' })
      this.loading = true
      this.error = null
      const requestId = crypto.randomUUID
        ? crypto.randomUUID()
        : `${Date.now()}-${Math.random().toString(36).slice(2)}`
      this.streamingId = requestId
      try {
        const ctx = this.pageContext
        // 阶段 1.1（页面理解层）：发送前取活动标签语义快照（本地提取，不出网），
        // 让 AI 真正"理解页面"——正文摘要 + 可交互元素。失败则降级为标题/地址。
        let snapshotDesc = ''
        try {
          const snap = await invokeOk<Snapshot>('content_snapshot')
          if (snap && snap.title) {
            const text = (snap.text || '').slice(0, 800)
            snapshotDesc = `【页面语义快照】标题：${snap.title}；地址：${snap.url}；正文摘要：${text || '(无可读文本)'}；可交互元素：链接 ${snap.interactive?.links ?? 0} 个、按钮 ${snap.interactive?.buttons ?? 0} 个、输入框 ${snap.interactive?.inputs ?? 0} 个。`
            // 阶段 1.5 WebMCP polyfill 试验：页面暴露的工具声明附给 AI（供理解与引导操作）
            if (snap.webmcp_tools && snap.webmcp_tools.length > 0) {
              const toolsDesc = snap.webmcp_tools
                .map((t) => `${t.name}：${t.description || '(无描述)'}`)
                .join('；')
              snapshotDesc += ` 页面通过 WebMCP 暴露了 ${snap.webmcp_tools.length} 个工具：${toolsDesc}。`
            }
            // Lighthouse Agentic 信号②·代理为中心无障碍→Agent 定位（阶段 1.5 落地）：
            // 把程序化名称（aria-label/labelledby）+ 有效角色 + 隐藏态提炼成"Agent 可执行的
            // 语义定位清单"——Agent 不再靠裸 CSS 选择器猜元素，而是用站点已声明好的
            // 无障碍自述来**语义定位**（对齐乘 4.2.1"程序化名称"+信号③"树完整性"）。
            if (snap.aria && snap.aria.length > 0) {
              const usual = snap.aria.filter((a) => !a.hidden)
              if (usual.length > 0) {
                const ariaList = usual
                  .slice(0, 10)
                  .map(
                    (a, idx) =>
                      `${idx + 1}. ${a.tag}[role=${a.role}]「${a.name || '(无名)'}」`,
                  )
                  .join('；')
                snapshotDesc += ` 页面带程序化名称的可交互元素：${ariaList}。要对"「${usual[0].name || '首页'}」按钮"操作时,优先用无障碍自述里的语义定位（角色+名称）而不是猜 CSS。`
              }
            }
          }
        } catch {
          // 快照失败不阻断对话（可能页面未就绪/无活动标签），降级用标题+地址
        }
        const system = snapshotDesc
          ? `你是 ShiftBrowser 内置 AI 助手。${snapshotDesc}回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。`
          : ctx
            ? `你是 ShiftBrowser 内置 AI 助手。当前用户正在浏览的页面：标题「${ctx.title}」，地址 ${ctx.url}。回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。`
            : '你是 ShiftBrowser 内置 AI 助手。回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。'
        // 从设置读取 AI 参数（模型/API 地址/API Key）
        const settingsStore = useSettingsStore()
        const model = settingsStore.get('ai_model') || 'agnes-2.0-flash'
        const baseUrl = settingsStore.get('ai_base_url') || ''
        const apiKey = settingsStore.get('ai_api_key') || ''
        // get_all_settings 返回的是掩码（前4后4 ****），不是真 key：
        // 不回传（避免网络层泄露），后端也会忽略掩码并回退环境变量
        const realKey = apiKey.includes('****') ? '' : apiKey
        // 历史 = 去掉最后一条空占位，再取最近 20 条
        const history = this.messages.slice(0, -1).slice(-20)

        // 阶段 1.3（工具层）：有可用 MCP 工具时走非流式 ai_chat（function calling 多轮执行），
        // 无工具保持流式体验。工具场景需要多轮工具执行，流式反而卡顿。
        await this.loadTools()
        if (this.tools.length > 0) {
          const reply = await invokeOk<string>('ai_chat', {
            provider: settingsStore.get('ai_provider') || 'agnes',
            messages: [
              { role: 'system', content: system },
              ...history.map((m) => ({ role: m.role, content: m.content })),
            ],
            model,
            baseUrl: baseUrl || undefined,
            apiKey: realKey || undefined,
          })
          const last = this.messages[this.messages.length - 1]
          if (last && last.role === 'assistant') last.content = reply
          this.loading = false
          this.streamingId = null
          return
        }

        const channel = new Channel<StreamEvent>()
        channel.onmessage = (event) => {
          if (event.type === 'delta' && event.content) {
            // 增量追加到当前 assistant 回复
            const last = this.messages[this.messages.length - 1]
            if (last && last.role === 'assistant') last.content += event.content
          } else if (event.type === 'done') {
            // 用完整回复兜底（保证一致）
            const last = this.messages[this.messages.length - 1]
            if (last && last.role === 'assistant' && event.full) {
              last.content = event.full
            }
            this.loading = false
            this.streamingId = null
          } else if (event.type === 'error') {
            // 用户主动停止不算错误
            if (event.message !== '已停止生成') {
              this.error = event.message || 'AI 请求失败'
            }
            this.dropEmptyAssistant()
            this.loading = false
            this.streamingId = null
          }
        }

        await invokeOk('ai_chat_stream', {
          requestId,
          provider: settingsStore.get('ai_provider') || 'agnes',
          messages: [
            { role: 'system', content: system },
            ...history.map((m) => ({ role: m.role, content: m.content })),
          ],
          onEvent: channel,
          model,
          baseUrl: baseUrl || undefined,
          apiKey: realKey || undefined,
        })
      } catch (e) {
        this.error = errText(e)
        this.dropEmptyAssistant()
        this.loading = false
        this.streamingId = null
      }
    },

    /** 停止当前流式生成 */
    async stopStreaming() {
      if (!this.streamingId) return
      const id = this.streamingId
      this.streamingId = null
      try {
        await invoke('ai_cancel_stream', { requestId: id })
      } catch {
        // 忽略取消失败（后端流式循环会在 send 失败/超时后自行退出）
      }
    },

    /** 移除最后一条空 assistant 占位（出错/停止时） */
    dropEmptyAssistant() {
      const last = this.messages[this.messages.length - 1]
      if (last && last.role === 'assistant' && !last.content.trim()) {
        this.messages.pop()
      }
    },

    /** 快捷操作：总结当前页 */
    async summarizePage() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可分析的页面。' })
        return
      }
      await this.sendMessage(
        `请总结当前页面「${ctx.title}」（${ctx.url}）的主要内容，用 5 条以内要点概括。`,
      )
    },

    /** 快捷操作：翻译当前页标题/简介 */
    async translatePage() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可翻译的页面。' })
        return
      }
      await this.sendMessage(`请把当前页面「${ctx.title}」的标题和核心内容翻译成中文。`)
    },

    /** 快捷操作：解释当前页 */
    async explainPage() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可解释的页面。' })
        return
      }
      await this.sendMessage(
        `请用通俗易懂的方式解释当前页面「${ctx.title}」（${ctx.url}）讲的是什么、对用户有什么价值。`,
      )
    },

    // ── 网页随手问（P2/P3）：本机 opencode ──

    /**
     * 探测本机 opencode 连通性。
     * 面板每次打开时调一次：不可达就别让用户白等流式空转。
     */
    async probeOpencode() {
      try {
        this.webqaProbe = await invokeOk<OpencodeProbe>('opencode_probe')
      } catch (e) {
        this.webqaProbe = null
        this.error = errText(e)
      }
    },

    /**
     * 取当前标签的选区（L2 隐私档的"上下文"）。
     *
     * 三种情形都要处理，不能当成"用户没选东西"：
     * - 正常有选区 → 用它
     * - rejected（密码框等敏感区）→ 明确告知，绝不静默当成没选
     * - hibernated（标签已休眠）→ 提示切回标签唤醒
     */
    async captureSelection(tabId?: string) {
      try {
        const args: Record<string, unknown> = { expand: true }
        if (tabId !== undefined) args.tabId = tabId
        const r = await invokeOk<{
          has_selection: boolean
          text?: string
          title?: string
          url?: string
          rejected?: boolean
          hibernated?: boolean
          hints?: string
        }>('read_selection', args)

        if (r.hibernated) {
          this.selection = null
          this.error = r.hints || '标签已休眠，切回该标签后可再读选区'
          return
        }
        if (r.rejected) {
          this.selection = null
          this.error = '选区在密码/敏感区域，已拒绝读取（不会发给 AI）'
          return
        }
        if (!r.has_selection) {
          this.selection = null
          return
        }
        this.selection = {
          text: r.text || '',
          title: r.title || this.pageContext?.title || '',
          url: r.url || this.pageContext?.url || '',
        }
      } catch (e) {
        this.selection = null
        this.error = errText(e)
      }
    },

    /**
     * 「网页随手问」：把问题 + 选区上下文交给本机 opencode，流式回字。
     *
     * 与 sendMessage 的区别：走回环地址的 opencode 而非 AGNES 云端，
     * 且自带选区上下文（不必用户自己粘贴）。
     */
    async askPage(question: string) {
      const text = question.trim()
      if (!text || this.loading) return

      this.error = null
      this.loading = true
      this.messages.push({ role: 'user', content: text })
      this.messages.push({ role: 'assistant', content: '' })

      const requestId = `webqa-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`
      this.streamingId = requestId

      const channel = new Channel<WebqaEvent>()
      channel.onmessage = (event) => {
        if (event.type === 'session' && event.sessionId) {
          this.webqaSessionId = event.sessionId
        } else if (event.type === 'delta' && event.content) {
          const last = this.messages[this.messages.length - 1]
          if (last && last.role === 'assistant') last.content += event.content
        } else if (event.type === 'done') {
          // full 是权威值（delta 可能重复到达）
          const last = this.messages[this.messages.length - 1]
          if (last && last.role === 'assistant' && event.full) {
            last.content = event.full
          }
          if (event.sessionId) this.webqaSessionId = event.sessionId
          this.loading = false
          this.streamingId = null
        } else if (event.type === 'error') {
          if (event.message !== '已停止生成') {
            this.error = event.message || '本机 opencode 请求失败'
          }
          this.dropEmptyAssistant()
          this.loading = false
          this.streamingId = null
        }
      }

      try {
        await invokeOk('opencode_ask', {
          req: {
            requestId,
            question: text,
            context: this.selection?.text,
            pageTitle: this.selection?.title || this.pageContext?.title,
            pageUrl: this.selection?.url || this.pageContext?.url,
            sessionId: this.webqaSessionId,
          },
          onEvent: channel,
        })
      } catch (e) {
        this.error = errText(e)
        this.dropEmptyAssistant()
        this.loading = false
        this.streamingId = null
      }
    },

    /** 停止网页随手问的流式生成 */
    async stopWebqa() {
      if (!this.streamingId) return
      const id = this.streamingId
      this.streamingId = null
      try {
        await invoke('opencode_cancel', { requestId: id })
      } catch {
        // 忽略取消失败：后端流式循环会在超时/断开后自行退出
      }
      this.loading = false
    },

    /** 退出网页随手问：清会话与选区（下次提问重新开独立会话） */
    exitWebqa() {
      this.webqaMode = false
      this.webqaSessionId = null
      this.selection = null
      this.messages = []
      this.error = null
    },

    /** 清空对话 */
    clearConversation() {
      this.messages = []
      this.error = null
      this.streamingId = null
    },
  },
})

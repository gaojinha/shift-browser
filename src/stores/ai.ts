// AI 侧边栏状态管理（Pinia）
// 对接 AGNES AI（Rust ai_chat_stream 流式命令，OpenAI 兼容）
import { defineStore } from 'pinia'
import { invoke, Channel } from '@tauri-apps/api/core'
import { invokeOk } from '../utils/invoke'
import { shortcutSummarize, shortcutExplain, humanizeError } from '../frontend-prompts'
import { AI_BEHAVIOR_RULES } from '../constants'
import { useSettingsStore } from './settings'
import type { Snapshot } from '../types/content'

/**
 * webzh 三件套命令（collect / apply / restore）统一返回 `{ok, msg, data}`，
 * 与 api.sock 协议一致（见 webzh.rs 的 CollectResult / ApplyResult）。
 *
 * 坑一：`invokeOk` 只归一化错误、**不解包**。曾经直接写 `col.segments`，
 * 实际拿到的是 `{ok,msg,data}`，`col.segments` 恒为 undefined，于是整页汉化
 * 永远报"这页没找到需要翻译的文字"——而且只在真机会犯：api.sock 那条路协议层
 * 会自动展开 `{ok,data}`，测试和手测全绿，前端链路一次没跑过。
 * 坑二：`ok:false` 必须抛错。当成空数据的话，"收集失败"会被下游误判成
 * "页面没文字"，提示驴唇不对马嘴。
 */
async function zhCall<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const r = await invokeOk<{ ok?: boolean; msg?: string | null; data?: T }>(cmd, args)
  if (!r || r.ok !== true) throw new Error(r?.msg || `${cmd} 失败`)
  return r.data as T
}

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
  /** 同一会话已追问轮数：超 50 轮另开新会话，防上下文无限滚大 */
  webqaRounds: number
  /** opencode 连通性探测结果 */
  webqaProbe: OpencodeProbe | null
  /** 「网页随手问」模式：走本机 opencode，不走 AGNES 云端 */
  webqaMode: boolean
  // ── 整页汉化（webzh）：本机 opencode 翻译，直接改 DOM ──
  /** 汉化进行中（整页要跑几十秒，全程占状态） */
  zhBusy: boolean
  /** 汉化进度：已翻/总段数、已翻/总批次、当前批次文本预览 */
  zhProgress: { done: number; total: number; batch: number; batches: number; preview: string } | null
  /** 该标签是否已汉化（决定按钮显示"汉化"还是"还原"） */
  zhApplied: boolean
  /** 取消汉化的开关：置 true 后当前批次做完就停 */
  zhCancel: boolean
  /** 问答上下文来源（选区/正文全文/语义快照），供界面显示“本次用啥问的” */
  qaSource: string | null
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
    webqaRounds: 0,
    webqaProbe: null,
    webqaMode: false,
    zhBusy: false,
    zhProgress: null,
    zhApplied: false,
    zhCancel: false,
    qaSource: null,
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

    /**
     * 发送消息（流式：先占位空回复，逐块追加）。
     * actualPrompt：实际发给模型的最后一问。对话框和历史里仍存原文，
     * 模型侧用组装好的提示词（带上下文模板），两边不互相污染。
     */
    async sendMessage(content: string, actualPrompt?: string) {
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
        // 行为约定（数据/指令隔离、hints 优先、wait_for 先于交互等）与
        // MCP initialize 的 instructions 同源（contracts::ai_behavior，build.rs 生成）
        const system = `${snapshotDesc
          ? `你是 ShiftBrowser 内置 AI 助手。${snapshotDesc}回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。`
          : ctx
            ? `你是 ShiftBrowser 内置 AI 助手。当前用户正在浏览的页面：标题「${ctx.title}」，地址 ${ctx.url}。回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。`
            : '你是 ShiftBrowser 内置 AI 助手。回答要简洁实用，中文回复；代码用围栏代码块，列表用 Markdown 语法。'}

${AI_BEHAVIOR_RULES}`
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
        // 模型侧历史：最后一问可用组装好的提示词替换（显示侧不变）
        const actual = actualPrompt?.trim() || ''
        const modelHistory =
          actual && history.length > 0 && history[history.length - 1].role === 'user'
            ? history.map((m, i) =>
                i === history.length - 1 ? { role: m.role, content: actual } : m,
              )
            : history

        // 阶段 1.3（工具层）：有可用 MCP 工具时走非流式 ai_chat（function calling 多轮执行），
        // 无工具保持流式体验。工具场景需要多轮工具执行，流式反而卡顿。
        await this.loadTools()
        if (this.tools.length > 0) {
          const reply = await invokeOk<string>('ai_chat', {
            provider: settingsStore.get('ai_provider') || 'agnes',
            messages: [
              { role: 'system', content: system },
              ...modelHistory.map((m) => ({ role: m.role, content: m.content })),
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
            // 用户主动停止不算错误；其余翻成人话
            if (event.message !== '已停止生成') {
              this.error = humanizeError(event.message || 'AI 请求失败')
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
            ...modelHistory.map((m) => ({ role: m.role, content: m.content })),
          ],
          onEvent: channel,
          model,
          baseUrl: baseUrl || undefined,
          apiKey: realKey || undefined,
        })
      } catch (e) {
        this.error = humanizeError(e)
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
      await this.sendMessage(shortcutSummarize(ctx.title, ctx.url))
    },

    /**
     * 快捷操作：带上下文的总结（单源模板组装）。
     * 正文经阅读管线抓取，比 800 字快照全；失败回退老一句，不让用户干等。
     */
    async summarizeWithContext() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可分析的页面。' })
        return
      }
      const display = shortcutSummarize(ctx.title, ctx.url)
      try {
        const r = await invokeOk<{ prompt: string }>('content_summary_prompt', {
          url: ctx.url,
        })
        if (!r || !r.prompt.trim()) throw new Error('空提示词')
        this.qaSource = '正文全文'
        await this.sendMessage(display, r.prompt)
      } catch {
        await this.summarizePage()
      }
    },

    /**
     * 快捷操作：整页汉化（翻译直接长在页面上，不是译到对话框里）
     *
     * 编排逻辑放前端而不是 Rust：分批大小、并发、进度节奏都得随页面大小和
     * 实测速度调，写死后端每次调整都要重新出包。
     *
     * 2026-09-27 速度优化（实测 opencode 云端每批固定开销 ~6s、边际 ~0.07s/段）：
     * 1. 每批**独立会话** + 3 路 worker 并发——复用会话时历史 prefill 逐批滚雪球，
     *    实测 6 路并发仅 1.36x，2-3 路性价比最高；视口首批在收集时已排最前，
     *    worker 按批次顺序抢任务，首屏仍是最先翻完的。
     * 2. 失败批自动**拆半重试**直到单条：一批 60 条里模型只吐了 50 条，
     *    旧逻辑整批留原文；现在只补翻缺的 10 条。
     * 3. SQLite 翻译记忆（模型维度）：同站常见短语（Submit/Settings…）命中即写，
     *    连模型都不用问；换模型自动失效。
     * 4. 收集期**预热**：模型名查询与 3 个独立会话在收集/查缓存期间就建好。
     *
     * 任何一批失败都继续往下走，不整页回滚——翻好的留在页面上，其余保留原文。
     */
    async translatePage(tabId?: string) {
      if (this.zhBusy) return
      this.error = null
      this.zhBusy = true
      this.zhCancel = false
      this.zhProgress = null

      // 并发路数：实测 3 路是甜点（6 路只 1.36x，云端会话固定开销反而摊不平）
      const CONC = 3
      // 本轮用过的翻译会话：结束（成功/失败/取消）统一删，控制台不堆积
      const usedTranslateSessions = new Set<string>()
      type Item = { id: string; text: string }
      type Markers = Record<string, { tag: string; a: string[][] }>
      type Seg = {
        id: string; kind: string; text: string; chars: number
        markers?: Markers | null
      }

      // 模型名（翻译记忆的隔离维度）与页面收集**并行**跑：一个本地请求、
      // 一个 DOM 遍历，谁也不等谁。模型取不到 = 本轮禁用缓存（绝不用空名读写）。
      const modelP = invokeOk<string>('opencode_translate_model')
        .then((m) => (typeof m === 'string' ? m : ''))
        .catch(() => '')

      try {
        // ---- 1. 收集 ----
        const col = await zhCall<{
          segments: Seg[]
          attrs: Array<{ id: string; attr: string; orig: string }>
          total: number
          attrTotal: number
          skipped: number
          title: string
        }>('page_collect_translatable', { tab_id: tabId ?? null })

        const segs = col.segments || []
        const attrs = col.attrs || []
        if (segs.length === 0 && attrs.length === 0) {
          this.error = '这页没找到需要翻译的文字（可能已经是中文，或是纯代码/数字页面）。'
          return
        }

        const items: Item[] = [
          ...segs.map((s) => ({ id: s.id, text: s.text })),
          // 属性条目用 x 前缀 id，跟文本节点区分开；写回时分开处理
          ...attrs.map((a) => ({ id: a.id, text: a.orig })),
        ]
        const origById = new Map(items.map((i) => [i.id, i.text]))
        const attrMeta = new Map(attrs.map((a) => [a.id, a]))
        const markersById = new Map<string, Markers>()
        for (const s of segs) if (s.markers) markersById.set(s.id, s.markers)

        const model = await modelP
        const cacheOn = !!model

        const lookupP: Promise<Array<{ id: string; text: string }>> = cacheOn
          ? invokeOk<Array<{ id: string; text: string }>>('translate_memory_lookup', {
              req: { model, items },
            })
              .then((h) => h ?? [])
              .catch(() => [])
          : Promise.resolve([])

        // ---- 2. 过滤缓存命中 → 未命中分批（后端算分批，前后端口径一致）----
        const hits = await lookupP
        const hitIds = new Set<string>()
        const cachePairs: Array<{ id: string; orig: string; translated: string; markers?: Markers }> = []
        const cacheAttrs: Array<{ attr: string; orig: string; translated: string }> = []
        for (const h of hits) {
          const orig = origById.get(h.id)
          if (orig === undefined || hitIds.has(h.id)) continue
          hitIds.add(h.id)
          if (h.text.trim() === orig.trim()) continue
          const meta = attrMeta.get(h.id)
          if (meta) cacheAttrs.push({ attr: meta.attr, orig: meta.orig, translated: h.text.trim() })
          else {
            const mk = markersById.get(h.id)
            cachePairs.push(
              mk
                ? { id: h.id, orig, translated: h.text.trim(), markers: mk }
                : { id: h.id, orig, translated: h.text.trim() },
            )
          }
        }
        const missItems = items.filter((i) => !hitIds.has(i.id))
        // 预热与分批并行：确认有未命中段才建会话，全命中页不白带 3 个空会话。
        // 预热失败无所谓：translateItems 拿不到就传 null，Rust 照常按需新建。
        const warmP =
          missItems.length === 0
            ? Promise.resolve([] as string[])
            : invokeOk<string[]>('opencode_translate_warmup', { want: CONC })
                .then((s) => (Array.isArray(s) ? s.filter((x) => typeof x === 'string') : []))
                .catch(() => [] as string[])
        const batches =
          missItems.length === 0
            ? []
            : await zhCall<Item[][]>('plan_translate_batches', {
                req: { items: missItems, page_title: col.title, model: cacheOn ? model : undefined },
              })
        const warmSessions = await warmP

        this.zhProgress = {
          done: cachePairs.length + cacheAttrs.length,
          total: items.length,
          batch: 0,
          batches: batches.length,
          preview: '',
        }

        // ---- 3. 共享状态：写回串行链 / 子块拼装表 / 计数 ----
        let appliedTotal = 0
        let mismatchedTotal = 0
        let keptTotal = 0
        let applyError: string | null = null
        let done = cachePairs.length + cacheAttrs.length
        let batchesDone = 0
        let fatalMsg: string | null = null
        // 本页新译文，收尾统一进翻译记忆（命中缓存的不重复写）
        const rememberPairs: Array<{
          id: string; orig: string; translated: string; markers?: Markers
        }> = []

        // 多个 worker 会同时翻完，但同一 webview 的 eval 写回必须串行：
        // 两条写回交叉执行，备份/校验的时序就没法推理了。
        let writeChain: Promise<void> = Promise.resolve()
        const scheduleApply = (
          pairs: typeof cachePairs,
          attrs: typeof cacheAttrs,
        ): Promise<void> => {
          const run = writeChain
            .then(async () => {
              if (pairs.length === 0 && attrs.length === 0) return
              const ap = await zhCall<{
                applied: number
                mismatched: number
                keptStructure?: number
              }>('page_apply_translation', {
                tab_id: tabId ?? null,
                mode: 'apply',
                payload: { pairs, attrs },
              })
              appliedTotal += ap.applied || 0
              mismatchedTotal += ap.mismatched || 0
              keptTotal += ap.keptStructure || 0
              if ((ap.applied || 0) > 0) this.zhApplied = true
            })
            .catch((e) => {
              // 一批写回失败不拖垮后续批次（每批都是独立页面操作）
              applyError = applyError || `部分译文写回失败：${String(e)}`
            })
          writeChain = run.then(
            () => undefined,
            () => undefined,
          )
          return run
        }

        // 缓存命中的译文立即贴到页面上——全命中时用户看到的是"秒翻"
        await scheduleApply(cachePairs, cacheAttrs)

        // 超长段在分批时被切成 `原id.序号` 子块：chunkSpec 记每个 base 的
        // 有序子块 id，chunkParts 收到一块记一块，齐了就拼成整段写回。
        const chunkSpec = new Map<string, string[]>()
        for (const b of batches) {
          for (const it of b) {
            const dot = it.id.lastIndexOf('.')
            if (dot === -1) continue
            const base = it.id.slice(0, dot)
            if (!origById.has(base)) continue
            const arr = chunkSpec.get(base)
            if (arr) arr.push(it.id)
            else chunkSpec.set(base, [it.id])
          }
        }
        const chunkParts = new Map<string, Map<number, string>>()
        // 已拼成整段（或半截）写回过的超长段 id，扫尾时不重复处理
        const chunkEmitted = new Set<string>()

        /**
         * 把整段译文转成"写回对"（文本/属性两条路）。
         * 模型偶尔把译文写成原文（尤其短词）：白改一遍还引起页面重排，跳过。
         */
        const emitBase = (
          baseId: string,
          translated: string,
        ): { pair?: (typeof cachePairs)[number]; attr?: (typeof cacheAttrs)[number] } => {
          const orig = origById.get(baseId)!
          if (translated.trim() === orig.trim()) return {}
          const meta = attrMeta.get(baseId)
          if (meta) {
            return { attr: { attr: meta.attr, orig: meta.orig, translated } }
          }
          const mk = markersById.get(baseId)
          return {
            pair: mk
              ? { id: baseId, orig, translated, markers: mk }
              : { id: baseId, orig, translated },
          }
        }

        /** 把一批模型结果分成"可立即写回的对"，子块齐了就拼成整段 */
        const acceptResults = async (
          got: Map<string, string>,
          requested: Item[],
        ): Promise<void> => {
          const readyPairs: typeof cachePairs = []
          const readyAttrs: typeof cacheAttrs = []
          for (const r of requested) {
            const t = got.get(r.id)
            if (t === undefined) continue
            const dot = r.id.lastIndexOf('.')
            if (dot !== -1 && chunkSpec.has(r.id.slice(0, dot))) {
              const base = r.id.slice(0, dot)
              const idx = Number(r.id.slice(dot + 1))
              let parts = chunkParts.get(base)
              if (!parts) {
                parts = new Map()
                chunkParts.set(base, parts)
              }
              parts.set(idx, t.trim())
              const spec = chunkSpec.get(base)!
              if (parts.size < spec.length) continue
              // 拼接顺序 == 分批顺序 == 原文顺序
              const translated = spec
                .map((cid) => parts!.get(Number(cid.slice(cid.lastIndexOf('.') + 1))) || '')
                .join('')
              const { pair, attr } = emitBase(base, translated)
              chunkEmitted.add(base)
              if (pair) {
                readyPairs.push(pair)
                rememberPairs.push(pair)
              }
              if (attr) {
                readyAttrs.push(attr)
                rememberPairs.push({ id: base, orig: origById.get(base)!, translated })
              }
              done++
              continue
            }
            const { pair, attr } = emitBase(r.id, t.trim())
            if (pair) {
              readyPairs.push(pair)
              rememberPairs.push(pair)
            }
            if (attr) {
              readyAttrs.push(attr)
              rememberPairs.push({ id: r.id, orig: origById.get(r.id)!, translated: t.trim() })
            }
            done++
          }
          // 每批翻完立刻贴：不用等整页跑完，视口首批在前，头几批贴完首屏就好
          await scheduleApply(readyPairs, readyAttrs)
          this.zhProgress = {
            ...this.zhProgress!,
            done,
            batch: ++batchesDone,
          }
        }

        /**
         * 翻译一组条目，返回 id → 译文（缺的不出现）。
         *
         * 每次调用都用**独立会话**（预热池里取一个，取不到就让 Rust 新建）：
         * 会话历史不复用 = 没有逐批 prefill 滚雪球。
         * 缺项自动拆半递归重试，直到单条——只补真正没翻出来的，不重复花钱。
         * 用过的会话记账，整页结束统一删（opencode 控制台不堆积）。
         */
        const translateItems = async (itemsB: Item[]): Promise<Map<string, string>> => {
          const out = new Map<string, string>()
          let res: {
            data?: Array<{ id: string; text: string }> | null
            session_id?: string | null
          }
          // 预热会话一个只用一次；用过的记下来，整页翻完统一删（会话复用实施）
          const warmSid = warmSessions.shift() ?? null
          if (warmSid) usedTranslateSessions.add(warmSid)
          try {
            res = await invokeOk<{
              ok?: boolean
              msg?: string | null
              data?: Array<{ id: string; text: string }> | null
              session_id?: string | null
            }>('opencode_translate_batch', {
              req: {
                items: itemsB,
                page_title: col.title,
                // 预热会话一个只用一次；消费完即弃，绝不复用成"长会话"
                session_id: warmSid,
              },
            })
          } catch (e) {
            // 传输/凭据层硬错：拆半重试也是白试，向上抛给 worker 中止
            throw new Error(String(e))
          }
          if (res?.session_id) usedTranslateSessions.add(res.session_id)
          for (const g of res?.data || []) out.set(g.id, g.text)
          if (this.zhCancel) return out
          const missing = itemsB.filter((i) => !out.has(i.id))
          if (missing.length === 0 || itemsB.length === 1) return out
          // 拆半重试：两瓣串行，避免重试请求和正常批次的并发叠加成风暴
          const half = Math.ceil(missing.length / 2)
          const a = await translateItems(missing.slice(0, half))
          a.forEach((v, k) => {
            if (!out.has(k)) out.set(k, v)
          })
          if (this.zhCancel) return out
          const b = await translateItems(missing.slice(half))
          b.forEach((v, k) => {
            if (!out.has(k)) out.set(k, v)
          })
          return out
        }

        // ---- 4. worker 池：按批次顺序抢任务（视口首批天然先被抢到）----
        let cursor = 0
        const stripPreview = (s: string) => s.replace(/⟦[^⟧]*⟧/g, '').slice(0, 40)
        const worker = async () => {
          while (!fatalMsg && !this.zhCancel) {
            const bi = cursor++
            if (bi >= batches.length) return
            this.zhProgress = {
              ...this.zhProgress!,
              preview: stripPreview(batches[bi][0]?.text || ''),
            }
            let got: Map<string, string>
            try {
              got = await translateItems(batches[bi])
            } catch (e) {
              fatalMsg = `第 ${bi + 1}/${batches.length} 批翻译失败：${String(e)}`
              return
            }
            await acceptResults(got, batches[bi])
          }
        }
        await Promise.all(
          Array.from({ length: Math.min(CONC, Math.max(batches.length, 1)) }, () => worker()),
        )

        // 超长段拆半重试后仍缺子块：按旧契约把**已翻到的部分**先拼起来贴上
        // （有半截译文也比整段留原文强）。缺失位置留空，不补原文；不进翻译记忆，
        // 避免把半截译文当成整段权威译文缓存。
        const partialPairs: typeof cachePairs = []
        const partialAttrs: typeof cacheAttrs = []
        for (const [base, spec] of chunkSpec) {
          if (chunkEmitted.has(base)) continue
          const parts = chunkParts.get(base)
          if (!parts || parts.size === 0) continue
          const translated = spec
            .map((cid) => parts.get(Number(cid.slice(cid.lastIndexOf('.') + 1))) || '')
            .join('')
          const { pair, attr } = emitBase(base, translated)
          chunkEmitted.add(base)
          if (pair) partialPairs.push(pair)
          if (attr) partialAttrs.push(attr)
          done++
        }
        await scheduleApply(partialPairs, partialAttrs)
        // 等所有排队中的 DOM 写回落袋
        await writeChain

        // ---- 5. 新译文入翻译记忆（失败静默：缓存只是加速，不影响本轮结果）----
        if (cacheOn && rememberPairs.length > 0) {
          try {
            await invokeOk<number>('translate_memory_remember', {
              req: { model, pairs: rememberPairs },
            })
          } catch {
            // 记日志是后端的事，前端不打扰用户
          }
        }

        // ---- 6. 收尾汇报 ----
        if (fatalMsg) this.error = fatalMsg
        if (applyError && !fatalMsg) this.error = applyError
        const failedCount = items.length - done
        if (appliedTotal > 0 || cachePairs.length > 0 || cacheAttrs.length > 0) {
          const hitNote =
            cachePairs.length + cacheAttrs.length > 0
              ? `（其中 ${cachePairs.length + cacheAttrs.length} 处来自本地翻译记忆）`
              : ''
          const skippedNote = mismatchedTotal ? `（${mismatchedTotal} 处页面已变动，跳过）` : ''
          const keptNote = keptTotal ? `（${keptTotal} 处为保链接/图片原样保留原文）` : ''
          this.messages.push({
            role: 'assistant',
            content:
              `已汉化本页：${appliedTotal} 处${hitNote}${skippedNote}${keptNote}。` +
              (failedCount > 0 ? `另有 ${failedCount} 处没翻出来，保留原文。` : '') +
              (this.zhCancel ? '已按你的要求停下。' : ''),
          })
        } else if (!fatalMsg) {
          this.error = '这批一句都没翻出来，页面保持原样。多半是页面本身没问题。'
        }
      } catch (e) {
        this.error = `整页汉化失败：${String(e)}`
      } finally {
        this.zhBusy = false
        this.zhProgress = null
        this.zhCancel = false
        // 收尾删翻译会话（成功/失败/取消都删；fire-and-forget，失败不扰民）
        if (usedTranslateSessions.size > 0) {
          const sids = [...usedTranslateSessions]
          usedTranslateSessions.clear()
          invokeOk<number>('opencode_translate_cleanup', { sessions: sids }).catch((e) => {
            console.error('[ai] 翻译会话清理失败:', e)
          })
        }
      }
    },

    /**
     * 翻译页面上选中的一段文字（不整页）。
     *
     * 走的是和整页汉化同一套 opencode 翻译链路（独立"网页汉化"会话），
     * 区别只在于：输入是选区那一段，输出贴在对话框里，**不动页面 DOM**。
     * 选不中或落在密码区就拿不到内容，直接给提示，不发请求。
     */
    async translateSelection(tabId?: string) {
      if (this.zhBusy) return
      this.error = null
      await this.captureSelection(tabId)
      const sel = this.selection
      if (!sel || !sel.text.trim()) {
        if (!this.error) this.error = '没读到选中内容——先在页面上选中要翻译的文字，再点这里。'
        return
      }
      const text = sel.text.trim()
      this.messages.push({
        role: 'user',
        content: `翻译选中内容（${text.length} 字）`,
      })
      try {
        const res = await invokeOk<{
          ok: boolean
          msg?: string | null
          data?: Array<{ id: string; text: string }> | null
          session_id?: string | null
        }>('opencode_translate_batch', {
          req: {
            items: [{ id: 's0', text }],
            page_title: sel.title || null,
            // 翻译选中是临时一次性请求，用新会话，不污染整页汉化的进度会话
            session_id: null,
          },
        })
        const got = res?.data || []
        if (got.length === 0) {
          this.error = res?.msg || '这段没翻出来，换一段再试。'
          return
        }
        this.messages.push({ role: 'assistant', content: got[0].text })
      } catch (e) {
        this.error = `翻译选中内容失败：${String(e)}`
      }
    },

    /** 还原当前页原文（DOM 改回去了） */
    async restorePage(tabId?: string) {
      if (this.zhBusy) return
      try {
        const r = await zhCall<{ restored: number }>('page_apply_translation', {
          tab_id: tabId ?? null,
          mode: 'restore',
          payload: null,
        })
        this.zhApplied = false
        this.messages.push({
          role: 'assistant',
          content: `已还原原文 ${r.restored} 处。`,
        })
      } catch (e) {
        this.error = `还原失败：${String(e)}`
      }
    },

    /** 停止汉化：当前批次跑完就停，不丢已翻好的部分 */
    stopTranslate() {
      this.zhCancel = true
    },

    /** 快捷操作：解释当前页 */
    async explainPage() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可解释的页面。' })
        return
      }
      await this.sendMessage(shortcutExplain(ctx.title, ctx.url))
    },

    /**
     * 快捷操作：带上下文的解释（三级降级组装，失败回退老一句）。
     * 与 summarizeWithContext 同路：显示存原文，模型侧用组装提示词。
     */
    async explainWithContext(tabId?: string) {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可解释的页面。' })
        return
      }
      const display = shortcutExplain(ctx.title, ctx.url)
      try {
        const built = await this.buildQaPrompt(display, tabId)
        if (!built) throw new Error('无可用上下文')
        this.qaSource = built.source
        await this.sendMessage(display, built.prompt)
      } catch {
        await this.explainPage()
      }
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
        this.error = humanizeError(e)
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
        // read_page_selection 是 Tauri command 桥（read_selection 本身只是
        // api.sock handler，前端 invoke 调不到）。键名是下划线：invoke 没有
        // Tauri 那套 camelCase 自动转换，写成 tabId 会静默失效。
        const r = await invokeOk<{
          has_selection: boolean
          text?: string
          title?: string
          url?: string
          rejected?: boolean
          hibernated?: boolean
          /** 后端给的是数组（可能多条），别当字符串用 */
          hints?: string[]
        }>('read_page_selection', {
          tab_id: tabId ?? null,
          expand: true,
        })

        if (r.hibernated) {
          this.selection = null
          this.error = r.hints?.join('；') || '标签已休眠，切回该标签后可再读选区'
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
        this.error = humanizeError(e)
      }
    },

    /**
     * 问答上下文三级降级：选区 → 正文全文 → 语义快照。
     *
     * 只读不改页面，每级失败静默降下一级，全空返回 null。
     * 返回的来源与覆盖标记供界面显示“本次用啥问的、读全没读全”。
     */
    async resolveQaContext(tabId?: string): Promise<{
      source: string
      coverage: string
      content: string
      title: string
      url: string
    } | null> {
      // 第一级：选区（用户明确选中的，优先级最高）
      await this.captureSelection(tabId)
      const selText = this.selection?.text?.trim() || ''
      if (selText) {
        const content = selText.slice(0, 4000)
        this.qaSource = '选区'
        return {
          source: '选区',
          coverage: selText.length > 4000 ? '已截断' : '全文',
          content,
          title: this.selection?.title || this.pageContext?.title || '',
          url: this.selection?.url || this.pageContext?.url || '',
        }
      }
      const pageUrl = this.pageContext?.url || this.selection?.url || ''
      const pageTitle = this.pageContext?.title || this.selection?.title || ''
      // 第二级：正文全文（出网抓取，需用户已在页面授权语境下调用）
      if (pageUrl) {
        try {
          const art = await invokeOk<{ title: string; text: string }>('content_extract', {
            url: pageUrl,
          })
          const t = (art?.text || '').trim()
          if (t) {
            this.qaSource = '正文全文'
            return {
              source: '正文全文',
              coverage: '全文',
              content: t.slice(0, 8000),
              title: art.title || pageTitle,
              url: pageUrl,
            }
          }
        } catch {
          // 抓取失败降下一级，不打扰用户
        }
      }
      // 第三级：语义快照（本地提取，不出网）
      try {
        const snap = await invokeOk<Snapshot>('content_snapshot')
        const t = (snap?.text || '').trim()
        if (t) {
          this.qaSource = '语义快照'
          return {
            source: '语义快照',
            coverage: '可能不全',
            content: t.slice(0, 8000),
            title: snap.title || pageTitle,
            url: snap.url || pageUrl,
          }
        }
      } catch {
        // 快照失败则无上下文可用
      }
      this.qaSource = null
      return null
    },

    /**
     * 组装问答提示词：三级上下文 + 后端单源模板。
     * 返回提示词与截断标记，调用方经现有 ai_chat 通道发送。
     */
    async buildQaPrompt(
      question: string,
      tabId?: string,
    ): Promise<{ prompt: string; source: string; truncated: boolean } | null> {
      const q = question.trim()
      if (!q) return null
      const ctx = await this.resolveQaContext(tabId)
      if (!ctx) {
        this.error = '当前页面没读到可用内容，切回标签等加载完再问。'
        return null
      }
      try {
        const r = await invokeOk<{ prompt: string; truncated: boolean }>('content_qa_prompt', {
          title: ctx.title,
          url: ctx.url,
          source: ctx.source,
          coverage: ctx.coverage,
          content: ctx.content,
          question: q,
        })
        return { prompt: r.prompt, source: ctx.source, truncated: !!r.truncated }
      } catch (e) {
        this.error = humanizeError(e)
        return null
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
      // 轮数封顶：同一会话超 50 轮另开新会话，上下文不无限滚大
      if (this.webqaRounds >= 50) {
        this.webqaSessionId = null
        this.webqaRounds = 0
      }
      this.webqaRounds += 1

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
            this.error = humanizeError(event.message || '本机 opencode 请求失败')
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
        this.error = humanizeError(e)
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
      this.webqaRounds = 0
      this.selection = null
      this.qaSource = null
      this.messages = []
      this.error = null
    },

    /** 清空对话 */
    clearConversation() {
      this.messages = []
      this.error = null
      this.streamingId = null
      this.qaSource = null
    },
  },
})

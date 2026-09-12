// AI 侧边栏状态管理（Pinia）
// 对接 AGNES AI（Rust ai_chat 命令，OpenAI 兼容）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'

export interface AIMessage {
  role: 'user' | 'assistant'
  content: string
}

interface AIState {
  messages: AIMessage[]
  loading: boolean
  /** 当前页面上下文（URL + 标题，供 AI 分析） */
  pageContext: { url: string; title: string } | null
  error: string | null
}

export const useAIStore = defineStore('ai', {
  state: (): AIState => ({
    messages: [],
    loading: false,
    pageContext: null,
    error: null,
  }),

  actions: {
    /** 设置当前页面上下文（调用方在打开侧边栏/切换标签时更新） */
    setPageContext(url: string, title: string) {
      this.pageContext = { url, title }
    },

    /** 发送消息（带系统提示 + 页面上下文） */
    async sendMessage(content: string) {
      const text = content.trim()
      if (!text || this.loading) return
      this.messages.push({ role: 'user', content: text })
      this.loading = true
      this.error = null
      try {
        const ctx = this.pageContext
        const system = ctx
          ? `你是 ShiftBrowser 内置 AI 助手。当前用户正在浏览的页面：标题「${ctx.title}」，地址 ${ctx.url}。回答要简洁实用，中文回复。`
          : '你是 ShiftBrowser 内置 AI 助手。回答要简洁实用，中文回复。'
        const reply = await invoke<string>('ai_chat', {
          messages: [
            { role: 'system', content: system },
            ...this.messages.map((m) => ({ role: m.role, content: m.content })),
          ],
        })
        this.messages.push({ role: 'assistant', content: reply })
      } catch (e) {
        this.error = String(e)
      } finally {
        this.loading = false
      }
    },

    /** 快捷操作：总结当前页 */
    async summarizePage() {
      const ctx = this.pageContext
      if (!ctx) {
        this.messages.push({ role: 'assistant', content: '当前没有可分析的页面。' })
        return
      }
      await this.sendMessage(`请总结当前页面「${ctx.title}」（${ctx.url}）的主要内容，用 5 条以内要点概括。`)
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
      await this.sendMessage(`请用通俗易懂的方式解释当前页面「${ctx.title}」（${ctx.url}）讲的是什么、对用户有什么价值。`)
    },

    /** 清空对话 */
    clearConversation() {
      this.messages = []
      this.error = null
    },
  },
})
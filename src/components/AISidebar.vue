<script setup lang="ts">
// AI 侧边栏：右侧分屏面板（不隐藏 webview，页面保持可见可交互，边看边描述）
// 流式输出：后端 SSE 逐块推送，前端实时渲染；回复支持 Markdown + 代码复制
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import { renderMarkdown, copyText } from '../utils/markdown'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ close: [] }>()

const aiStore = useAIStore()
const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()
const input = ref('')
const inputRef = ref<HTMLTextAreaElement | null>(null)
const listRef = ref<HTMLElement | null>(null)

// 内容区顶部偏移（标签栏 44 + 地址栏 42 + 书签栏 30 可显隐），与后端 CONTENT_TOP_OFFSET 一致
const topOffset = computed(() => {
  const bookmarkH = settingsStore.showBookmarksBar ? 30 : 0
  return 44 + 42 + bookmarkH
})

// 打开/关闭：分屏模式——不隐藏 webview，而是让 webview 让出右侧 AI_PANEL_WIDTH，
// 页面保持可见可交互（对话时能看着页面描述）；关闭时恢复全宽
watch(
  () => props.show,
  (v) => {
    invoke('set_webview_ai_panel', { active: v }).catch(() => {})
    if (v) {
      // 打开时同步当前页面上下文
      const tab = tabsStore.activeTab
      if (tab) aiStore.setPageContext(tab.url, tab.title)
      // 命令栏「问 AI」：打开即发问；否则自动聚焦输入框（配合后端把 GTK 焦点交给 HTML 层）
      nextTick(() => {
        const pending = aiStore.takePendingPrompt()
        if (pending) {
          aiStore.sendMessage(pending)
        } else {
          inputRef.value?.focus()
        }
      })
    }
  },
)

// 新消息/流式增量后滚动到底部
watch(
  () => aiStore.messages.map((m) => m.content.length).join(','),
  async () => {
    await nextTick()
    if (listRef.value) listRef.value.scrollTop = listRef.value.scrollHeight
  },
)

async function onSend() {
  // loading 期不发送也不清输入框，避免用户打了一半的内容被丢弃
  if (aiStore.loading) return
  const text = input.value
  if (!text.trim()) return
  input.value = ''
  await aiStore.sendMessage(text)
  // 发送失败把原文填回，用户不用重打
  if (aiStore.error) input.value = text
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault()
    onSend()
  }
}

/** 代码块复制按钮（事件委托） */
async function onListClick(e: MouseEvent) {
  const btn = (e.target as HTMLElement).closest('.code-copy') as HTMLElement | null
  if (!btn) return
  const encoded = btn.dataset.code
  if (!encoded) return
  let code = ''
  try {
    code = decodeURIComponent(encoded)
  } catch {
    return
  }
  const ok = await copyText(code)
  btn.textContent = ok ? '已复制 ✓' : '复制失败'
  setTimeout(() => {
    btn.textContent = '复制'
  }, 1500)
}

// ---- Agent 动作确认（阶段 1.4）：AI 请求浏览器动作时弹卡片，用户允许/拒绝 ----
interface AgentActionRequest {
  requestId: string
  description: string
}
interface AgentStep {
  step: number
  action: string
  selector?: string
  value?: string
  direction?: string
  url?: string
  ms?: number
}
interface MultiStepPlan {
  requestId: string
  description: string
  steps: AgentStep[]
}
const pendingAction = ref<AgentActionRequest | null>(null)
const pendingMultiStep = ref<MultiStepPlan | null>(null)

async function onAgentActionRequest(e: { payload: AgentActionRequest }) {
  pendingAction.value = e.payload
}
async function confirmAction(approved: boolean) {
  const req = pendingAction.value
  if (!req) return
  pendingAction.value = null
  try {
    await invoke('agent_confirm_action', { requestId: req.requestId, approved })
  } catch (err) {
    console.error('[agent] 确认回调失败:', err)
  }
}
async function confirmMultiStep(approved: boolean) {
  const plan = pendingMultiStep.value
  if (!plan) return
  pendingMultiStep.value = null
  try {
    await invoke('agent_confirm_multi_step', { requestId: plan.requestId, approved })
  } catch (err) {
    console.error('[agent] 多步确认回调失败:', err)
  }
}
async function onAgentMultiStepRequest(e: { payload: MultiStepPlan }) {
  pendingMultiStep.value = e.payload
}

let unlistenAction: (() => void) | null = null
onMounted(async () => {
  unlistenAction = await listen<AgentActionRequest>('agent-action-request', onAgentActionRequest)
  await listen<MultiStepPlan>('agent-multi-step-request', onAgentMultiStepRequest)
})
onUnmounted(() => {
  unlistenAction?.()
})
</script>

<template>
  <Transition name="slide">
  <div v-if="props.show" class="ai-overlay" @click.self="emit('close')">
    <div
      class="ai-sidebar"
      :style="{ top: topOffset + 'px', height: `calc(100vh - ${topOffset}px - 22px)` }"
    >
      <!-- 头部 -->
      <div class="ai-header">
        <span class="ai-title">🤖 AI 助手</span>
        <button class="ai-close" title="关闭" aria-label="关闭" @click="emit('close')">✕</button>
      </div>

      <!-- 快捷操作 -->
      <div class="ai-actions">
        <button class="ai-action" @click="aiStore.summarizePage()">📄 总结当前页</button>
        <button class="ai-action" @click="aiStore.translatePage()">🌐 翻译当前页</button>
        <button class="ai-action" @click="aiStore.explainPage()">💡 解释当前页</button>
      </div>

      <!-- 对话区 -->
      <div ref="listRef" class="ai-list" @click="onListClick">
        <div v-if="aiStore.messages.length === 0" class="ai-empty">
          我是 ShiftBrowser 内置 AI 助手，可以聊天、总结当前页面、翻译、解释网页内容。
        </div>
        <div
          v-for="(m, i) in aiStore.messages"
          :key="i"
          class="ai-msg"
          :class="m.role === 'user' ? 'user' : 'assistant'"
        >
          <div class="ai-bubble">
            <div v-if="m.role === 'user'" class="md-plain">{{ m.content }}</div>
            <div v-else class="md-body" v-html="renderMarkdown(m.content)"></div>
          </div>
        </div>
        <div v-if="aiStore.loading && !aiStore.streamingId" class="ai-msg assistant">
          <div class="ai-bubble ai-thinking"><span class="spinner"></span> 思考中…</div>
        </div>
        <div v-if="aiStore.error" class="ai-error">{{ aiStore.error }}</div>
      </div>

      <!-- Agent 动作确认卡片（阶段 1.4：每步人工确认） -->
      <div v-if="pendingAction" class="agent-confirm">
        <div class="agent-confirm-title">🤖 AI 想执行操作</div>
        <div class="agent-confirm-desc">{{ pendingAction.description }}</div>
        <div class="agent-confirm-btns">
          <button class="agent-confirm-allow" @click="confirmAction(true)">允许</button>
          <button class="agent-confirm-deny" @click="confirmAction(false)">拒绝</button>
        </div>
      </div>

      <!-- 多步任务确认卡片（阶段 1.6：多步编排提示词） -->
      <div v-if="pendingMultiStep" class="agent-confirm">
        <div class="agent-confirm-title">🤖 AI 想执行多步任务</div>
        <div class="agent-confirm-desc">{{ pendingMultiStep.description }}</div>
        <div class="multi-step-list">
          <div v-for="step in pendingMultiStep.steps" :key="step.step" class="multi-step-item">
            {{ step.step }}. {{ step.action }} {{ step.selector ? '「' + step.selector + '」' : '' }} {{ step.value ? '填「' + step.value + '」' : '' }}
          </div>
        </div>
        <div class="agent-confirm-btns">
          <button class="agent-confirm-allow" @click="confirmMultiStep(true)">允许全部</button>
          <button class="agent-confirm-deny" @click="confirmMultiStep(false)">拒绝</button>
        </div>
      </div>

      <!-- 输入区 -->
      <div class="ai-input-bar">
        <textarea
          ref="inputRef"
          v-model="input"
          class="ai-input"
          placeholder="输入问题，Enter 发送，Shift+Enter 换行"
          rows="1"
          @keydown="onKeydown"
        ></textarea>
        <button v-if="aiStore.loading" class="ai-stop" @click="aiStore.stopStreaming()">
          停止
        </button>
        <button v-else class="ai-send" :disabled="!input.trim()" @click="onSend">
          发送
        </button>
      </div>
      <div class="ai-footer">
        <button class="ai-clear" @click="aiStore.clearConversation()">清空对话</button>
        <span class="ai-hint">
          {{ aiStore.tools.length > 0 ? `🔧 ${aiStore.tools.length} 个工具可用` : '由 AGNES AI 驱动' }}
        </span>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.ai-overlay {
  position: fixed;
  inset: 0;
  /* 分屏模式：遮罩透明，页面（webview 让出右侧后）保持可见可交互 */
  background: transparent;
  z-index: 1000;
  height: 100vh;
  min-height: 100vh;
}
.ai-sidebar {
  position: fixed;
  right: 0;
  width: min(380px, 100vw);
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
  box-shadow: -8px 0 32px rgba(0, 0, 0, 0.5);
  border-radius: 12px 0 0 12px;
}
.ai-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-subtle);
}
.ai-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}
.ai-close {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 14px;
  cursor: pointer;
  border-radius: 4px;
}
.ai-close:hover {
  background: #e53935;
  color: var(--text-primary);
}
.ai-actions {
  display: flex;
  gap: 6px;
  padding: 10px 12px;
  border-bottom: 1px solid var(--bg-secondary);
  flex-wrap: wrap;
}
.ai-action {
  padding: 5px 10px;
  font-size: 12px;
  color: var(--accent);
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 14px;
  cursor: pointer;
  transition: background 0.15s;
}
.ai-action:hover {
  background: var(--bg-hover);
}
.ai-list {
  flex: 1;
  overflow-y: auto;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.ai-empty {
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 24px 12px;
  line-height: 1.6;
}
.ai-msg {
  display: flex;
}
.ai-msg.user {
  justify-content: flex-end;
}
.ai-msg.assistant {
  justify-content: flex-start;
}
.ai-bubble {
  max-width: 85%;
  padding: 8px 12px;
  border-radius: 10px;
  font-size: 13px;
  line-height: 1.6;
  white-space: pre-wrap;
  word-break: break-word;
}
.ai-msg.user .ai-bubble {
  background: var(--accent);
  color: var(--text-primary);
  border-bottom-right-radius: 2px;
}
.ai-msg.assistant .ai-bubble {
  background: var(--bg-secondary);
  color: var(--text-primary);
  border-bottom-left-radius: 2px;
}
.ai-thinking {
  color: var(--text-muted);
  font-style: italic;
  display: flex;
  align-items: center;
  gap: 8px;
}
.ai-error {
  color: #e53935;
  font-size: 12px;
  padding: 6px 12px;
  background: rgba(229, 57, 53, 0.1);
  border-radius: 6px;
}
.ai-input-bar {
  display: flex;
  gap: 8px;
  padding: 10px 12px;
  border-top: 1px solid var(--bg-secondary);
  align-items: flex-end;
}
.ai-input {
  flex: 1;
  min-height: 32px;
  max-height: 100px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  color: var(--text-primary);
  font-size: 13px;
  padding: 6px 10px;
  resize: none;
  outline: none;
  font-family: inherit;
}
.ai-input:focus {
  border-color: var(--accent);
}
.ai-send {
  height: 32px;
  padding: 0 14px;
  background: var(--accent);
  color: var(--text-primary);
  border: none;
  border-radius: 8px;
  font-size: 13px;
  cursor: pointer;
  flex-shrink: 0;
}
.ai-send:disabled {
  opacity: 0.5;
  cursor: default;
}
.ai-stop {
  height: 32px;
  padding: 0 14px;
  background: #e53935;
  color: var(--text-primary);
  border: none;
  border-radius: 8px;
  font-size: 13px;
  cursor: pointer;
  flex-shrink: 0;
}
.ai-stop:hover {
  background: #d32f2f;
}
/* ---- Markdown 渲染样式 ---- */
.md-plain {
  white-space: pre-wrap;
  word-break: break-word;
}
.md-body {
  font-size: 13px;
  line-height: 1.65;
  word-break: break-word;
}
.md-body :deep(p) {
  margin: 4px 0;
}
.md-body :deep(h1),
.md-body :deep(h2),
.md-body :deep(h3),
.md-body :deep(h4),
.md-body :deep(h5),
.md-body :deep(h6) {
  margin: 8px 0 4px;
  font-weight: 600;
  line-height: 1.4;
}
.md-body :deep(h1) { font-size: 16px; }
.md-body :deep(h2) { font-size: 15px; }
.md-body :deep(h3) { font-size: 14px; }
.md-body :deep(h4),
.md-body :deep(h5),
.md-body :deep(h6) { font-size: 13px; }
.md-body :deep(ul),
.md-body :deep(ol) {
  margin: 4px 0;
  padding-left: 20px;
}
.md-body :deep(li) {
  margin: 2px 0;
}
.md-body :deep(code) {
  font-family: 'JetBrains Mono', 'Fira Code', Consolas, monospace;
  font-size: 12px;
  background: rgba(127, 127, 127, 0.18);
  padding: 1px 4px;
  border-radius: 4px;
}
.md-body :deep(blockquote) {
  margin: 6px 0;
  padding: 4px 10px;
  border-left: 3px solid var(--accent);
  color: var(--text-muted);
  background: rgba(127, 127, 127, 0.1);
  border-radius: 0 6px 6px 0;
}
.md-body :deep(a) {
  color: var(--accent);
  text-decoration: underline;
}
.md-body :deep(.code-block) {
  margin: 6px 0;
  border-radius: 8px;
  overflow: hidden;
  border: 1px solid var(--border-color);
  background: #0d1117;
}
.md-body :deep(.code-header) {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 4px 8px;
  background: #161b22;
  border-bottom: 1px solid var(--border-color);
}
.md-body :deep(.code-lang) {
  font-size: 11px;
  color: var(--text-muted);
  font-family: 'JetBrains Mono', Consolas, monospace;
}
.md-body :deep(.code-copy) {
  font-size: 11px;
  padding: 2px 8px;
  border: none;
  border-radius: 4px;
  background: rgba(127, 127, 127, 0.25);
  color: var(--text-primary);
  cursor: pointer;
}
.md-body :deep(.code-copy:hover) {
  background: rgba(127, 127, 127, 0.4);
}
.md-body :deep(pre) {
  margin: 0;
  padding: 8px 10px;
  overflow-x: auto;
}
.md-body :deep(pre code) {
  background: transparent;
  padding: 0;
  font-size: 12px;
  line-height: 1.6;
  color: #e6edf3;
  white-space: pre;
}
.md-body :deep(.md-blank) {
  height: 6px;
}
.ai-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px 10px;
}
.ai-clear {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  cursor: pointer;
}
.ai-clear:hover {
  color: #e53935;
}
.ai-hint {
  color: var(--text-muted);
  font-size: 11px;
}
/* Agent 动作确认卡片（阶段 1.4） */
.agent-confirm {
  margin: 8px 12px;
  padding: 10px 12px;
  background: var(--bg-primary);
  border: 1px solid #e8a33d;
  border-radius: 10px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}
.agent-confirm-title {
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 4px;
}
.agent-confirm-desc {
  font-size: 12px;
  color: var(--text-secondary);
  margin-bottom: 8px;
  word-break: break-all;
}
.agent-confirm-btns {
  display: flex;
  gap: 8px;
}
.agent-confirm-allow {
  flex: 1;
  padding: 6px 0;
  border: none;
  border-radius: 6px;
  background: #2f9e44;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
}
.agent-confirm-deny {
  flex: 1;
  padding: 6px 0;
  border: none;
  border-radius: 6px;
  background: #e03131;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
}
</style>

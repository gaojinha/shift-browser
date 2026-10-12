<script setup lang="ts">
// AI 侧边栏：右侧分屏面板（不隐藏 webview，页面保持可见可交互，边看边描述）
// 流式输出：后端 SSE 逐块推送，前端实时渲染；回复支持 Markdown + 代码复制
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invokeOk } from '../utils/invoke'
import { listen } from '@tauri-apps/api/event'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import { renderMarkdown, copyText } from '../utils/markdown'
// 布局常量单一来源（build.rs 由 src-tauri/src/constants.rs 生成）
import { EVENTS, LAYOUT, SKILLS } from '../constants'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ close: [] }>()

const aiStore = useAIStore()
const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()
const input = ref('')
const inputRef = ref<HTMLTextAreaElement | null>(null)
const listRef = ref<HTMLElement | null>(null)

// 内容区顶部偏移（标签栏 + 地址栏 + 书签栏可显隐）——常量来自 constants.ts，
// 与后端 CONTENT_TOP_OFFSET 同源，不再手工对齐。
// 垂直标签模式标签栏在左，顶部不占高度（与 Rust top_offset 同算法，差 38px 错位回归单测见后端）。
const topOffset = computed(() => {
  const vertical = settingsStore.get('vertical_tabs') === 'true'
  const tabH = vertical ? 0 : LAYOUT.tabBar
  const bookmarkH = settingsStore.showBookmarksBar ? LAYOUT.bookmarksBar : 0
  return tabH + LAYOUT.addressBar + bookmarkH
})

/**
 * 汉化进度百分比：按**段数**算。
 * 并发 worker 后"第几批"完成顺序不固定，按批算会倒跳；段数单调递增。
 * 缓存命中的段在开工前就计入 done，所以进度条可能不是从 0 起步——这是对的。
 */
const zhPercent = computed(() => {
  const p = aiStore.zhProgress
  if (!p || p.total === 0) return 0
  return Math.min(100, Math.round((p.done / p.total) * 100))
})

// 打开/关闭：分屏模式——不隐藏 webview，而是让 webview 让出右侧 AI_PANEL_WIDTH，
// 页面保持可见可交互（对话时能看着页面描述）；关闭时恢复全宽
watch(
  () => props.show,
  (v) => {
    // 有意静默：child webview 布局同步失败只影响避让宽度，下次开关/切换标签会重发自愈
    invokeOk('set_webview_ai_panel', { active: v }).catch(() => {})
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
  // 网页随手问模式走本机 opencode，否则走 AGNES 云端
  if (aiStore.webqaMode) {
    await aiStore.askPage(text)
  } else {
    await aiStore.sendMessage(text)
  }
  // 发送失败把原文填回，用户不用重打
  if (aiStore.error) input.value = text
}

/** 进入「网页随手问」：探测本机 opencode + 抓当前选区 */
async function enterWebqa() {
  aiStore.webqaMode = true
  aiStore.messages = []
  aiStore.error = null
  await aiStore.probeOpencode()
  await aiStore.captureSelection(tabsStore.activeTab?.id)
  nextTick(() => inputRef.value?.focus())
}

/** 停止：按当前模式调对应的取消命令 */
function onStop() {
  if (aiStore.webqaMode) {
    aiStore.stopWebqa()
  } else {
    aiStore.stopStreaming()
  }
}

function onKeydown(e: KeyboardEvent) {
  // 组字中按 Enter 是选词，不是发送
  if (e.isComposing) return
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
    await invokeOk('agent_confirm_action', { requestId: req.requestId, approved })
  } catch (err) {
    console.error('[agent] 确认回调失败:', err)
  }
}
async function confirmMultiStep(approved: boolean) {
  const plan = pendingMultiStep.value
  if (!plan) return
  pendingMultiStep.value = null
  try {
    await invokeOk('agent_confirm_multi_step', { requestId: plan.requestId, approved })
  } catch (err) {
    console.error('[agent] 多步确认回调失败:', err)
  }
}
async function onAgentMultiStepRequest(e: { payload: MultiStepPlan }) {
  pendingMultiStep.value = e.payload
}

let unlistenAction: (() => void) | null = null
let unlistenMultiStep: (() => void) | null = null
onMounted(async () => {
  unlistenAction = await listen<AgentActionRequest>(EVENTS.AGENT_ACTION_REQUEST, onAgentActionRequest)
  unlistenMultiStep = await listen<MultiStepPlan>(EVENTS.AGENT_MULTI_STEP_REQUEST, onAgentMultiStepRequest)
})
onUnmounted(() => {
  unlistenAction?.()
  unlistenMultiStep?.()
  unlistenAction = null
  unlistenMultiStep = null
})

/** 技能分发：技能清单来自 constants.ts（源：contracts::skills），按钮由数据驱动 */
function runSkill(id: string) {
  const handlers: Record<string, () => void> = {
    summarize_page: () => aiStore.summarizeWithContext(),
    translate_page: () => aiStore.translatePage(tabsStore.activeTabId ?? undefined),
    translate_selection: () => aiStore.translateSelection(tabsStore.activeTabId ?? undefined),
    explain_page: () => aiStore.explainWithContext(tabsStore.activeTabId ?? undefined),
    page_qa: () => inputRef.value?.focus(),
  }
  handlers[id]?.()
}

function onAiClose() {
  emit('close')
}
function onOverlaySelf() {
  emit('close')
}
</script>

<template>
  <Transition name="slide">
  <div v-if="props.show" class="ai-overlay" @click.self="onOverlaySelf">
    <div
      class="ai-sidebar"
      :style="{ top: topOffset + 'px', height: `calc(100vh - ${topOffset}px - ${LAYOUT.statusBar}px)` }"
    >
      <!-- 头部 -->
      <div class="ai-header">
        <span class="ai-title">🤖 AI 助手</span>
        <button class="ai-close" title="关闭" aria-label="关闭" @click="onAiClose">✕</button>
      </div>

      <!-- 快捷操作 -->
      <div class="ai-actions">
        <button
          v-for="s in SKILLS"
          :key="s.id"
          class="ai-action"
          :title="s.desc"
          :disabled="s.id === 'translate_page' && aiStore.zhBusy"
          @click="runSkill(s.id)"
        >
          {{ s.icon }} {{ s.name }}
        </button>
      </div>

      <!-- 整页汉化：进度 + 还原 -->
      <div v-if="aiStore.zhBusy || aiStore.zhApplied" class="zh-panel">
        <template v-if="aiStore.zhBusy && aiStore.zhProgress">
          <div class="zh-row">
            <span class="zh-text">
              汉化中…已完成 {{ aiStore.zhProgress.batch }}/{{ aiStore.zhProgress.batches }} 批，已翻
              {{ aiStore.zhProgress.done }}/{{ aiStore.zhProgress.total }} 处
            </span>
            <button class="zh-stop" @click="aiStore.stopTranslate()">停止</button>
          </div>
          <div class="zh-bar">
            <div
              class="zh-bar-fill"
              :style="{ width: zhPercent + '%' }"
            ></div>
          </div>
          <div v-if="aiStore.zhProgress.preview" class="zh-preview">
            正在处理：{{ aiStore.zhProgress.preview }}…
          </div>
        </template>
        <div v-else-if="aiStore.zhApplied" class="zh-row">
          <span class="zh-text">本页已汉化，原文已备份在页面里</span>
          <button class="zh-restore" @click="aiStore.restorePage(tabsStore.activeTabId ?? undefined)">
            还原原文
          </button>
        </div>
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
          aria-label="AI 提问输入框"
          :placeholder="aiStore.webqaMode ? '问本机 opencode：Enter 发送，Shift+Enter 换行' : '输入问题，Enter 发送，Shift+Enter 换行'"
          rows="1"
          @keydown="onKeydown"
        ></textarea>
        <button v-if="aiStore.loading" class="ai-stop" @click="onStop">停止</button>
        <button v-else class="ai-send" :disabled="!input.trim()" @click="onSend">发送</button>
      </div>
      <div class="ai-footer">
        <button class="ai-clear" @click="aiStore.clearConversation()">清空对话</button>
        <button
          v-if="!aiStore.webqaMode"
          class="ai-clear ai-webqa"
          title="用本机 opencode 回答，只带上你选中的内容（不出本机）"
          @click="enterWebqa"
        >
          网页随手问
        </button>
        <button v-else class="ai-clear ai-webqa on" @click="aiStore.exitWebqa()">
          退出随手问
        </button>
        <span class="ai-hint">
          <template v-if="aiStore.webqaMode">
            <template v-if="aiStore.webqaProbe?.askAgentFallbackRisk">
              ⚠ 只读 agent 不可用，已拦下发问
            </template>
            <template v-else-if="aiStore.selection">
              📎 已带上选区（{{ aiStore.selection.text.length }} 字）· {{ aiStore.webqaProbe?.privacy }}
            </template>
            <template v-else-if="aiStore.webqaProbe">
              🏠 本机 opencode · {{ aiStore.webqaProbe.model }}
            </template>
            <template v-else> 🏠 本机 opencode 未连通 </template>
          </template>
          <template v-else>
            <template v-if="aiStore.qaSource">📄 本次上下文：{{ aiStore.qaSource }}</template>
            <template v-else>{{ aiStore.tools.length > 0 ? `🔧 ${aiStore.tools.length} 个工具可用` : '由 AGNES AI 驱动' }}</template>
          </template>
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
  /* 右侧面板层：高于弹窗（弹窗与侧栏互斥，不叠加），与翻译面板同层， ordering 不再靠挂载顺序碰运气 */
  z-index: var(--z-sidepanel);
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
  background: var(--danger);
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
.ai-action:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 整页汉化进度面板 */
.zh-panel {
  padding: 8px 12px;
  border-bottom: 1px solid var(--bg-secondary);
  background: var(--bg-secondary);
}
.zh-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.zh-text {
  font-size: 12px;
  color: var(--text-primary);
  line-height: var(--leading-base);
}
.zh-bar {
  margin-top: 6px;
  height: 4px;
  border-radius: 2px;
  background: var(--border-color);
  overflow: hidden;
}
.zh-bar-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 2px;
  transition: width 0.3s ease;
}
.zh-preview {
  margin-top: 4px;
  font-size: 11px;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.zh-stop,
.zh-restore {
  flex-shrink: 0;
  padding: 3px 10px;
  font-size: 12px;
  color: var(--bg-primary);
  background: var(--accent);
  border: none;
  border-radius: 12px;
  cursor: pointer;
}
.zh-restore {
  color: var(--accent);
  background: transparent;
  border: 1px solid var(--border-color);
}
.zh-restore:hover {
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
  line-height: var(--leading-base);
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
  line-height: var(--leading-base);
  white-space: pre-wrap;
  word-break: break-word;
}
.ai-msg.user .ai-bubble {
  background: var(--accent);
  color: var(--accent-contrast);
  border-bottom-right-radius: 2px;
}
.ai-msg.assistant .ai-bubble {
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
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
  color: var(--danger);
  font-size: 12px;
  padding: 6px 12px;
  background: var(--danger-soft);
  border: 1px solid var(--danger);
  border-radius: var(--radius-xs);
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
  background: var(--danger);
  color: var(--accent-contrast);
  border: none;
  border-radius: var(--radius-sm);
  font-size: var(--text-base);
  cursor: pointer;
  flex-shrink: 0;
}
.ai-stop:hover {
  filter: brightness(0.85);
}
/* ---- Markdown 渲染样式 ---- */
.md-plain {
  white-space: pre-wrap;
  word-break: break-word;
}
.md-body {
  font-size: 13px;
  line-height: var(--leading-relaxed);
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
  line-height: var(--leading-tight);
}
/* 内容标题阶梯（渲染正文用，h1 16 > h2 15 > h3 14 > h4-h6 13，不跟弹窗标题统一） */
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
  border-radius: var(--radius-sm);
  overflow: hidden;
  border: 1px solid var(--border-color);
  /* 代码块恒用暗色主题，深浅模式下不变，保证代码可读 */
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
  font-size: var(--text-xs);
  color: var(--text-muted);
  font-family: var(--font-mono);
}
.md-body :deep(.code-copy) {
  font-size: var(--text-xs);
  padding: 2px 8px;
  border: none;
  border-radius: var(--radius-2xs);
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
  font-size: var(--text-sm);
  line-height: var(--leading-base);
  /* 代码恒用浅色字配暗底，深浅模式下不变 */
  color: #e6edf3;
  white-space: pre;
}
.md-body :deep(.md-blank) {
  height: 6px;
}
.ai-footer {
  display: flex;
  align-items: center;
  gap: 8px;
  /* 网页随手问状态提示可能较长（选了区 + 隐私档），单行会挤爆按钮 */
  flex-wrap: wrap;
  padding: 6px 12px 10px;
}
.ai-clear {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: var(--text-sm);
  cursor: pointer;
  white-space: nowrap;
}
.ai-clear:hover {
  color: var(--danger);
}
/* 随手问入口是主动作，给个可点的样子，别和"清空对话"一样灰 */
.ai-webqa {
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-xs);
  padding: 2px 8px;
  color: var(--accent, #4a9eff);
}
.ai-webqa:hover {
  border-color: var(--accent, #4a9eff);
  color: var(--accent, #4a9eff);
}
.ai-webqa.on {
  background: var(--bg-primary);
}
.ai-hint {
  color: var(--text-muted);
  font-size: var(--text-xs);
  /* 状态提示可换行，按钮不跟着挪位置 */
  flex: 1 1 100%;
  text-align: right;
}
/* Agent 动作确认卡片（阶段 1.4） */
.agent-confirm {
  margin: 8px 12px;
  padding: 10px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--warning);
  border-radius: var(--radius-md);
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.15);
}
.agent-confirm-title {
  font-size: var(--text-base);
  font-weight: 600;
  margin-bottom: 4px;
}
.agent-confirm-desc {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  margin-bottom: 8px;
  word-break: break-all;
}
.agent-confirm-btns {
  display: flex;
  gap: var(--space-sm);
}
.agent-confirm-allow {
  flex: 1;
  padding: 6px 0;
  border: none;
  border-radius: var(--radius-xs);
  background: var(--success);
  color: var(--accent-contrast);
  font-size: var(--text-base);
  cursor: pointer;
}
.agent-confirm-deny {
  flex: 1;
  padding: 6px 0;
  border: none;
  border-radius: var(--radius-xs);
  background: var(--danger);
  color: var(--accent-contrast);
  font-size: var(--text-base);
  cursor: pointer;
}
</style>

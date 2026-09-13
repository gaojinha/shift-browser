<script setup lang="ts">
// AI 侧边栏：右侧滑出面板，对话 + 快捷操作（总结/翻译/解释当前页）
import { nextTick, ref, watch } from 'vue'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'
import { invoke } from '@tauri-apps/api/core'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ close: [] }>()

const aiStore = useAIStore()
const tabsStore = useTabsStore()
const input = ref('')
const listRef = ref<HTMLElement | null>(null)

// 弹窗打开时隐藏 GTK 层 webview（HTML 侧边栏盖不住 webview），关闭时恢复
watch(
  () => props.show,
  (v) => {
    invoke('set_webview_visible', { visible: !v }).catch(() => {})
    if (v) {
      // 打开时同步当前页面上下文
      const tab = tabsStore.activeTab
      if (tab) aiStore.setPageContext(tab.url, tab.title)
    }
  },
)

// 新消息后滚动到底部
watch(
  () => aiStore.messages.length,
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
</script>

<template>
  <Transition name="slide">
  <div v-if="props.show" class="ai-overlay" @click.self="emit('close')">
    <div class="ai-sidebar">
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
      <div ref="listRef" class="ai-list">
        <div v-if="aiStore.messages.length === 0" class="ai-empty">
          我是 ShiftBrowser 内置 AI 助手，可以聊天、总结当前页面、翻译、解释网页内容。
        </div>
        <div
          v-for="(m, i) in aiStore.messages"
          :key="i"
          class="ai-msg"
          :class="m.role === 'user' ? 'user' : 'assistant'"
        >
          <div class="ai-bubble">{{ m.content }}</div>
        </div>
        <div v-if="aiStore.loading" class="ai-msg assistant">
          <div class="ai-bubble ai-thinking"><span class="spinner"></span> 思考中…</div>
        </div>
        <div v-if="aiStore.error" class="ai-error">{{ aiStore.error }}</div>
      </div>

      <!-- 输入区 -->
      <div class="ai-input-bar">
        <textarea
          v-model="input"
          class="ai-input"
          placeholder="输入问题，Enter 发送，Shift+Enter 换行"
          rows="2"
          @keydown="onKeydown"
        ></textarea>
        <button class="ai-send" :disabled="aiStore.loading || !input.trim()" @click="onSend">
          发送
        </button>
      </div>
      <div class="ai-footer">
        <button class="ai-clear" @click="aiStore.clearConversation()">清空对话</button>
        <span class="ai-hint">由 AGNES AI 驱动</span>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.ai-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  z-index: 1000;
  height: 100vh;
  min-height: 100vh;
}
.ai-sidebar {
  position: fixed;
  right: 0;
  top: 0;
  width: min(380px, 100vw);
  height: 100vh;
  max-height: 100vh;
  background: var(--bg-secondary);
  border-left: 1px solid var(--border-subtle);
  display: flex;
  flex-direction: column;
  box-shadow: -8px 0 32px rgba(0, 0, 0, 0.5);
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
  min-height: 40px;
  max-height: 100px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  color: var(--text-primary);
  font-size: 13px;
  padding: 8px 10px;
  resize: none;
  outline: none;
  font-family: inherit;
}
.ai-input:focus {
  border-color: var(--accent);
}
.ai-send {
  height: 38px;
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
</style>

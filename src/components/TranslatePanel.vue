<script setup lang="ts">
// 翻译面板：右侧滑出，粘贴文本 → 调 Rust 翻译 API → 显示译文
// 原版 Electron 翻译功能的核心场景——选不中文本复制到这里翻译
import { computed, ref, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { humanizeError, TRANSLATE_PANEL_HINT } from '../frontend-prompts'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useAIStore } from '../stores/ai'
import { useTabsStore } from '../stores/tabs'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ close: [] }>()

const settingsStore = useSettingsStore()
/** 当前翻译源标签（设置 → 通用 可切换 Google / Bing） */
const engineLabel = computed(() =>
  settingsStore.get('translate_engine') === 'bing' ? 'Bing' : 'Google',
)
/** 翻译目标语言（设置 → 通用 → 翻译目标语言，默认中文简体） */
const targetLang = computed(() => settingsStore.get('translate_target') || 'zh-CN')
const targetLabel = computed(() => {
  const map: Record<string, string> = {
    'zh-CN': '中文（简体）',
    'zh-TW': '中文（繁体）',
    en: '英语',
    ja: '日语',
    ko: '韩语',
    fr: '法语',
    de: '德语',
    ru: '俄语',
    es: '西班牙语',
    pt: '葡萄牙语',
    it: '意大利语',
  }
  return map[targetLang.value] || targetLang.value
})

const input = ref('')
const output = ref('')
const loading = ref(false)
const error = ref('')
const elapsedMs = ref(0)

/** 整页汉化走 AI 侧栏（译文直接长在页面上）；本面板只做粘贴机翻 */
async function translatePage() {
  error.value = ''
  try {
    const tabId = useTabsStore().activeTab?.id ?? undefined
    await useAIStore().translatePage(tabId)
    emit('close') // 关掉面板，让用户直接看页面上的汉化进度
  } catch (e) {
    error.value = humanizeError(e)
  }
}

// 弹窗打开时隐藏 webview（HTML 侧边栏盖不住 webview），关闭时恢复
watch(
  () => props.show,
  (v) => {
    setWebviewVisible(!v, 'translate-panel')
    if (v) {
      // 自动聚焦输入框
      nextTick(() => {
        const ta = document.querySelector('.tr-textarea') as HTMLTextAreaElement | null
        ta?.focus()
      })
    }
  },
)

async function doTranslate() {
  const text = input.value.trim()
  if (!text || loading.value) return
  loading.value = true
  error.value = ''
  output.value = ''
  elapsedMs.value = 0
  const start = performance.now()
  const tick = window.setInterval(() => {
    elapsedMs.value = Math.round(performance.now() - start)
  }, 50)
  try {
    const r = await invoke<{ ok: boolean; msg?: string; data?: string }>(
      'translate_text',
      { text, target: targetLang.value },
    )
    window.clearInterval(tick)
    if (r && r.ok && r.data) {
      output.value = r.data
    } else {
      error.value = (r && r.msg) || '翻译失败'
    }
  } catch (e: unknown) {
    window.clearInterval(tick)
    error.value = humanizeError(e) || '翻译失败'
  } finally {
    loading.value = false
    elapsedMs.value = Math.round(performance.now() - start)
  }
}

async function copyOutput() {
  const txt = output.value
  if (!txt) return
  try {
    await navigator.clipboard.writeText(txt)
    flashCopied()
  } catch {
    // 兜底
    const ta = document.createElement('textarea')
    ta.value = txt
    ta.style.position = 'fixed'
    ta.style.left = '-9999px'
    document.body.appendChild(ta)
    ta.select()
    // 有意静默：execCommand('copy') 在剪贴板权限受限时会抛错，此时降级为用户手动选中文本复制
    try { document.execCommand('copy') } catch { /* 见上行注释 */ }
    ta.remove()
    flashCopied()
  }
}

const copied = ref(false)
function flashCopied() {
  copied.value = true
  setTimeout(() => { copied.value = false }, 1200)
}

function onKeydown(e: KeyboardEvent) {
  // 组字中：Esc 退组字、Ctrl+Enter 非发送意图，全交给输入法
  if (e.isComposing) return
  // Ctrl+Enter 翻译；Esc 关闭
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
  } else if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
    e.preventDefault()
    doTranslate()
  }
}

function clearAll() {
  input.value = ''
  output.value = ''
  error.value = ''
  elapsedMs.value = 0
}

const charCount = (s: string) => s.length
</script>

<template>
  <Transition name="slide">
    <div v-if="props.show" class="tr-overlay" @click.self="emit('close')" @keydown="onKeydown">
      <div class="tr-panel" role="region" aria-label="翻译面板">
        <!-- 头部 -->
        <div class="tr-header">
          <span class="tr-title">🌐 翻译</span>
          <span class="tr-meta">{{ engineLabel }} 翻译 · →{{ targetLabel }}</span>
          <button class="tr-close" title="关闭 (Esc)" aria-label="关闭" @click="emit('close')">✕</button>
        </div>

        <!-- 输入区 -->
        <div class="tr-section">
          <div class="tr-section-title">
            <span>原文</span>
            <span class="tr-count">{{ charCount(input) }} 字符</span>
          </div>
          <textarea
            v-model="input"
            class="tr-textarea"
            placeholder="粘贴或输入要翻译的文本（Ctrl+Enter 翻译）"
            rows="8"
          />
        </div>

        <!-- 操作按钮 -->
        <div class="tr-actions">
          <button
            class="tr-btn tr-btn-primary"
            :disabled="loading || !input.trim()"
            @click="doTranslate"
          >
            {{ loading ? '⏳ 翻译中…' : '🔄 翻译' }}
          </button>
          <button class="tr-btn" :disabled="loading" @click="clearAll">清空</button>
          <button
            class="tr-btn"
            :disabled="loading"
            title="翻译当前标签的整个页面（把网页文字换成中文）"
            @click="translatePage"
          >
            📄 翻译当前页面
          </button>
          <span class="tr-elapsed" v-if="elapsedMs > 0">{{ elapsedMs }} ms</span>
        </div>

        <!-- 结果区 -->
        <div class="tr-section">
          <div class="tr-section-title">
            <span>译文</span>
            <button
              v-if="output"
              class="tr-copy"
              :class="{ ok: copied }"
              @click="copyOutput"
            >
              {{ copied ? '✅ 已复制' : '📋 复制' }}
            </button>
          </div>
          <div class="tr-output">
            <div v-if="error" class="tr-error">{{ error }}</div>
            <div v-else-if="output" class="tr-result">{{ output }}</div>
            <div v-else-if="!loading" class="tr-empty">译文会显示在这里</div>
          </div>
        </div>

        <div class="tr-footer">
          {{ TRANSLATE_PANEL_HINT }}
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.tr-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.4);
  z-index: var(--z-sidepanel);
  display: flex;
  justify-content: flex-end;
}
.tr-panel {
  width: var(--dialog-md);
  max-width: 90vw;
  height: 100%;
  background: var(--bg-elevated);
  color: var(--text-primary);
  box-shadow: -4px 0 20px rgba(0, 0, 0, 0.5);
  display: flex;
  flex-direction: column;
  font-size: var(--text-base);
  line-height: var(--leading-base);
  font-family: var(--font-sans);
}
.tr-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-subtle);
  background: var(--bg-elevated-2);
}
.tr-title {
  font-size: var(--text-md);
  font-weight: 600;
}
.tr-meta {
  font-size: var(--text-xs);
  color: var(--text-muted);
  flex: 1;
}
.tr-close {
  background: transparent;
  border: 0;
  color: var(--text-muted);
  font-size: 18px;
  cursor: pointer;
  padding: 0 4px;
}
.tr-close:hover { color: var(--text-primary); }

.tr-section {
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-bottom: 1px solid var(--hairline);
}
.tr-section:last-of-type { border-bottom: 0; }
.tr-section-title {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: var(--text-xs);
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.tr-count { font-weight: normal; }

.tr-textarea {
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-2xs);
  color: var(--text-primary);
  padding: 10px;
  font-size: var(--text-base);
  line-height: var(--leading-base);
  font-family: var(--font-mono);
  resize: vertical;
  min-height: 100px;
}
.tr-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}

.tr-actions {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 8px 16px;
  background: var(--bg-elevated-2);
  border-bottom: 1px solid var(--hairline);
}
.tr-btn {
  background: var(--bg-hover);
  color: var(--text-primary);
  border: 0;
  padding: 6px 14px;
  border-radius: var(--radius-2xs);
  cursor: pointer;
  font-size: var(--text-base);
}
.tr-btn:hover:not(:disabled) { background: var(--bg-active); }
.tr-btn:disabled { opacity: 0.5; cursor: default; }
.tr-btn-primary {
  background: var(--accent);
  color: var(--accent-contrast);
}
.tr-btn-primary:hover:not(:disabled) { background: var(--accent-hover); }
.tr-elapsed {
  margin-left: auto;
  font-size: var(--text-xs);
  color: var(--text-muted);
}

.tr-output {
  background: var(--bg-elevated-2);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-2xs);
  padding: 10px;
  min-height: 140px;
  max-height: 360px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}
.tr-empty { color: var(--text-muted); font-style: italic; }
.tr-error {
  color: var(--danger);
  background: var(--danger-soft);
  padding: 8px;
  border-radius: var(--radius-2xs);
}
.tr-result { color: var(--text-secondary); }

.tr-copy {
  background: var(--bg-hover);
  color: var(--text-primary);
  border: 0;
  padding: 3px 10px;
  border-radius: var(--radius-2xs);
  cursor: pointer;
  font-size: var(--text-xs);
}
.tr-copy:hover { background: var(--bg-active); }
.tr-copy.ok { background: var(--success); color: var(--accent-contrast); }

.tr-footer {
  margin-top: auto;
  padding: 8px 16px;
  background: var(--bg-elevated-2);
  border-top: 1px solid var(--hairline);
  font-size: var(--text-xs);
  color: var(--text-muted);
  text-align: center;
}

.slide-enter-active, .slide-leave-active {
  transition: transform var(--dur-slow) var(--ease);
}
.slide-enter-from, .slide-leave-to {
  transform: translateX(100%);
}
</style>

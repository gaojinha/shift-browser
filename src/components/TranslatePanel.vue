<script setup lang="ts">
// 翻译面板：右侧滑出，粘贴文本 → 调 Rust 翻译 API → 显示译文
// 原版 Electron 翻译功能的核心场景——选不中文本复制到这里翻译
import { ref, watch, nextTick } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ close: [] }>()

const input = ref('')
const output = ref('')
const loading = ref(false)
const error = ref('')
const elapsedMs = ref(0)

/** 翻译当前标签的整个页面：复用 Rust 侧整页翻译脚本（whole_page_translate.js） */
async function translatePage() {
  error.value = ''
  try {
    await invoke('translate_current_page')
    emit('close') // 关掉面板，让用户直接看页面上的翻译进度浮层
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e || '整页翻译失败')
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
      { text, target: 'zh-CN' },
    )
    window.clearInterval(tick)
    if (r && r.ok && r.data) {
      output.value = r.data
    } else {
      error.value = (r && r.msg) || '翻译失败'
    }
  } catch (e: unknown) {
    window.clearInterval(tick)
    error.value = e instanceof Error ? e.message : String(e || '翻译失败')
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
    try { document.execCommand('copy') } catch {}
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
      <div class="tr-panel">
        <!-- 头部 -->
        <div class="tr-header">
          <span class="tr-title">🌐 翻译</span>
          <span class="tr-meta">Google 翻译 · →中文</span>
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
          提示：从网页选中文字复制 → 在此粘贴 → Ctrl+Enter 翻译
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
  z-index: 9000;
  display: flex;
  justify-content: flex-end;
}
.tr-panel {
  width: 480px;
  max-width: 90vw;
  height: 100%;
  background: #1e1e1e;
  color: #eee;
  box-shadow: -4px 0 20px rgba(0, 0, 0, 0.5);
  display: flex;
  flex-direction: column;
  font: 13px/1.5 system-ui, -apple-system, sans-serif;
}
.tr-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
  border-bottom: 1px solid #333;
  background: #252525;
}
.tr-title {
  font-size: 15px;
  font-weight: 600;
}
.tr-meta {
  font-size: 11px;
  color: #888;
  flex: 1;
}
.tr-close {
  background: transparent;
  border: 0;
  color: #aaa;
  font-size: 18px;
  cursor: pointer;
  padding: 0 4px;
}
.tr-close:hover { color: #fff; }

.tr-section {
  padding: 12px 16px;
  display: flex;
  flex-direction: column;
  gap: 6px;
  border-bottom: 1px solid #2a2a2a;
}
.tr-section:last-of-type { border-bottom: 0; }
.tr-section-title {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 11px;
  color: #888;
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.tr-count { font-weight: normal; }

.tr-textarea {
  background: #2a2a2a;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  color: #eee;
  padding: 10px;
  font: 13px/1.6 ui-monospace, monospace;
  resize: vertical;
  min-height: 100px;
}
.tr-textarea:focus {
  outline: none;
  border-color: #3a7bc8;
}

.tr-actions {
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 8px 16px;
  background: #252525;
  border-bottom: 1px solid #2a2a2a;
}
.tr-btn {
  background: #3a3a3a;
  color: #eee;
  border: 0;
  padding: 6px 14px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
}
.tr-btn:hover:not(:disabled) { background: #4a4a4a; }
.tr-btn:disabled { opacity: 0.5; cursor: default; }
.tr-btn-primary {
  background: #3a7bc8;
  color: #fff;
}
.tr-btn-primary:hover:not(:disabled) { background: #4a8cd8; }
.tr-elapsed {
  margin-left: auto;
  font-size: 11px;
  color: #888;
}

.tr-output {
  background: #252525;
  border: 1px solid #3a3a3a;
  border-radius: 4px;
  padding: 10px;
  min-height: 140px;
  max-height: 360px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
}
.tr-empty { color: #666; font-style: italic; }
.tr-error {
  color: #f88;
  background: rgba(255, 100, 100, 0.1);
  padding: 8px;
  border-radius: 4px;
}
.tr-result { color: #ddd; }

.tr-copy {
  background: #3a3a3a;
  color: #eee;
  border: 0;
  padding: 3px 10px;
  border-radius: 4px;
  cursor: pointer;
  font-size: 11px;
}
.tr-copy:hover { background: #4a4a4a; }
.tr-copy.ok { background: #2a7a4a; color: #fff; }

.tr-footer {
  margin-top: auto;
  padding: 8px 16px;
  background: #252525;
  border-top: 1px solid #2a2a2a;
  font-size: 11px;
  color: #888;
  text-align: center;
}

.slide-enter-active, .slide-leave-active {
  transition: transform 0.25s ease;
}
.slide-enter-from, .slide-leave-to {
  transform: translateX(100%);
}
</style>

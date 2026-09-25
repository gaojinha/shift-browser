<script setup lang="ts">
// 历史管理弹窗（Ctrl+H 打开）
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import type { HistoryEntry } from '../types/history'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const tabsStore = useTabsStore()
const toastStore = useToastStore()

const list = ref<HistoryEntry[]>([])
const query = ref('')
const loading = ref(false)
// 自定义确认弹窗（WebKitGTK 下 window.confirm 不弹，必须用组件内弹窗）
const showConfirm = ref(false)

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        await loadHistory()
      } else {
        query.value = ''
      }
    } catch (e) {
      console.error('[history] 加载失败:', e)
      toastStore.error('加载历史失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'history-dialog')
      }
    }
  },
)

/** 加载最近历史（带搜索词时走 search_history） */
async function loadHistory() {
  loading.value = true
  try {
    const q = query.value.trim()
    if (q) {
      list.value = await invoke<HistoryEntry[]>('search_history', { query: q, limit: 200 })
    } else {
      list.value = await invoke<HistoryEntry[]>('get_recent_history', { limit: 200 })
    }
  } finally {
    loading.value = false
  }
}

/** 搜索防抖 300ms */
let searchTimer: number | null = null
function onSearchInput() {
  if (searchTimer) clearTimeout(searchTimer)
  searchTimer = window.setTimeout(() => loadHistory(), 300)
}

/** 点击历史条目：新标签打开 */
function onOpen(entry: HistoryEntry) {
  tabsStore.createTab(entry.url)
}

/** 删除单条 */
async function onDelete(e: MouseEvent, entry: HistoryEntry) {
  e.stopPropagation()
  try {
    await invoke('delete_history_entry', { id: entry.id })
    list.value = list.value.filter((x) => x.id !== entry.id)
  } catch (e) {
    console.error('[history] 删除失败:', e)
    toastStore.error('删除历史失败')
  }
}

/** 清空全部（二次确认，自定义弹窗） */
function onClearAll() {
  showConfirm.value = true
}

async function doClearAll() {
  showConfirm.value = false
  try {
    await invoke('clear_history')
    list.value = []
    toastStore.success('历史已清空')
  } catch (e) {
    console.error('[history] 清空失败:', e)
    toastStore.error('清空历史失败')
  }
}

/** 格式化时间（YYYY-MM-DD HH:mm） */
function formatTime(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** 提取显示用域名 */
function hostOf(url: string): string {
  try {
    return new URL(url).host
  } catch {
    return url
  }
}
</script>

<template>
  <Transition name="fade">
    <div v-if="show" class="dialog-overlay" @click.self="emit('close')">
      <div class="dialog">
        <div class="dialog-header">
          <span>浏览历史</span>
          <button class="close-btn" aria-label="关闭" @click="emit('close')">×</button>
        </div>
        <div class="dialog-body history-body">
          <div class="history-toolbar">
            <input
              v-model="query"
              type="text"
              class="history-search"
              placeholder="搜索历史记录..."
              spellcheck="false"
              @input="onSearchInput"
            />
            <button class="clear-btn" :disabled="list.length === 0" @click="onClearAll">
              清空
            </button>
          </div>
          <div v-if="loading" class="empty loading-hint">加载中…</div>
          <div v-else-if="list.length === 0" class="empty">
            {{ query ? '没有匹配的历史记录' : '还没有浏览历史' }}
          </div>
          <div v-else class="history-list">
            <div v-for="entry in list" :key="entry.id" class="history-item" @click="onOpen(entry)">
              <div class="history-main">
                <div class="history-title">{{ entry.title || hostOf(entry.url) }}</div>
                <div class="history-url">{{ entry.url }}</div>
              </div>
              <div class="history-meta">
                <span class="history-count">{{ entry.visit_count }} 次</span>
                <span class="history-time">{{ formatTime(entry.last_visit_at) }}</span>
              </div>
              <button class="history-del" aria-label="删除" @click="onDelete($event, entry)">×</button>
            </div>
          </div>
        </div>
        <!-- 自定义确认弹窗（覆盖在 dialog 之上） -->
        <div v-if="showConfirm" class="confirm-overlay">
          <div class="confirm-box">
            <div class="confirm-text">确定清空全部浏览历史？此操作不可恢复。</div>
            <div class="confirm-actions">
              <button class="confirm-btn cancel" @click="showConfirm = false">取消</button>
              <button class="confirm-btn ok" @click="doClearAll">清空</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.history-body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 260px;
}
.history-toolbar {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.history-search {
  flex: 1;
  height: 30px;
  padding: 0 10px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  outline: none;
}
.history-search:focus {
  border-color: var(--accent);
}
.clear-btn {
  height: 30px;
  padding: 0 12px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  flex-shrink: 0;
}
.clear-btn:hover:not(:disabled) {
  color: var(--danger);
  border-color: var(--danger);
}
.clear-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.history-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
  max-height: 50vh;
}
.history-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s;
}
.history-item:hover {
  background: var(--bg-hover);
}
.history-main {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}
.history-title {
  font-size: 13px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.history-url {
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.history-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  flex-shrink: 0;
}
.history-count {
  font-size: 11px;
  color: var(--text-muted);
}
.history-time {
  font-size: 11px;
  color: var(--text-muted);
}
.history-del {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 14px;
  cursor: pointer;
  padding: 2px 6px;
  border-radius: 4px;
  flex-shrink: 0;
  visibility: hidden;
}
.history-item:hover .history-del {
  visibility: visible;
}
.history-del:hover {
  color: var(--danger);
  background: var(--bg-active);
}
.confirm-overlay {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  background: var(--scrim);
  border-radius: var(--radius-md);
  z-index: 10;
}
.confirm-box {
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  padding: 18px 20px;
  min-width: 260px;
  box-shadow: var(--shadow-pop);
}
.confirm-text {
  font-size: 13px;
  color: var(--text-primary);
  margin-bottom: 14px;
  line-height: 1.6;
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.confirm-btn {
  height: 30px;
  padding: 0 14px;
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  border: 1px solid var(--border-color);
}
.confirm-btn.cancel {
  background: var(--bg-hover);
  color: var(--text-secondary);
}
.confirm-btn.ok {
  background: var(--danger);
  border-color: var(--danger);
  color: #fff;
}
.confirm-btn:hover {
  filter: brightness(1.1);
}
</style>

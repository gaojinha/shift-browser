<script setup lang="ts">
// 历史管理弹窗（Ctrl+H 打开）
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import UiDialog from './ui/UiDialog.vue'
import type { HistoryEntry } from '../types/history'
import { groupHistoryByDate } from '../utils/history'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

/** 外层显隐（UiDialog 双向绑定：关即 emit close） */
const showModel = computed({
  get: () => props.show,
  set: (v: boolean) => {
    if (!v) emit('close')
  },
})

const tabsStore = useTabsStore()
const toastStore = useToastStore()

const list = ref<HistoryEntry[]>([])
/** 日期分组（C-4）：今天/昨天/更早，纯前端从 list 派生，不新增后端调用 */
const groups = computed(() => groupHistoryByDate(list.value))
/** 分组折叠状态（默认全展开，点组头折叠/展开） */
const collapsed = ref<Record<string, boolean>>({})
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

/** 分组是否折叠（C-4） */
function isCollapsed(key: string): boolean {
  return collapsed.value[key] === true
}
/** 折叠/展开分组（C-4） */
function toggleGroup(key: string) {
  collapsed.value[key] = !isCollapsed(key)
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
  <!-- 浏览历史：外层 UiDialog；清空确认走内层 UiDialog（footer 放按钮） -->
  <UiDialog v-model:open="showModel" title="浏览历史" width="min(520px, calc(100vw - 32px))">
    <div class="history-body">
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
            <div v-for="group in groups" :key="group.key" class="history-group">
              <button
                class="history-group-header"
                :aria-expanded="!isCollapsed(group.key)"
                @click="toggleGroup(group.key)"
              >
                <span class="history-group-caret" :class="{ collapsed: isCollapsed(group.key) }">▾</span>
                <span class="history-group-label">{{ group.label }}</span>
                <span class="history-group-count">{{ group.entries.length }}</span>
              </button>
              <div v-show="!isCollapsed(group.key)" class="history-group-body">
                <div
                  v-for="entry in group.entries"
                  :key="entry.id"
                  class="history-item"
                  @click="onOpen(entry)"
                >
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
          </div>
        </div>
        <!-- 自定义确认弹窗（内层 UiDialog，顶层展示，z 层由 Reka 接管） -->
        <UiDialog
          v-model:open="showConfirm"
          title="清空历史"
          description="确定清空全部浏览历史？此操作不可恢复。"
        >
          <template #footer>
            <button class="confirm-btn cancel" @click="showConfirm = false">取消</button>
            <button class="confirm-btn ok" @click="doClearAll">清空</button>
          </template>
        </UiDialog>
  </UiDialog>
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
  border-radius: var(--radius-xs);
  font-size: 13px;
  outline: none;
}
.history-search:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}
.clear-btn {
  height: 30px;
  padding: 0 12px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-xs);
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
.history-group {
  display: flex;
  flex-direction: column;
}
.history-group-header {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  margin-top: 4px;
  padding: 6px 10px;
  border: none;
  background: var(--bg-hover);
  color: var(--text-secondary);
  border-radius: var(--radius-xs);
  font-size: 12px;
  font-weight: 600;
  cursor: pointer;
  text-align: left;
}
.history-group-header:hover {
  background: var(--bg-active);
}
.history-group-header:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.history-group-caret {
  display: inline-block;
  font-size: 10px;
  transition: transform 0.15s;
}
.history-group-caret.collapsed {
  transform: rotate(-90deg);
}
.history-group-label {
  flex: 1;
}
.history-group-count {
  font-weight: 400;
  color: var(--text-muted);
}
.history-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: var(--radius-xs);
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
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留历史业务样式 */
/* 通用 .confirm-btn 已收口至 styles/components.css */
</style>

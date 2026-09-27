<script setup lang="ts">
// 命令栏（Ctrl+K 万能入口）——阶段 3.1 交互形态
// 一个输入框统一入口：执行浏览器命令 / 切换标签 / 搜索并打开历史 / 直接打开网址 / 问 AI。
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，打开时必须隐藏所有标签 webview，否则被页面盖住。
import { computed, nextTick, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import type { HistoryEntry } from '../types/history'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'command', action: string): void
  (e: 'ask-ai', query: string): void
}>()

const tabsStore = useTabsStore()

type Kind = 'command' | 'tab' | 'history' | 'url' | 'ai'
interface Item {
  key: string
  kind: Kind
  icon: string
  title: string
  subtitle?: string
  /** command=动作名；tab=标签 id；history/url=网址；ai=提问文本 */
  value: string
}

const query = ref('')
const activeIndex = ref(0)
const historyEntries = ref<HistoryEntry[]>([])
const inputRef = ref<HTMLInputElement | null>(null)
const listRef = ref<HTMLElement | null>(null)

/** 内置命令（kind=command，value 交给 App 派发） */
const COMMANDS: Item[] = [
  { key: 'cmd:new-tab', kind: 'command', icon: '＋', title: '新建标签页', value: 'new-tab' },
  { key: 'cmd:close-tab', kind: 'command', icon: '✕', title: '关闭当前标签页', value: 'close-tab' },
  { key: 'cmd:reload', kind: 'command', icon: '↻', title: '刷新当前页面', value: 'reload' },
  { key: 'cmd:focus-address', kind: 'command', icon: '⌖', title: '聚焦地址栏', value: 'focus-address' },
  { key: 'cmd:ai', kind: 'command', icon: '✦', title: '打开 AI 助手', value: 'ai' },
  { key: 'cmd:history', kind: 'command', icon: '🕘', title: '打开历史记录', value: 'history' },
  { key: 'cmd:bookmarks', kind: 'command', icon: '🔖', title: '打开书签管理', value: 'bookmarks' },
  { key: 'cmd:downloads', kind: 'command', icon: '⤓', title: '打开下载管理', value: 'downloads' },
  { key: 'cmd:passwords', kind: 'command', icon: '🔑', title: '打开密码管理', value: 'passwords' },
  { key: 'cmd:sessions', kind: 'command', icon: '🗂', title: '打开会话管理', value: 'sessions' },
  { key: 'cmd:userscripts', kind: 'command', icon: '📜', title: '打开用户脚本', value: 'userscripts' },
  { key: 'cmd:translate', kind: 'command', icon: '🌐', title: '翻译当前页面', value: 'translate' },
  { key: 'cmd:settings', kind: 'command', icon: '⚙', title: '打开设置', value: 'settings' },
  { key: 'cmd:close-same-domain', kind: 'command', icon: '🧹', title: '关闭同域名标签', value: 'close-same-domain' },
  { key: 'cmd:close-duplicates', kind: 'command', icon: '🧹', title: '关闭重复标签', value: 'close-duplicates' },
  { key: 'cmd:mute-all', kind: 'command', icon: '🔇', title: '全部静音/取消', value: 'mute-all' },
  { key: 'cmd:devtools', kind: 'command', icon: '🛠', title: '开发者工具', value: 'devtools' },
]

/** 输入是否像网址（含协议，或像域名） */
function isUrlLike(s: string): boolean {
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) return true
  return !/\s/.test(s) && /^[^\s/]+\.[^\s/]{2,}/.test(s)
}
function normalizeUrl(s: string): string {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(s) ? s : 'https://' + s
}

const items = computed<Item[]>(() => {
  const raw = query.value.trim()
  const q = raw.toLowerCase()
  const out: Item[] = []

  // ① 问 AI（有输入即置顶候选）
  if (raw) {
    out.push({
      key: 'ai:' + raw,
      kind: 'ai',
      icon: '✦',
      title: `问 AI：${raw}`,
      value: raw,
    })
  }

  // ② 直接打开网址（输入像 URL/域名）
  if (raw && isUrlLike(raw)) {
    out.push({
      key: 'url:' + raw,
      kind: 'url',
      icon: '↗',
      title: `打开 ${raw}`,
      value: normalizeUrl(raw),
    })
  }

  // ③ 标签页（切换）
  for (const t of tabsStore.tabs) {
    const title = t.title || t.url || '新标签页'
    if (!q || title.toLowerCase().includes(q) || t.url.toLowerCase().includes(q)) {
      out.push({ key: 'tab:' + t.id, kind: 'tab', icon: '🗔', title, subtitle: t.url, value: t.id })
    }
  }

  // ④ 历史（打开；无输入时只给最近 6 条，避免刷屏）
  const hist = historyEntries.value.filter((h) => {
    if (!q) return true
    const title = (h.title || h.url).toLowerCase()
    return title.includes(q) || h.url.toLowerCase().includes(q)
  })
  for (const h of hist.slice(0, q ? 20 : 6)) {
    out.push({
      key: 'his:' + h.id,
      kind: 'history',
      icon: '🕘',
      title: h.title || h.url,
      subtitle: h.url,
      value: h.url,
    })
  }

  // ⑤ 内置命令
  for (const c of COMMANDS) {
    if (!q || c.title.toLowerCase().includes(q)) out.push(c)
  }

  return out.slice(0, 50)
})

function scrollActiveIntoView() {
  nextTick(() => {
    const el = listRef.value?.querySelector<HTMLElement>('.cp-item.active')
    el?.scrollIntoView({ block: 'nearest' })
  })
}

function onInput() {
  activeIndex.value = 0
}

function move(delta: number) {
  const n = items.value.length
  if (n === 0) return
  activeIndex.value = (activeIndex.value + delta + n) % n
  scrollActiveIntoView()
}

function execute(item?: Item) {
  const it = item ?? items.value[activeIndex.value]
  if (!it) {
    emit('close')
    return
  }
  if (it.kind === 'command') emit('command', it.value)
  else if (it.kind === 'tab') tabsStore.switchTab(it.value)
  else if (it.kind === 'history' || it.kind === 'url') tabsStore.createTab(it.value)
  else if (it.kind === 'ai') emit('ask-ai', it.value)
  emit('close')
}

function onKeydown(e: KeyboardEvent) {
  // 组字中：方向键/Enter/Esc 交给输入法候选框，不抢跑
  if (e.isComposing) return
  if (e.key === 'ArrowDown') {
    e.preventDefault()
    e.stopPropagation()
    move(1)
  } else if (e.key === 'ArrowUp') {
    e.preventDefault()
    e.stopPropagation()
    move(-1)
  } else if (e.key === 'Enter') {
    e.preventDefault()
    e.stopPropagation()
    execute()
  } else if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    emit('close')
  }
}

async function loadHistory() {
  try {
    historyEntries.value = await invoke<HistoryEntry[]>('get_recent_history', { limit: 40 })
  } catch {
    historyEntries.value = []
  }
}

// 打开：隐藏 webview（否则被页面盖住）+ 载入历史 + 聚焦输入框；关闭：恢复 webview
watch(
  () => props.show,
  async (v) => {
    if (v) {
      query.value = ''
      activeIndex.value = 0
      historyEntries.value = []
      await setWebviewVisible(false, 'command-palette')
      loadHistory()
      nextTick(() => inputRef.value?.focus())
    } else {
      await setWebviewVisible(true, 'command-palette')
    }
  },
)
</script>

<template>
  <div v-if="show" class="cp-overlay" @mousedown.self="emit('close')">
    <div class="cp-panel">
      <div class="cp-input-row">
        <span class="cp-search">⌕</span>
        <input
          ref="inputRef"
          v-model="query"
          class="cp-input"
          placeholder="输入命令、网址，或问 AI…"
          spellcheck="false"
          @input="onInput"
          @keydown="onKeydown"
        />
        <span class="cp-esc">Esc</span>
      </div>
      <div ref="listRef" class="cp-list">
        <div v-if="items.length === 0" class="cp-empty">没有匹配项</div>
        <div
          v-for="(it, i) in items"
          :key="it.key"
          class="cp-item"
          :class="{ active: i === activeIndex }"
          @mousemove="activeIndex = i"
          @click="execute(it)"
        >
          <span class="cp-icon">{{ it.icon }}</span>
          <span class="cp-text">
            <span class="cp-title">{{ it.title }}</span>
            <span v-if="it.subtitle" class="cp-sub">{{ it.subtitle }}</span>
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.cp-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  background: var(--scrim);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding-top: 12vh;
}
.cp-panel {
  width: min(640px, 92vw);
  max-height: 60vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dialog);
  overflow: hidden;
  animation: cp-pop var(--dur) var(--ease-out);
}
@keyframes cp-pop {
  from {
    opacity: 0;
    transform: translateY(-8px) scale(0.985);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
.cp-input-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 13px 16px;
  border-bottom: 1px solid var(--hairline);
}
.cp-search {
  font-size: 16px;
  color: var(--accent);
}
.cp-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 15px;
}
.cp-input::placeholder {
  color: var(--text-muted);
}
.cp-esc {
  font-size: var(--text-xs);
  color: var(--text-muted);
  background: var(--bg-elevated-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-xs);
  padding: 2px 7px;
  line-height: 1.4;
}
.cp-list {
  overflow-y: auto;
  padding: 6px;
}
.cp-empty {
  padding: 24px;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-base);
}
.cp-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
.cp-item.active {
  background: var(--accent-soft);
}
.cp-icon {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: var(--text-secondary);
  background: var(--bg-elevated-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-xs);
}
.cp-item.active .cp-icon {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.cp-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.cp-title {
  color: var(--text-primary);
  font-size: var(--text-base);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cp-item.active .cp-title {
  font-weight: 600;
}
.cp-sub {
  color: var(--text-muted);
  font-size: var(--text-xs);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>

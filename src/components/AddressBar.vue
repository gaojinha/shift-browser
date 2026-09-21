<script setup lang="ts">
// 地址栏（对应 Electron 版 .address-bar 设计）
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'
import type { Bookmark } from '../types/bookmark'
import type { HistoryEntry } from '../types/history'
import TranslateProgress from './TranslateProgress.vue'
import SpaceSwitcher from './SpaceSwitcher.vue'

const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()
const addressInput = ref('')
const inputEl = ref<HTMLInputElement | null>(null)
/** 聚焦地址栏并全选（Ctrl+L 快捷键用） */
function focusInput() {
  inputEl.value?.focus()
  inputEl.value?.select()
}
defineExpose({ focusInput })
const isLoading = computed(
  () => !!tabsStore.loadingTabId && tabsStore.loadingTabId === tabsStore.activeTabId,
)
const isHttps = computed(() => /^https:\/\//i.test(addressInput.value))

const emit = defineEmits<{
  (e: 'openAi'): void
  (e: 'openTranslate'): void
  (e: 'openPasswords'): void
  (e: 'openGroups'): void
  (e: 'openUserscripts'): void
  (e: 'openDownloads'): void
  (e: 'openSettings'): void
}>()

// 搜索引擎 URL 模板（与设置弹窗 SEARCH_ENGINES 一致）
const SEARCH_URLS: Record<string, string> = {
  bing: 'https://www.bing.com/search?q=',
  baidu: 'https://www.baidu.com/s?wd=',
  google: 'https://www.google.com/search?q=',
}

// 当前活动标签变化时同步地址栏（空 URL 也清空，避免残留上一个标签的地址）
watch(
  () => tabsStore.activeTab?.url,
  (url) => {
    addressInput.value = url ?? ''
  },
  { immediate: true },
)

// ===== 搜索建议下拉（历史 + 书签） =====
interface Suggestion {
  type: 'bookmark' | 'history'
  title: string
  url: string
  id: number
  visitCount?: number
}
const suggestions = ref<Suggestion[]>([])
const selectedIndex = ref(-1)
const showSuggestions = ref(false)

/** 输入防抖拉建议：书签优先（精确匹配度高），再补历史 */
let suggestTimer: number | null = null
function onInput() {
  if (suggestTimer) clearTimeout(suggestTimer)
  suggestTimer = window.setTimeout(async () => {
    const q = addressInput.value.trim()
    if (!q) {
      suggestions.value = []
      showSuggestions.value = false
      return
    }
    try {
      const [bks, his] = await Promise.all([
        invoke<Bookmark[]>('search_bookmarks', { query: q }).catch(() => [] as Bookmark[]),
        invoke<HistoryEntry[]>('search_history', { query: q, limit: 20 }).catch(
          () => [] as HistoryEntry[],
        ),
      ])
      const merged: Suggestion[] = [
        ...bks.slice(0, 5).map((b) => ({ type: 'bookmark' as const, title: b.title, url: b.url, id: b.id })),
        ...his.slice(0, 8).map((h) => ({
          type: 'history' as const,
          title: h.title || h.url,
          url: h.url,
          id: h.id,
          visitCount: h.visit_count,
        })),
      ]
      suggestions.value = merged
      selectedIndex.value = -1
      showSuggestions.value = merged.length > 0
    } catch (e) {
      console.error('[address] 拉取建议失败:', e)
    }
  }, 200)
}

/** 打开建议项（当前标签导航） */
function openSuggestion(s: Suggestion) {
  const active = tabsStore.activeTab
  if (!active) return
  tabsStore.navigateTab(active.id, s.url)
  closeSuggestions()
}

function closeSuggestions() {
  showSuggestions.value = false
  suggestions.value = []
  selectedIndex.value = -1
}

/** 键盘导航：↓ 上移 ↑ 下移 Enter 打开选中 Esc 关闭 */
function onKeydown(e: KeyboardEvent) {
  if (showSuggestions.value && suggestions.value.length > 0) {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      selectedIndex.value = (selectedIndex.value + 1) % suggestions.value.length
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      selectedIndex.value =
        (selectedIndex.value - 1 + suggestions.value.length) % suggestions.value.length
      return
    }
    if (e.key === 'Escape') {
      closeSuggestions()
      return
    }
    if (e.key === 'Enter' && selectedIndex.value >= 0) {
      e.preventDefault()
      openSuggestion(suggestions.value[selectedIndex.value])
      return
    }
  }
  // 无选中建议时 Enter 走原逻辑
  if (e.key === 'Enter') onAddressEnter()
}

/** 失焦关闭（延迟让点击建议生效） */
function onBlur() {
  window.setTimeout(closeSuggestions, 150)
}

/** 判断输入是否像 URL（域名/含点/localhost/IP/host:port），而不是搜索词 */
function looksLikeUrl(input: string): boolean {
  return (
    input.includes('.') ||
    /^localhost(:\d+)?$/i.test(input) ||
    /^\d{1,3}(\.\d{1,3}){3}(:\d+)?$/.test(input) ||
    /^[a-zA-Z0-9-]+:\d+$/.test(input)
  )
}

/** 地址栏回车：输入 URL 或搜索 */
function onAddressEnter() {
  const input = addressInput.value.trim()
  if (!input) return
  const active = tabsStore.activeTab
  if (!active) return

  let url = input
  // 无协议时补全 https://
  if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(input)) {
    // 带空格或不像 URL → 走搜索（用设置的搜索引擎）
    if (/\s/.test(input) || !looksLikeUrl(input)) {
      const engine = settingsStore.get('search_engine') || 'bing'
      const base = SEARCH_URLS[engine] ?? SEARCH_URLS.bing
      url = `${base}${encodeURIComponent(input)}`
    } else {
      url = `https://${input}`
    }
  }
  tabsStore.navigateTab(active.id, url)
}

function onRefresh() {
  const active = tabsStore.activeTab
  if (active) tabsStore.navigateTab(active.id, active.url)
}

function onHome() {
  const homepage = settingsStore.get('homepage') || 'https://www.bing.com'
  // 默认 homepage='newtab'（本地新标签页关键字），navigateTab 会把它当 URL 解析必失败；
  // 本地新标签页必须走 createTab。统一判断：'newtab' → createTab，其余 → navigateTab
  if (homepage === 'newtab') {
    // 修复（2026-09-19）：原实现 activeTab 为 null（所有标签已关闭）时直接 return，
    // 导致主页按钮没反应；主页语义就是"打开主页"，无标签时新建标签打开
    tabsStore.createTab()
    return
  }
  const active = tabsStore.activeTab
  if (!active) {
    tabsStore.createTab(homepage)
    return
  }
  tabsStore.navigateTab(active.id, homepage)
}

function onBack() {
  tabsStore.goBack()
}

function onForward() {
  tabsStore.goForward()
}

/** 阅读模式：提取当前页正文，新标签打开干净版 */
const readerLoading = ref(false)
const readerTip = ref('')
async function onReader() {
  const active = tabsStore.activeTab
  if (!active || readerLoading.value) return
  if (!/^https?:\/\//.test(active.url)) {
    readerTip.value = '当前页不支持'
    setTimeout(() => (readerTip.value = ''), 2000)
    return
  }
  readerLoading.value = true
  try {
    const url = await invoke<string>('enter_reader_mode', { url: active.url })
    await tabsStore.createTab(url)
  } catch (e) {
    readerTip.value = String(e).replace(/^[^:]*:\s*/, '') || '提取失败'
    setTimeout(() => (readerTip.value = ''), 4000)
  } finally {
    readerLoading.value = false
  }
}
</script>

<template>
  <div class="address-bar">
    <SpaceSwitcher />
    <button class="nav-btn" title="后退" :disabled="!tabsStore.canGoBack" @click="onBack">←</button>
    <button class="nav-btn" title="前进" :disabled="!tabsStore.canGoForward" @click="onForward">
      →
    </button>
    <button
      class="nav-btn"
      :class="{ spinning: isLoading }"
      title="刷新"
      :disabled="isLoading"
      @click="onRefresh"
    >
      ↻
    </button>
    <button class="nav-btn" title="主页" @click="onHome">⌂</button>
    <div class="address-container">
      <span v-if="isHttps" class="lock-icon" title="HTTPS 安全连接">🔒</span>
      <input
        ref="inputEl"
        v-model="addressInput"
        type="text"
        placeholder="输入网址或搜索..."
        spellcheck="false"
        @input="onInput"
        @keydown="onKeydown"
        @blur="onBlur"
      />
      <div v-if="isLoading" class="load-bar"></div>
      <div v-if="showSuggestions" class="suggest-dropdown">
        <div
          v-for="(s, i) in suggestions"
          :key="s.type + s.id"
          class="suggest-item"
          :class="{ selected: i === selectedIndex }"
          @mousedown.prevent="openSuggestion(s)"
        >
          <span class="suggest-icon">{{ s.type === 'bookmark' ? '🔖' : '🕘' }}</span>
          <span class="suggest-title">{{ s.title }}</span>
          <span class="suggest-url">{{ s.url }}</span>
        </div>
      </div>
    </div>
    <button
      class="action-btn"
      :disabled="readerLoading"
      :title="readerTip || '阅读模式（提取正文大字看）'"
      @click="onReader"
    >
      {{ readerLoading ? '⏳' : '📖' }}
    </button>
    <span v-if="readerTip" class="reader-tip" :title="readerTip">{{ readerTip }}</span>
    <TranslateProgress />
    <button class="action-btn translate-btn" title="翻译（粘贴文本→中文）" @click="emit('openTranslate')">🌐</button>
    <button class="action-btn ai-btn" title="AI 助手" @click="emit('openAi')">AI</button>
    <button class="action-btn" title="密码管理" @click="emit('openPasswords')">🔑</button>
    <button class="action-btn" title="标签分组" @click="emit('openGroups')">🗂</button>
    <button class="action-btn" title="用户脚本" @click="emit('openUserscripts')">📜</button>
    <button class="action-btn" title="下载管理" @click="emit('openDownloads')">⬇</button>
    <button class="action-btn" title="设置" @click="emit('openSettings')">⚙</button>
  </div>
</template>

<style scoped>
.address-bar {
  display: flex;
  align-items: center;
  height: 42px;
  background: var(--bg-primary);
  padding: 0 8px 4px;
  gap: 4px;
  flex-shrink: 0;
  user-select: none;
  position: relative;
}
.nav-btn,
.action-btn {
  width: 30px;
  height: 30px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: 6px;
  cursor: pointer;
  font-size: 15px;
  flex-shrink: 0;
  transition:
    background 0.15s,
    color 0.15s;
}
.action-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}
.reader-tip {
  position: absolute;
  right: 8px;
  top: 42px;
  font-size: 11px;
  color: #ff9b9b;
  max-width: 220px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 4px;
  padding: 2px 8px;
  z-index: 20;
  pointer-events: none;
}
.btn-icon {
  pointer-events: none;
  display: inline-block;
}
.ai-btn {
  color: var(--accent);
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.5px;
}
.translate-btn {
  font-size: 15px;
}
.nav-btn:hover:not(:disabled),
.action-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}
.nav-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.address-container {
  flex: 1;
  /* 限制最大宽度，给右侧按钮腾空间（按钮多时不被挤压隐藏） */
  max-width: 640px;
  display: flex;
  align-items: center;
  background: var(--bg-secondary);
  border-radius: 16px;
  padding: 0 14px;
  height: 30px;
  position: relative;
}
.address-container:focus-within {
  outline: 1.5px solid var(--accent);
  outline-offset: 0;
}
.lock-icon {
  font-size: 11px;
  margin-right: 6px;
  flex-shrink: 0;
  opacity: 0.7;
}
.load-bar {
  position: absolute;
  left: 8px;
  right: 8px;
  bottom: 2px;
  height: 2px;
  border-radius: 1px;
  background: var(--accent);
  animation: load-slide 1.2s ease-in-out infinite;
  transform-origin: left;
}
@keyframes load-slide {
  0% {
    transform: scaleX(0.15);
    opacity: 0.6;
  }
  50% {
    transform: scaleX(1);
    opacity: 1;
  }
  100% {
    transform: scaleX(0.15);
    opacity: 0.6;
  }
}
.spinning {
  animation: nav-spin 0.8s linear infinite;
}
@keyframes nav-spin {
  to {
    transform: rotate(360deg);
  }
}
.address-container input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  height: 100%;
}
.address-container input::placeholder {
  color: var(--text-muted);
}
.suggest-dropdown {
  position: absolute;
  left: 0;
  right: 0;
  top: 34px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.4);
  z-index: 100;
  overflow: hidden;
  max-height: 320px;
  overflow-y: auto;
}
.suggest-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  transition: background 0.1s;
}
.suggest-item.selected {
  background: var(--bg-active);
}
.suggest-item:hover {
  background: var(--bg-hover);
}
.suggest-icon {
  flex-shrink: 0;
  font-size: 12px;
}
.suggest-title {
  flex-shrink: 0;
  max-width: 180px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.suggest-url {
  flex: 1;
  min-width: 0;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--text-muted);
  font-size: 11px;
  text-align: right;
}
</style>

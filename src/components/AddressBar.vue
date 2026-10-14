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
import AppIcon from './AppIcon.vue'
// 认址纯函数（URL 识别 / 默认协议 / 搜索分流，单测见 utils/address.test.ts）
import { resolveInputTarget } from '../utils/address'
import { setWebviewVisible } from '../utils/webview'

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

/** 鼠标点进地址栏：Chrome式全选旧地址，直接敲新地址盖掉；已聚焦时再点不触发，不抢光标 */
function onInputFocus() {
  inputEl.value?.select()
}
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
  (e: 'openHistory'): void
  (e: 'openDownloads'): void
  (e: 'openSettings'): void
  (e: 'openCerts'): void
}>()

// 搜索引擎 URL 模板已收敛到 constants.ts（唯一来源）

// 当前活动标签变化时同步地址栏（空 URL 也清空，避免残留上一个标签的地址）
watch(
  () => tabsStore.activeTab?.url,
  (url) => {
    addressInput.value = url ?? ''
  },
  { immediate: true },
)

// ===== 搜索建议下拉（直达 + 历史 + 书签） =====
interface Suggestion {
  type: 'direct' | 'bookmark' | 'history'
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
        // 首条直达：回车/点击直接打开当前输入（URL 或搜索），不用先选
        {
          type: 'direct' as const,
          title: directTitle(q),
          url: directTarget(q),
          id: -1,
        },
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

// 联想下拉是 HTML 层，会伸进网页区：显示时隐藏 GTK 网页，关闭时恢复（参与全局计数）
watch(showSuggestions, (v) => {
  setWebviewVisible(!v, 'address-suggest').catch(() => {})
})

/** 键盘导航：↓ 上移 ↑ 下移 Enter 打开选中 Esc 关闭 */
function onKeydown(e: KeyboardEvent) {
  // 组字中（拼音候选态）：方向键翻候选、Enter 选词、Esc 退组字，全交给输入法
  if (e.isComposing) return
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

/** 直达项目标：与回车逻辑同一份（内网 http / 公网 https / 搜索分流） */
function directTarget(q: string): string {
  const engine = settingsStore.get('search_engine') || 'bing'
  return resolveInputTarget(q, engine).url
}

/** 直达项标题：超长截断，搜索与打开一眼区分 */
function directTitle(q: string): string {
  const engine = settingsStore.get('search_engine') || 'bing'
  const { isSearch } = resolveInputTarget(q, engine)
  const short = q.length > 40 ? q.slice(0, 40) + '…' : q
  return `${isSearch ? '搜索' : '打开'}：${short}`
}

/** 地址栏回车：输入 URL 或搜索 */
function onAddressEnter() {
  const input = addressInput.value.trim()
  if (!input) return
  const active = tabsStore.activeTab
  if (!active) return

  // 先关下拉（下拉开着时网页层是隐藏的，不关的话导航了也看不见，跟没打开一样）
  closeSuggestions()
  const engine = settingsStore.get('search_engine') || 'bing'
  const { url } = resolveInputTarget(input, engine)
  if (!url) return
  tabsStore.navigateTab(active.id, url)
}

function onRefresh() {
  // 修复（2026-09-22）：原用 navigateTab（重新导航）代替刷新——会丢 POST 数据/滚动位置，
  // 且内部页（shift://）可能失败导致"刷新没反应"。改用真正的 reload_tab。
  const active = tabsStore.activeTab
  if (active) tabsStore.reloadTab(active.id)
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
    <button class="nav-btn" data-nav="back" title="后退" :disabled="!tabsStore.canGoBack" @click="onBack">
      <AppIcon name="arrowLeft" :size="15" />
    </button>
    <button class="nav-btn" data-nav="forward" title="前进" :disabled="!tabsStore.canGoForward" @click="onForward">
      <AppIcon name="arrowRight" :size="15" />
    </button>
    <button
      class="nav-btn"
      data-nav="reload"
      :class="{ spinning: isLoading }"
      title="刷新"
      :disabled="isLoading"
      @click="onRefresh"
    >
      <AppIcon name="rotateCw" :size="15" />
    </button>
    <button class="nav-btn" data-nav="home" title="主页" @click="onHome">
      <AppIcon name="house" :size="15" />
    </button>
    <div class="address-container">
      <span v-if="isHttps" class="lock-icon" title="查看证书（点击打开证书中心）" @click="emit('openCerts')">
        <AppIcon name="lock" :size="12" />
      </span>
      <input
        ref="inputEl"
        v-model="addressInput"
        type="text"
        placeholder="输入网址或搜索..."
        aria-label="地址栏"
        spellcheck="false"
        @input="onInput"
        @keydown="onKeydown"
        @focus="onInputFocus"
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
          <span class="suggest-icon">
            <AppIcon :name="s.type === 'bookmark' ? 'folder' : s.type === 'direct' ? 'arrowRight' : 'clock'" :size="12" />
          </span>
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
      <AppIcon name="bookOpen" :size="15" :class="{ 'icon-spin': readerLoading }" />
    </button>
    <span v-if="readerTip" class="reader-tip" :title="readerTip">{{ readerTip }}</span>
    <TranslateProgress />
    <button class="action-btn translate-btn" title="翻译（粘贴文本→中文）" @click="emit('openTranslate')">
      <AppIcon name="globe" :size="15" />
    </button>
    <button class="action-btn ai-btn" title="AI 助手" @click="emit('openAi')">
      <AppIcon name="sparkles" :size="15" />
    </button>
    <button class="action-btn hide-narrow" title="密码管理" @click="emit('openPasswords')">
      <AppIcon name="key" :size="15" />
    </button>
    <button class="action-btn hide-narrow" title="标签分组" @click="emit('openGroups')">
      <AppIcon name="folder" :size="15" />
    </button>
    <button class="action-btn hide-narrow" title="用户脚本" @click="emit('openUserscripts')">
      <AppIcon name="scroll" :size="15" />
    </button>
    <button class="action-btn hide-narrow" title="历史记录（Ctrl+H）" @click="emit('openHistory')">
      <AppIcon name="clock" :size="15" />
    </button>
    <button class="action-btn" title="下载管理" @click="emit('openDownloads')">
      <AppIcon name="download" :size="15" />
    </button>
    <button class="action-btn" title="设置" @click="emit('openSettings')">
      <AppIcon name="gear" :size="15" />
    </button>
  </div>
</template>

<style scoped>
.address-bar {
  display: flex;
  align-items: center;
  height: var(--sb-address-bar-h);
  background: var(--bg-primary);
  padding: 0 8px;
  gap: 4px;
  flex-shrink: 0;
  user-select: none;
  position: relative;
}
.nav-btn,
.action-btn {
  width: 31px;
  height: 31px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 15px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease),
    box-shadow var(--dur-fast) var(--ease);
}
.action-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.reader-tip {
  position: absolute;
  right: 8px;
  top: 37px;
  font-size: var(--text-xs);
  color: var(--danger);
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-xs);
  padding: 4px 10px;
  box-shadow: var(--shadow-pop);
  z-index: var(--z-sticky);
  pointer-events: none;
}
.btn-icon {
  pointer-events: none;
  display: inline-block;
}
.ai-btn {
  color: var(--accent);
}
.ai-btn:hover {
  background: var(--accent-soft);
  color: var(--accent-hover);
}
.icon-spin {
  animation: nav-spin 0.8s linear infinite;
}
.nav-btn:hover:not(:disabled),
.action-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-btn:disabled {
  opacity: 0.35;
  cursor: default;
}
.address-container {
  flex: 1;
  /* 自动长度：占满两侧按钮之外的剩余空间——右侧图标多/宽时自动变短，少/窄时自动变长 */
  min-width: 120px;
  display: flex;
  align-items: center;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-full);
  padding: 0 14px;
  height: 31px;
  position: relative;
  transition:
    border-color var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease),
    background var(--dur) var(--ease);
}
.address-container:hover {
  border-color: var(--border-color);
}
.address-container:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}
.lock-icon {
  font-size: 11px;
  margin-right: 6px;
  flex-shrink: 0;
  opacity: 0.7;
  cursor: pointer;
}
.lock-icon:hover {
  opacity: 1;
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
  top: 37px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-pop);
  z-index: var(--z-dropdown);
  overflow: hidden;
  max-height: 320px;
  overflow-y: auto;
  padding: 5px;
}
.suggest-item {
  display: flex;
  align-items: center;
  gap: 9px;
  padding: 7px 11px;
  border-radius: var(--radius-xs);
  cursor: pointer;
  font-size: var(--text-base);
  color: var(--text-primary);
  transition: background var(--dur-fast) var(--ease);
}
.suggest-item.selected {
  background: var(--accent-soft);
  box-shadow: inset 0 0 0 1px var(--accent);
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
  font-size: var(--text-xs);
  text-align: right;
}

/* ===== 窄窗响应式（小窗口 / 横屏窄屏）===== */
@media (max-width: 860px) {
  .address-container {
    min-width: 60px;
  }
}
@media (max-width: 620px) {
  .address-bar {
    gap: 2px;
    padding: 0 5px;
  }
  .nav-btn,
  .action-btn {
    width: 28px;
    height: 28px;
  }
  /* 低频入口（密码/分组/用户脚本/历史）让位给地址栏，仍可由命令栏 Ctrl+K 到达 */
  .action-btn.hide-narrow {
    display: none;
  }
  .address-container {
    min-width: 0;
    padding: 0 11px;
  }
}
</style>

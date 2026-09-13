<script setup lang="ts">
// 地址栏（对应 Electron 版 .address-bar 设计）
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'

const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()
const addressInput = ref('')

const emit = defineEmits<{
  (e: 'openAi'): void
  (e: 'openPasswords'): void
  (e: 'openGroups'): void
  (e: 'openDownloads'): void
  (e: 'openSettings'): void
}>()

// 搜索引擎 URL 模板（与设置弹窗 SEARCH_ENGINES 一致）
const SEARCH_URLS: Record<string, string> = {
  bing: 'https://www.bing.com/search?q=',
  baidu: 'https://www.baidu.com/s?wd=',
  google: 'https://www.google.com/search?q=',
}

// 当前活动标签变化时同步地址栏
watch(
  () => tabsStore.activeTab?.url,
  (url) => {
    if (url) addressInput.value = url
  },
  { immediate: true }
)

/** 地址栏回车：输入 URL 或搜索 */
function onAddressEnter() {
  const input = addressInput.value.trim()
  if (!input) return
  const active = tabsStore.activeTab
  if (!active) return

  let url = input
  // 无协议时补全 https://
  if (!/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(input)) {
    // 带空格或非域名格式 → 走搜索（用设置的搜索引擎）
    if (/\s/.test(input) || !input.includes('.')) {
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
  const active = tabsStore.activeTab
  if (active) tabsStore.navigateTab(active.id, settingsStore.get('homepage') || 'https://www.bing.com')
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
    <button class="nav-btn" title="后退" @click="onBack">←</button>
    <button class="nav-btn" title="前进" @click="onForward">→</button>
    <button class="nav-btn" title="刷新" @click="onRefresh">↻</button>
    <button class="nav-btn" title="主页" @click="onHome">⌂</button>
    <div class="address-container">
      <input
        v-model="addressInput"
        type="text"
        placeholder="输入网址或搜索..."
        spellcheck="false"
        @keydown.enter="onAddressEnter"
      />
    </div>
    <button class="action-btn" :disabled="readerLoading" :title="readerTip || '阅读模式（提取正文大字看）'" @click="onReader">{{ readerLoading ? '⏳' : '📖' }}</button>
    <span v-if="readerTip" class="reader-tip">{{ readerTip }}</span>
    <button class="action-btn ai-btn" title="AI 助手" @click="emit('openAi')">AI</button>
    <button class="action-btn" title="密码管理" @click="emit('openPasswords')">🔑</button>
    <button class="action-btn" title="标签分组" @click="emit('openGroups')">🗂</button>
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
  transition: background 0.15s, color 0.15s;
}
.action-btn:disabled {
  opacity: 0.5;
  cursor: wait;
}
.reader-tip {
  font-size: 11px;
  color: #ff9b9b;
  max-width: 150px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex-shrink: 0;
}
.btn-icon {
  pointer-events: none;
  display: inline-block;
}
.ai-btn {
  color: #4a9eff;
  font-size: 13px;
  font-weight: 700;
  letter-spacing: 0.5px;
}
.nav-btn:hover:not(:disabled),
.action-btn:hover {
  background: var(--bg-active);
  color: #fff;
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
</style>
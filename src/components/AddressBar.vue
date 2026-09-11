<script setup lang="ts">
// 地址栏（对应 Electron 版 .address-bar 设计）
import { ref, watch } from 'vue'
import { useTabsStore } from '../stores/tabs'

const tabsStore = useTabsStore()
const addressInput = ref('')

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
    // 带空格或非域名格式 → 走搜索
    if (/\s/.test(input) || !input.includes('.')) {
      url = `https://www.bing.com/search?q=${encodeURIComponent(input)}`
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
  if (active) tabsStore.navigateTab(active.id, 'https://www.bing.com')
}

function onBack() {
  tabsStore.goBack()
}

function onForward() {
  tabsStore.goForward()
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
    <button class="action-btn" title="收藏">☆</button>
    <button class="action-btn" title="菜单">⋮</button>
  </div>
</template>

<style scoped>
.address-bar {
  display: flex;
  align-items: center;
  height: 42px;
  background: #1a1a1a;
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
  color: #999;
  border-radius: 6px;
  cursor: pointer;
  font-size: 15px;
  flex-shrink: 0;
  transition: background 0.15s, color 0.15s;
}
.nav-btn:hover:not(:disabled),
.action-btn:hover {
  background: #3d3d3d;
  color: #fff;
}
.nav-btn:disabled {
  opacity: 0.4;
  cursor: default;
}
.address-container {
  flex: 1;
  display: flex;
  align-items: center;
  background: #2d2d2d;
  border-radius: 16px;
  padding: 0 14px;
  height: 30px;
}
.address-container input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: #e0e0e0;
  font-size: 13px;
  height: 100%;
}
.address-container input::placeholder {
  color: #888;
}
</style>
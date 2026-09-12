<script setup lang="ts">
// 状态栏：当前 URL + 标签数 + 广告拦截状态 + 版本（自动读 tauri.conf.json）
import { computed, onMounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'

const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()

const currentUrl = computed(() => tabsStore.activeTab?.url ?? '')
const tabCount = computed(() => tabsStore.tabs.length)
// 与后端注入逻辑一致：默认开启（仅显式 'false' 才算关）
const adblockOn = computed(() => settingsStore.get('adblock_enabled') !== 'false')

const appVersion = ref('')
onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    appVersion.value = '5.5.0'
  }
})
</script>

<template>
  <div class="status-bar">
    <span class="status-left">{{ currentUrl }}</span>
    <span class="status-right">
      <span class="status-adblock" :class="{ off: !adblockOn }" :title="adblockOn ? '广告拦截：开' : '广告拦截：关'">
        🛡 {{ adblockOn ? '开' : '关' }}
      </span>
      <span class="status-sep">·</span>
      {{ tabCount }} 标签<span class="status-sep">·</span> v{{ appVersion }}
    </span>
  </div>
</template>

<style scoped>
.status-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 22px;
  background: var(--bg-primary);
  border-top: 1px solid var(--border-subtle);
  padding: 0 12px;
  flex-shrink: 0;
  user-select: none;
}
.status-left {
  color: var(--text-muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0; /* flexbox 默认 min-width:auto 会让长 URL 撑满空间、把 .status-right 挤出可视区 */
  margin-right: 12px;
}
.status-right {
  color: var(--text-muted);
  font-size: 11px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  white-space: nowrap;
}
.status-adblock {
  color: #43a047;
}
.status-adblock.off {
  color: var(--text-muted);
  opacity: 0.6;
}
.status-sep {
  opacity: 0.5;
}
</style>

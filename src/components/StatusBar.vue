<script setup lang="ts">
// 状态栏：左侧状态图标+文本+域名（原版 status-updated），
// 右侧下载/设置/帮助按钮 + 网络状态（原版 net-status-updated）+ 广告拦截 + 标签数 + 版本
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { getVersion } from '@tauri-apps/api/app'
import { listen } from '@tauri-apps/api/event'
import { openUrl } from '@tauri-apps/plugin-opener'
import { useTabsStore } from '../stores/tabs'
import { useSettingsStore } from '../stores/settings'

const tabsStore = useTabsStore()
const settingsStore = useSettingsStore()

// 打开下载/设置面板（App.vue 监听后打开对应弹窗）
const emit = defineEmits<{
  openDownloads: []
  openSettings: []
}>()

// ---- 左侧：加载状态 + 域名 ----
const isLoading = computed(() => tabsStore.loadingTabId !== null)
const statusIcon = computed(() => (isLoading.value ? '🔄' : '✓'))
const statusText = computed(() => (isLoading.value ? '加载中' : '就绪'))

// 当前 URL（原版显示 hostname，https 🔒 / http ⚠️；内部页面不显示域名）
const domainText = computed(() => {
  const url = tabsStore.activeTab?.url ?? ''
  if (!url || url.startsWith('shift:') || url.startsWith('file:')) return ''
  try {
    const u = new URL(url)
    if (u.protocol === 'https:') return '🔒 ' + u.hostname
    if (u.protocol === 'http:') return '⚠️ ' + u.hostname
    return ''
  } catch {
    return ''
  }
})

// ---- 右侧：网络状态（net-status-updated，每 2 秒推送）----
interface NetStatus {
  down_kb: number
  up_kb: number
  ping_ms: number
  online: boolean
}
const net = ref<NetStatus | null>(null)
let unlistenNet: (() => void) | null = null

function fmtSpeed(kb: number): string {
  return kb >= 1024 ? (kb / 1024).toFixed(1) + 'M' : Math.round(kb) + 'K'
}
const netSpeed = computed(() => {
  if (!net.value) return '📶 检测中...'
  const s = net.value
  return '⬇' + fmtSpeed(s.down_kb) + ' ⬆' + fmtSpeed(s.up_kb)
})
const netClass = computed(() => {
  if (!net.value) return 'net-mid'
  const s = net.value
  if (!s.online) return 'net-bad'
  if (s.ping_ms < 0) return 'net-mid'
  if (s.ping_ms < 80) return 'net-good'
  if (s.ping_ms < 200) return 'net-mid'
  return 'net-bad'
})
const netIcon = computed(() => {
  if (!net.value) return '📶'
  return net.value.online ? '📶' : '📵'
})
const netTitle = computed(() => {
  if (!net.value) return '网络状态：检测中'
  const s = net.value
  const ping = s.ping_ms >= 0 ? s.ping_ms + 'ms' : '未知'
  return (
    '网络状态：⬇下行 ' + fmtSpeed(s.down_kb) + '/s ⬆上行 ' + fmtSpeed(s.up_kb) + '/s · 延迟 ' + ping +
    (s.online ? '' : '（离线）')
  )
})

// ---- 版本 ----
const appVersion = ref('')
onMounted(async () => {
  try {
    appVersion.value = await getVersion()
  } catch {
    appVersion.value = '5.5.0'
  }
  try {
    unlistenNet = await listen<NetStatus>('net-status-updated', (e) => {
      net.value = e.payload
    })
  } catch (e) {
    console.error('[status] 监听网络状态失败:', e)
  }
})
onUnmounted(() => {
  if (unlistenNet) unlistenNet()
})

const adblockOn = computed(() => settingsStore.get('adblock_enabled') !== 'false')
const tabCount = computed(() => tabsStore.tabs.length)

function openHelp() {
  // 原版 open-help-external：打开 GitHub 项目页
  openUrl('https://github.com/gaogang/ShiftBrowser').catch((e) => {
    console.error('[status] 打开帮助失败:', e)
  })
}
</script>

<template>
  <div class="status-bar">
    <span class="status-left">
      <span class="status-icon">{{ statusIcon }}</span>
      <span class="status-text">{{ statusText }}</span>
      <span v-if="domainText" class="status-domain">{{ domainText }}</span>
    </span>
    <span class="status-right">
      <button class="status-btn" title="下载管理" @click="emit('openDownloads')">📥</button>
      <button class="status-btn" title="设置" @click="emit('openSettings')">⚙️</button>
      <button class="status-btn" title="帮助" @click="openHelp">❓</button>
      <span class="net-status" :class="netClass" :title="netTitle">{{ netIcon }} {{ netSpeed }}</span>
      <span
        class="status-adblock"
        :class="{ off: !adblockOn }"
        :title="adblockOn ? '广告拦截：开' : '广告拦截：关'"
      >
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
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0; /* flexbox 默认 min-width:auto 会让长 URL 撑满空间、把 .status-right 挤出可视区 */
  margin-right: 12px;
}
.status-icon {
  width: 14px;
  text-align: center;
}
.status-domain {
  color: var(--text-muted);
  opacity: 0.75;
  font-size: 10px;
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
.status-btn {
  background: none;
  border: none;
  color: var(--text-muted);
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 4px;
  font-size: 11px;
  line-height: 1;
}
.status-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.net-status {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  margin-left: 6px;
  padding: 2px 8px;
  border-radius: 10px;
  font-size: 10px;
  font-family: monospace;
  white-space: nowrap;
  cursor: default;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
}
.net-good {
  color: var(--success);
  border-color: rgba(67, 160, 71, 0.4);
}
.net-mid {
  color: #ff9800;
  border-color: rgba(255, 152, 0, 0.35);
}
.net-bad {
  color: var(--danger);
  border-color: rgba(229, 57, 53, 0.4);
}
.status-adblock {
  color: var(--success);
}
.status-adblock.off {
  color: var(--text-muted);
  opacity: 0.6;
}
.status-sep {
  opacity: 0.5;
}
</style>
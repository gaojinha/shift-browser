<script setup lang="ts">
// 状态栏（三段式）：
// 左段：加载状态 + 站点图标 + 域名（原版 status-updated）
// 中段：隐私防护战绩（拦截广告/清洗追踪参数，点击跳设置→隐私防护）
// 右段：网络胶囊（延迟+速度常驻）+ 下载（进行中红点角标）+ 设置 + 帮助
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
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
  openPrivacy: []
}>()

// ---- 左段：加载状态 + 站点图标 + 域名 ----
const isLoading = computed(() => tabsStore.loadingTabId !== null)
const statusIcon = computed(() => (isLoading.value ? '🔄' : '✓'))
const statusText = computed(() => (isLoading.value ? '加载中' : '就绪'))

// 当前站点图标（favicon；内部页面无）
const siteIcon = computed(() => {
  const url = tabsStore.activeTab?.url ?? ''
  if (!url || url.startsWith('shift:') || url.startsWith('file:')) return ''
  return tabsStore.activeTab?.favicon ?? ''
})

// 当前 URL（https 🔒 / http ⚠️；内部页面不显示域名）
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

// ---- 中段：隐私防护战绩（本次运行累计，来自 get_privacy_stats）----
interface PrivacyStats {
  cleaned_params: number
  privacy_injections: number
  adblock_injections: number
}
const privacy = ref<PrivacyStats | null>(null)
const adblockOn = computed(() => settingsStore.get('adblock_enabled') !== 'false')
const privacyText = computed(() => {
  if (!privacy.value) return '🛡 统计中...'
  return `🛡 已拦 ${privacy.value.adblock_injections} · 清参 ${privacy.value.cleaned_params}`
})
const privacyTitle = computed(() => {
  if (!privacy.value) return '隐私防护统计：读取中'
  const p = privacy.value
  return `隐私防护（本次运行）：广告拦截 ${p.adblock_injections} 次 · 清洗追踪参数 ${p.cleaned_params} 次 · 隐私注入 ${p.privacy_injections} 次（点击查看详情）`
})
const privacyClass = computed(() => (adblockOn.value ? 'privacy-on' : 'privacy-off'))

// ---- 右段：网络状态（net-status-updated，每 5 秒推送）----
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
const netPing = computed(() => {
  if (!net.value) return '--'
  const s = net.value
  return s.ping_ms >= 0 ? s.ping_ms + 'ms' : '--'
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

// ---- 右段：下载（进行中数量角标，downloads-updated 事件触发刷新）----
const activeDownloads = ref(0)
let unlistenDownloads: (() => void) | null = null
async function refreshActiveDownloads() {
  try {
    const list = await invoke<Array<{ state: string }>>('get_downloads')
    activeDownloads.value = list.filter((d) => d.state === 'in_progress').length
  } catch {
    activeDownloads.value = 0
  }
}

// 页面加载失败提示（5 秒后自动消失）
const loadFailed = ref(false)
const loadFailedUri = ref('')
let unlistenLoadFailed: (() => void) | null = null

onMounted(async () => {
  try {
    unlistenNet = await listen<NetStatus>('net-status-updated', (e) => {
      net.value = e.payload
    })
  } catch (e) {
    console.error('[status] 监听网络状态失败:', e)
  }
  try {
    unlistenLoadFailed = await listen<{ tabId: string; uri: string }>('page-load-failed', (e) => {
      loadFailed.value = true
      loadFailedUri.value = e.payload.uri
      setTimeout(() => {
        loadFailed.value = false
      }, 5000)
    })
  } catch (e) {
    console.error('[status] 监听页面加载失败失败:', e)
  }
  try {
    unlistenDownloads = await listen('downloads-updated', () => {
      refreshActiveDownloads()
    })
  } catch (e) {
    console.error('[status] 监听下载事件失败:', e)
  }
  // 初始拉一次隐私统计 + 下载数
  try {
    privacy.value = await invoke<PrivacyStats>('get_privacy_stats')
  } catch (e) {
    console.error('[status] 读取隐私统计失败:', e)
  }
  refreshActiveDownloads()
  // 隐私统计持续刷新（每 10 秒；改设置页重置后状态栏同步）
  window.setInterval(async () => {
    try {
      privacy.value = await invoke<PrivacyStats>('get_privacy_stats')
    } catch {
      /* 静默 */
    }
  }, 10000)
})
onUnmounted(() => {
  if (unlistenNet) unlistenNet()
  if (unlistenLoadFailed) unlistenLoadFailed()
  if (unlistenDownloads) unlistenDownloads()
})

function openHelp() {
  openUrl('https://gitcode.com/gaojinha/shiftbrowser-tauri').catch((e) => {
    console.error('[status] 打开帮助失败:', e)
  })
}
</script>

<template>
  <div class="status-bar">
    <span class="status-left">
      <span v-if="loadFailed" class="status-load-failed">⚠️ 页面加载失败</span>
      <span class="status-icon">{{ statusIcon }}</span>
      <span class="status-text">{{ statusText }}</span>
      <img v-if="siteIcon" class="status-favicon" :src="siteIcon" alt="" @error="(e) => ((e.target as HTMLImageElement).style.display = 'none')" />
      <span v-if="domainText" class="status-domain">{{ domainText }}</span>
    </span>

    <button class="privacy-badge" :class="privacyClass" :title="privacyTitle" @click="emit('openPrivacy')">
      {{ privacyText }}
    </button>

    <span class="status-right">
      <span class="net-status" :class="netClass" :title="netTitle">{{ netIcon }} {{ netPing }} {{ netSpeed }}</span>
      <button class="status-btn dl-btn" title="下载管理" @click="emit('openDownloads')">
        📥<span v-if="activeDownloads > 0" class="dl-badge">{{ activeDownloads }}</span>
      </button>
      <button class="status-btn" title="设置" @click="emit('openSettings')">⚙️</button>
      <button class="status-btn" title="帮助" @click="openHelp">❓</button>
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
  gap: 8px;
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
}
.status-load-failed {
  color: #e53935;
  font-size: 11px;
  animation: pulse 2s infinite;
}
@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.7; }
}
.status-icon {
  width: 14px;
  text-align: center;
}
.status-favicon {
  width: 12px;
  height: 12px;
  border-radius: 2px;
  object-fit: contain;
  flex: 0 0 auto;
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

/* 中段：隐私战绩（可点击跳设置） */
.privacy-badge {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 1px 10px;
  border-radius: 10px;
  font-size: 10px;
  white-space: nowrap;
  cursor: pointer;
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--success);
  transition: background 0.15s;
}
.privacy-badge:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.privacy-badge.privacy-off {
  color: var(--text-muted);
  opacity: 0.6;
}

/* 右段 */
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
  position: relative;
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
.dl-badge {
  position: absolute;
  top: -4px;
  right: -4px;
  min-width: 13px;
  height: 13px;
  padding: 0 3px;
  border-radius: 999px;
  background: var(--danger);
  color: #fff;
  font-size: 9px;
  line-height: 13px;
  text-align: center;
  font-weight: 700;
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
</style>
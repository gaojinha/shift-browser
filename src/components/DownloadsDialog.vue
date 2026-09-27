<script setup lang="ts">
// 下载管理弹窗
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此显示时隐藏所有标签 webview（同 PasswordsDialog），关闭时恢复。
import { onMounted, onUnmounted, watch } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import { setWebviewVisible } from '../utils/webview'
import { useDownloadsStore } from '../stores/downloads'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { EVENTS } from '../constants'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const downloadsStore = useDownloadsStore()

// 监听 Rust 侧下载事件（下载开始/进度/完成/失败），弹窗打开时实时刷新
let unlisten: (() => void) | null = null
onMounted(async () => {
  unlisten = await listen(EVENTS.DOWNLOADS_UPDATED, () => {
    if (props.show) {
      downloadsStore.loadDownloads().catch((e) => {
        console.error('[downloads] 事件刷新失败:', e)
      })
    }
  })
})
onUnmounted(() => {
  unlisten?.()
})

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        await downloadsStore.loadDownloads()
      }
    } catch (e) {
      console.error('[downloads] 加载失败:', e)
      useToastStore().error('加载下载记录失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'downloads-dialog')
      }
    }
  },
)

/** 格式化字节数（B/KB/MB） */
function formatBytes(n: number): string {
  if (n <= 0) return '0 B'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}

/** 进度百分比 */
function percent(d: { total_bytes: number; received_bytes: number }): number {
  if (d.total_bytes <= 0) return 0
  return Math.min(100, Math.round((d.received_bytes / d.total_bytes) * 100))
}

/** 状态文字 + 颜色 */
function stateInfo(state: string): { text: string; cls: string } {
  switch (state) {
    case 'completed':
      return { text: '已完成', cls: 'state-done' }
    case 'failed':
      return { text: '失败', cls: 'state-fail' }
    case 'cancelled':
      return { text: '已取消', cls: 'state-cancel' }
    case 'interrupted':
      return { text: '已中断（重启）', cls: 'state-fail' }
    default:
      return { text: '下载中', cls: 'state-run' }
  }
}

/** 重新下载：中断/失败且有来源链接时，新标签打开原链接触发新的下载 */
async function onRetry(url: string | null) {
  if (!url) return
  try {
    await useTabsStore().createTab(url)
    emit('close')
  } catch (e) {
    console.error('[downloads] 重新下载失败:', e)
    useToastStore().error('重新下载失败')
  }
}

async function onDelete(id: number) {
  await downloadsStore.deleteDownload(id)
}

/** 打开已下载的文件（completed 状态可用） */
async function onOpen(path: string | null) {
  if (!path) return
  try {
    await openPath(path)
  } catch (e) {
    console.error('[downloads] 打开文件失败:', e)
  }
}

/** 在文件管理器中显示（completed 状态可用） */
async function onReveal(path: string | null) {
  if (!path) return
  try {
    await revealItemInDir(path)
  } catch (e) {
    console.error('[downloads] 打开所在文件夹失败:', e)
  }
}
</script>

<template>
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <span>下载管理</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
      </div>

      <div class="dialog-body">
        <div class="list" v-if="downloadsStore.list.length > 0">
          <div v-for="d in downloadsStore.list" :key="d.id" class="item">
            <div class="item-info">
              <span class="item-name">{{ d.filename }}</span>
              <span class="item-url">{{ d.url || '未知来源' }}</span>
              <div class="item-progress">
                <div class="progress-track">
                  <div class="progress-fill" :style="{ width: percent(d) + '%' }"></div>
                </div>
                <span class="progress-text">
                  {{ formatBytes(d.received_bytes) }} / {{ formatBytes(d.total_bytes) }} ({{
                    percent(d)
                  }}%)
                </span>
              </div>
            </div>
            <div class="item-actions">
              <span class="state" :class="stateInfo(d.state).cls">{{
                stateInfo(d.state).text
              }}</span>
              <template v-if="d.state === 'completed' && d.path">
                <button class="mini-btn" @click="onOpen(d.path)">打开</button>
                <button class="mini-btn" @click="onReveal(d.path)">文件夹</button>
              </template>
              <template v-else-if="(d.state === 'interrupted' || d.state === 'failed') && d.url">
                <button class="mini-btn" @click="onRetry(d.url)">重新下载</button>
              </template>
              <button class="mini-btn danger" @click="onDelete(d.id)">删除</button>
            </div>
          </div>
        </div>
        <div v-else-if="downloadsStore.loading" class="empty loading-hint">加载中…</div>
        <div v-else class="empty">还没有下载记录</div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
/* 下载弹窗比默认弹窗宽一点（520px） */
.dialog {
  width: min(520px, calc(100vw - 32px));
}
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 10px 12px;
  gap: 10px;
}
.item-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
  flex: 1;
}
.item-name {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.item-url {
  color: var(--text-muted);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.item-progress {
  display: flex;
  align-items: center;
  gap: 8px;
}
.progress-track {
  flex: 1;
  height: 6px;
  background: var(--bg-secondary);
  border-radius: 3px;
  overflow: hidden;
}
.progress-fill {
  height: 100%;
  background: var(--accent);
  border-radius: 3px;
  transition: width 0.3s;
}
.progress-text {
  color: var(--text-muted);
  font-size: 11px;
  flex-shrink: 0;
}
.item-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.state {
  font-size: var(--text-sm);
  padding: 2px 10px;
  border-radius: var(--radius-full);
}
.state-run {
  color: var(--accent);
  background: var(--accent-soft);
}
.state-done {
  color: var(--success);
  background: var(--success-soft);
}
.state-fail {
  color: var(--danger);
  background: var(--danger-soft);
}
.state-cancel {
  color: var(--text-muted);
  background: var(--bg-secondary);
}
.mini-btn {
  background: var(--bg-active);
  color: var(--text-secondary);
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
}
.mini-btn:hover {
  background: var(--text-secondary);
}
.mini-btn.danger:hover {
  background: #5c2b2b;
  color: #ff8a8a;
}
.empty {
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 24px 0;
}
</style>

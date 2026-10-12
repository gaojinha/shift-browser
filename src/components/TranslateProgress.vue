<script setup lang="ts">
// 翻译进度条（挂在地址栏）：参照原版 Electron 的 #translateProgress 蓝色胶囊
// 数据来源：整页翻译脚本 → shift://__translate__/progress → Rust emit "translate-progress"
import { onMounted, onUnmounted, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { EVENTS } from '../constants'

interface ProgressPayload {
  state: 'start' | 'update' | 'done' | 'hide'
  engine: string
  pct: number
  info: string
  elapsed: number
  eta: number
  fails: number
}

const visible = ref(false)
const engine = ref('Google')
const pct = ref(0)
const info = ref('')
const timeText = ref('')
const fails = ref(0)

let unlisten: (() => void) | null = null
let hideTimer: number | null = null

function clearHideTimer() {
  if (hideTimer !== null) {
    clearTimeout(hideTimer)
    hideTimer = null
  }
}

function dismiss() {
  visible.value = false
  clearHideTimer()
}

onMounted(async () => {
  try {
    unlisten = await listen<ProgressPayload>(EVENTS.TRANSLATE_PROGRESS, (e) => {
      const p = e.payload
      if (p.state === 'hide') {
        dismiss()
        return
      }
      visible.value = true
      clearHideTimer()
      engine.value = p.engine || 'Google'
      pct.value = Math.max(0, Math.min(100, p.pct || 0))
      info.value = p.info || ''
      fails.value = p.fails || 0
      if (p.state === 'start') {
        timeText.value = '准备中…'
      } else if (p.state === 'done') {
        pct.value = 100
        timeText.value = '完成 · ' + p.elapsed + 's'
        // 完成态停留 5 秒后自动收起
        hideTimer = window.setTimeout(() => {
          visible.value = false
          hideTimer = null
        }, 5000)
      } else {
        timeText.value = p.elapsed + 's' + (p.eta > 0 ? ' · 剩' + p.eta + 's' : '')
      }
    })
  } catch (e) {
    console.error('[translate] 监听翻译进度失败:', e)
  }
})

onUnmounted(() => {
  if (unlisten) unlisten()
  clearHideTimer()
})
</script>

<template>
  <div
    v-if="visible"
    class="tp"
    :title="`翻译中 ${info}${fails ? '（' + fails + ' 段未译）' : ''}　点击收起`"
    @click="dismiss"
  >
    <span class="tp-engine">{{ engine }}</span>
    <div class="tp-track"><div class="tp-fill" :style="{ width: pct + '%' }"></div></div>
    <span class="tp-pct">{{ pct }}%</span>
    <span class="tp-info">{{ info }}</span>
    <span class="tp-info">{{ timeText }}</span>
    <span v-if="fails" class="tp-fails">✕{{ fails }}</span>
  </div>
</template>

<style scoped>
.tp {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 26px;
  padding: 0 8px;
  margin-right: 6px;
  border-radius: var(--radius-xs);
  background: var(--accent);
  color: var(--accent-contrast);
  font-size: var(--text-xs);
  flex-shrink: 0;
  white-space: nowrap;
  cursor: pointer;
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.3);
}
.tp-engine {
  background: rgba(255, 255, 255, 0.2);
  border-radius: var(--radius-2xs);
  padding: 1px 5px;
  font-size: 10px;
  flex-shrink: 0;
}
.tp-track {
  width: 70px;
  height: 5px;
  background: rgba(255, 255, 255, 0.3);
  border-radius: 3px;
  overflow: hidden;
  flex-shrink: 0;
}
.tp-fill {
  height: 100%;
  width: 0%;
  background: var(--accent-contrast);
  border-radius: 3px;
  transition: width var(--dur-slow);
}
.tp-pct {
  font-weight: 700;
  flex-shrink: 0;
}
.tp-info {
  color: var(--accent-contrast);
  flex-shrink: 0;
}
.tp-fails {
  /* 蓝底专用浅红，白色底上对比不足所以不用 danger 原色 */
  color: #ffcdd2;
  flex-shrink: 0;
}
</style>

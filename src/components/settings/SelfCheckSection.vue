<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { fixFor } from '../../utils/advice'
import './ui/settings-shared.css'

// ---- 系统自检（可观测性）：一键汇总版本/存储/运行时/能力状态 ----
interface CheckItem {
  name: string
  ok: boolean
  detail: string
}
interface SelfCheckReport {
  healthy: boolean
  timestamp: number
  versions: CheckItem[]
  storage: CheckItem[]
  runtime: CheckItem[]
  capabilities: CheckItem[]
}
const selfCheckReport = ref<SelfCheckReport | null>(null)
const selfCheckLoading = ref(false)
const selfCheckError = ref('')

async function runSelfCheck() {
  if (selfCheckLoading.value) return
  selfCheckLoading.value = true
  selfCheckError.value = ''
  try {
    selfCheckReport.value = await invoke<SelfCheckReport>('selfcheck_cmd')
  } catch (e) {
    selfCheckError.value = String(e)
  } finally {
    selfCheckLoading.value = false
  }
}

/** 自检项分组（模板里遍历用） */
const selfCheckGroups = computed(() => {
  const r = selfCheckReport.value
  if (!r) return []
  return [
    { title: '版本与环境', items: r.versions },
    { title: '数据存储', items: r.storage },
    { title: '运行时', items: r.runtime },
    { title: '能力状态', items: r.capabilities },
  ]
})

/** 打开自检分区时自动跑一次 */
onMounted(() => {
  runSelfCheck()
})

/** 红项一句话处置（单源见 utils/advice，组件只展示） */

</script>

<template>
  <div class="section-stack">
            <div class="card" data-anchor="selfcheck">
              <div class="card-title">
                <AppIcon name="activity" :size="14" /> 系统自检
                <button class="selfcheck-btn" :disabled="selfCheckLoading" @click="runSelfCheck">
                  {{ selfCheckLoading ? '检查中…' : '重新检查' }}
                </button>
              </div>
              <div v-if="selfCheckError" class="selfcheck-error">{{ selfCheckError }}</div>
              <div v-else-if="!selfCheckReport" class="selfcheck-hint">点击「重新检查」开始体检</div>
              <template v-else>
                <div class="selfcheck-summary" :class="selfCheckReport.healthy ? 'ok' : 'bad'">
                  {{ selfCheckReport.healthy ? '✅ 一切正常' : '⚠️ 发现异常项' }}
                </div>
                <div v-for="g in selfCheckGroups" :key="g.title" class="selfcheck-group">
                  <div class="selfcheck-group-title">{{ g.title }}</div>
                  <div v-for="it in g.items" :key="g.title + it.name" class="selfcheck-row">
                    <span class="selfcheck-mark">{{ it.ok ? '✅' : '❌' }}</span>
                    <span class="selfcheck-name">{{ it.name }}</span>
                    <span class="selfcheck-detail">{{ it.detail }}</span>
                  </div>
                  <div v-for="it in g.items.filter((x) => !x.ok)" :key="g.title + it.name + '-fix'" class="selfcheck-fix">
                    → {{ it.name }}：{{ fixFor(it.name) }}
                  </div>
                </div>
              </template>
            </div>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>

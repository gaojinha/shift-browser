<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'

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

/** 红项一句话处置：按项名关键字匹配，修法只说真实存在的入口 */
function fixFor(name: string): string {
  if (name.includes('内容过滤器')) return '去「设置 → 隐私防护」开关一次广告拦截，等编译完成再查。'
  if (name.includes('密码主密钥')) return '检查数据目录权限，或恢复主密钥备份；缺失则已存密码解不开。'
  if (name.includes('数据库') || name.includes('数据表'))
    return '先确认磁盘没满、数据目录可写，再重启浏览器重查。'
  if (name.includes('数据目录')) return '确认主目录存在可写；snap/容器环境看挂载路径。'
  if (name.includes('主窗口') || name.includes('标签管理器')) return '重启浏览器；仍红则看日志报上来。'
  if (name.includes('隐私') || name.includes('广告拦截')) return '去「设置 → 隐私防护」核对开关档位。'
  if (name.includes('扩展')) return '去扩展管理看哪家没启用。'
  return '重启重查一次；仍红把这行原文报上来。'
}

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
.settings-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
}
.settings-scroll::-webkit-scrollbar {
  width: 6px;
}
.settings-scroll::-webkit-scrollbar-thumb {
  background: var(--bg-active);
  border-radius: 3px;
}
.card {
  margin-bottom: 12px;
}
.selfcheck-btn {
  float: right;
  padding: 3px 10px;
  font-size: 12px;
  color: var(--accent);
  background: transparent;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  cursor: pointer;
}
.selfcheck-btn:disabled {
  opacity: 0.5;
  cursor: default;
}
.selfcheck-summary {
  margin: 8px 0 12px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.selfcheck-summary.ok {
  background: var(--success-soft);
  color: var(--success);
}
.selfcheck-summary.bad {
  background: var(--danger-soft);
  color: var(--danger);
}
.selfcheck-hint {
  color: var(--text-muted);
  font-size: 13px;
  padding: 12px 0;
}
.selfcheck-error {
  color: var(--danger);
  font-size: var(--text-sm);
  padding: 8px 0;
}
.selfcheck-group {
  margin-bottom: 12px;
}
.selfcheck-group-title {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.selfcheck-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 3px 0;
  font-size: 13px;
}
.selfcheck-mark {
  flex-shrink: 0;
}
.selfcheck-name {
  flex-shrink: 0;
  min-width: 96px;
  color: var(--text-primary);
}
.selfcheck-detail {
  color: var(--text-muted);
  word-break: break-all;
}
.selfcheck-fix {
  font-size: 12px;
  color: var(--warning);
  padding: 2px 0 2px 24px;
  line-height: 1.6;
}
.card-title {
  display: flex;
  align-items: center;
  gap: 7px;
  font-size: 14px;
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 12px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--border-subtle);
}
.card-title .app-icon {
  color: var(--accent);
}
.field-hint {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary);
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 8px 10px;
  margin-bottom: 10px;
}
.field-hint code {
  color: var(--accent);
  font-family: var(--font-mono, monospace);
  font-size: 11px;
}
</style>

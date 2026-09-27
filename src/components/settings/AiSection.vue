<script setup lang="ts">
// 设置弹窗 · AI 助手栏（IA 重组 2026-09）
// 栏内三子页签：模型服务 / MCP 工具 / 试验特性；非激活页签懒挂载（避免无谓取数）
import { computed, onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { invokeOk } from '../../utils/invoke'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import McpSection from './McpSection.vue'
import WebmcpCard from './WebmcpCard.vue'

type AiTabId = 'model' | 'mcp' | 'labs'

const props = defineProps<{ initialTab?: AiTabId }>()

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

// ── 本机 opencode Provider ──────────────────────────────
// 侧边栏选「本机 opencode」后：凭据由后端从运行中的 serve 进程自动发现，
// 模型在这里动态拉取（GET /api/model），选中值存 opencode_model（providerID/modelID）。
interface OcModel {
  modelRef: string
  providerId: string
  modelId: string
  name: string
  status: string
  enabled: boolean
  deprecated: boolean
}
interface OcProbe {
  ok: boolean
  baseUrl: string
  model: string
  askAgentPresent: boolean
}
const isOc = computed(() => (settingsStore.get('ai_provider') || 'agnes') === 'opencode')
const isLocal = computed(() => (settingsStore.get('ai_provider') || 'agnes') === 'local')
const ocModels = ref<OcModel[]>([])
const ocProbe = ref<OcProbe | null>(null)
const ocLoading = ref(false)
const ocError = ref('')
const ocLoaded = ref(false)
const ocModel = computed({
  get: () => settingsStore.get('opencode_model') || '',
  set: (v: string) => void onSet('opencode_model', v),
})

async function loadOc(force = false) {
  if (ocLoaded.value && !force) return
  ocLoading.value = true
  ocError.value = ''
  try {
    // CommandResult 命令：invokeOk 返回裸值（probe 是 {ok,baseUrl,model,...}）
    const [probe, models] = await Promise.all([
      invokeOk<OcProbe>('opencode_probe'),
      invokeOk<OcModel[]>('opencode_list_models'),
    ])
    ocProbe.value = probe
    ocModels.value = models
    ocLoaded.value = true
  } catch (e) {
    ocError.value = e instanceof Error ? e.message : String(e)
  } finally {
    ocLoading.value = false
  }
}
watch(isOc, (v) => {
  if (v) void loadOc()
}, { immediate: true })

const aiKeyInput = ref('')
const hasAiKey = () => {
  const v = settingsStore.get('ai_api_key')
  return v !== '' && v !== undefined
}
async function onAiKeyChange() {
  const v = aiKeyInput.value.trim()
  if (v) {
    await settingsStore.set('ai_api_key', v)
  }
  aiKeyInput.value = ''
}

// AI Provider 列表（阶段 1.2：多 provider 抽象，设置页可配）
const aiProviders = ref<Array<{ id: string; name: string; configured: boolean; description: string }>>([])
async function loadAiProviders() {
  try {
    aiProviders.value = await invoke('ai_list_providers')
  } catch (e) {
    console.error('[settings] 加载 provider 列表失败:', e)
  }
}
onMounted(loadAiProviders)

const AI_TABS: Array<{ id: AiTabId; label: string; icon: string }> = [
  { id: 'model', label: '模型服务', icon: 'sparkles' },
  { id: 'mcp', label: 'MCP 工具', icon: 'plug' },
  { id: 'labs', label: '试验特性', icon: 'activity' },
]
const activeTab = ref<AiTabId>(props.initialTab ?? 'model')
// 页签访问记录：首次激活才挂载（模型页随栏挂载即加载）
const visited = ref<Record<AiTabId, boolean>>({
  model: true,
  mcp: props.initialTab === 'mcp',
  labs: props.initialTab === 'labs',
})
function switchTab(id: AiTabId) {
  activeTab.value = id
  visited.value[id] = true
}
// 外部（设置搜索 / 深链 ai#mcp）在组件已挂载后切换页签
watch(
  () => props.initialTab,
  (t) => {
    if (t && t !== activeTab.value) switchTab(t)
  },
)
</script>

<template>
  <div class="ai-pane">
    <div class="ai-tabs">
      <button
        v-for="t in AI_TABS"
        :key="t.id"
        class="ai-tab"
        :class="{ active: activeTab === t.id }"
        @click="switchTab(t.id)"
      >
        <AppIcon :name="t.icon" :size="13" />
        {{ t.label }}
      </button>
    </div>

    <div v-show="activeTab === 'model'" class="settings-scroll">
      <div class="card" data-anchor="model">
        <div class="card-title"><AppIcon name="sparkles" :size="14" /> 模型服务</div>
        <div class="field">
          <label>推理服务（Provider）</label>
          <select
            :value="settingsStore.get('ai_provider') || 'agnes'"
            @change="onSet('ai_provider', ($event.target as HTMLSelectElement).value)"
          >
            <option
              v-for="p in aiProviders"
              :key="p.id"
              :value="p.id"
            >
              {{ p.name }}
            </option>
            <option v-if="aiProviders.length === 0" value="agnes">AGNES 云端</option>
          </select>
          <div class="field-hint" v-if="settingsStore.get('ai_provider') === 'local'">
            本机 Ollama（默认连 127.0.0.1:11434，需先在本机起好 Ollama 并拉好模型；连不上自动回退 AGNES 云端）。
          </div>
        </div>

        <!-- AGNES 云端专属配置 -->
        <template v-if="!isOc && !isLocal">
          <div class="field">
            <label>模型</label>
            <select
              :value="settingsStore.get('ai_model')"
              @change="onSet('ai_model', ($event.target as HTMLSelectElement).value)"
            >
              <option value="agnes-2.0-flash">agnes-2.0-flash（推荐）</option>
              <option value="agnes-2.5-pro">agnes-2.5-pro</option>
              <option value="agnes-2.5-flash">agnes-2.5-flash（可能超时）</option>
            </select>
          </div>
          <div class="field">
            <label>API 地址</label>
            <input
              type="text"
              :value="settingsStore.get('ai_base_url')"
              placeholder="https://apihub.agnes-ai.com/v1"
              spellcheck="false"
              @change="onSet('ai_base_url', ($event.target as HTMLInputElement).value.trim())"
            />
          </div>
          <div class="field">
            <label>API Key</label>
            <input
              type="password"
              v-model="aiKeyInput"
              :placeholder="hasAiKey() ? '已配置（不回显），留空保持不变' : '留空使用环境变量 AGNES_API_KEY'"
              spellcheck="false"
              @change="onAiKeyChange"
            />
          </div>
          <div class="field-row">
            <div class="field">
              <label>请求超时（秒）</label>
              <input
                type="number"
                min="10"
                max="300"
                :value="settingsStore.get('ai_timeout') || '90'"
                @change="onSet('ai_timeout', ($event.target as HTMLInputElement).value)"
              />
            </div>
            <div class="field">
              <label>流式超时（秒）</label>
              <input
                type="number"
                min="10"
                max="300"
                :value="settingsStore.get('ai_stream_timeout') || '120'"
                @change="onSet('ai_stream_timeout', ($event.target as HTMLInputElement).value)"
              />
            </div>
          </div>
          <div class="field-hint">范围 10-300 秒，慢网可调大；超范围回落默认值（90/120）。</div>
        </template>

        <!-- 本机 Ollama 专属配置（复用 ai_model 键，无新增契约） -->
        <template v-else-if="isLocal">
          <div class="field">
            <label>模型</label>
            <input
              type="text"
              :value="settingsStore.get('ai_model')"
              placeholder="llama3.1（填 Ollama 本机已拉取的模型名）"
              spellcheck="false"
              @change="onSet('ai_model', ($event.target as HTMLInputElement).value.trim())"
            />
          </div>
          <div class="field-hint">
            直连本机 Ollama（127.0.0.1:11434）；Ollama 没在跑或超时时自动回退 AGNES 云端（回退用云端默认模型）。
          </div>
        </template>

        <!-- 本机 opencode 专属配置 -->
        <template v-else>
          <div class="field-hint">
            侧边栏对话将转发到本机 opencode serve（127.0.0.1:4096）。凭据自动从运行中的
            serve 进程发现（也可用设置项 opencode_username / opencode_password）；
            每轮问答使用只读 plan agent，会话保留在 opencode 控制台可回看。
          </div>

          <div class="field oc-status">
            <label>连接状态</label>
            <div class="oc-status-body">
              <span v-if="ocLoading" class="oc-msg">正在连接本机 opencode…</span>
              <span v-else-if="ocError" class="oc-msg oc-err">
                {{ ocError }}
                <button class="oc-retry" @click="loadOc(true)">重试</button>
              </span>
              <template v-else-if="ocProbe">
                <span class="oc-msg oc-ok">已连接 {{ ocProbe.baseUrl }}</span>
                <span class="oc-msg">opencode 默认模型：{{ ocProbe.model || '（未配置）' }}</span>
                <span v-if="!ocProbe.askAgentPresent" class="oc-msg oc-err">
                  未检测到只读 plan agent，发问会被后端拒绝（防止静默回退到可写 agent）
                </span>
              </template>
            </div>
          </div>

          <div class="field">
            <label>opencode 模型</label>
            <select v-model="ocModel" :disabled="ocLoading || !!ocError">
              <option value="">
                跟随 opencode 默认{{ ocProbe?.model ? `（${ocProbe.model}）` : '' }}
              </option>
              <option v-for="m in ocModels" :key="m.modelRef" :value="m.modelRef">
                {{ m.name }}（{{ m.modelRef }}）{{ m.deprecated ? ' · 已弃用' : '' }}
              </option>
            </select>
          </div>
          <div class="field-hint" v-if="ocLoaded && ocModels.length === 0 && !ocError">
            没拿到可用模型列表（opencode 可能尚未配置任何提供商）。
          </div>
        </template>
      </div>
    </div>

    <div v-show="activeTab === 'mcp'" class="settings-scroll">
      <McpSection v-if="visited.mcp" />
    </div>

    <div v-show="activeTab === 'labs'" class="settings-scroll">
      <WebmcpCard v-if="visited.labs" />
    </div>
  </div>
</template>

<style scoped>
.ai-pane { flex: 1; min-height: 0; display: flex; flex-direction: column; }.ai-tabs { display: flex; gap: 4px; padding: 10px 16px 0; border-bottom: 1px solid var(--border-subtle); flex-shrink: 0; }.ai-tab { display: inline-flex; align-items: center; gap: 6px; padding: 7px 14px; font-size: 12.5px; color: var(--text-secondary); background: transparent; border: 1px solid transparent; border-bottom: none; border-radius: 8px 8px 0 0; cursor: pointer; transition: all 0.15s; }.ai-tab:hover { color: var(--text-primary); background: var(--bg-hover); }.ai-tab.active { color: var(--accent); background: var(--accent-soft); border-color: var(--border-subtle); font-weight: 600; }.settings-scroll { flex: 1; overflow-y: auto; padding: 14px 16px; min-height: 0; }.settings-scroll::-webkit-scrollbar { width: 6px; }.settings-scroll::-webkit-scrollbar-thumb { background: var(--bg-active); border-radius: 3px; }.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field-row { display: flex; gap: 12px; }
.field-row .field { flex: 1; }
.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field-hint { font-size: 12px; line-height: 1.6; color: var(--text-secondary); background: var(--bg-primary); border: 1px solid var(--border-subtle); border-radius: 8px; padding: 8px 10px; margin-bottom: 10px; }.field-hint code { color: var(--accent); font-family: var(--font-mono, monospace); font-size: 11px; }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.field input,
.field select { flex: 1; max-width: 300px; border: 1px solid var(--border-color); background: var(--bg-primary); color: var(--text-primary); border-radius: 7px; padding: 7px 11px; font-size: 13px; outline: none; transition: border-color 0.15s; -webkit-appearance: none; appearance: none; }.field select { padding-right: 26px; background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23c6d0df' d='M2.5 4.5L6 8l3.5-3.5'/></svg>"); background-repeat: no-repeat; background-position: right 8px center; background-size: 12px; }.field input:focus,
.field select:focus { border-color: var(--accent); }
/* 本机 opencode 状态区（多行文本，不走 .field 的两端对齐单行布局） */
.field.oc-status { align-items: flex-start; }
.oc-status-body { flex: 1; max-width: 300px; display: flex; flex-direction: column; gap: 4px; }
.oc-msg { font-size: 12px; line-height: 1.5; color: var(--text-secondary); }
.oc-msg.oc-ok { color: var(--success, #3fb950); }
.oc-msg.oc-err { color: var(--danger, #f85149); }
.oc-retry { margin-left: 6px; padding: 1px 10px; font-size: 11.5px; color: var(--accent); background: transparent; border: 1px solid var(--border-color); border-radius: 6px; cursor: pointer; }
.oc-retry:hover { background: var(--bg-hover); }
</style>

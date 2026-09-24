<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

// SelfCheck / About 逻辑已移至 settings/SelfCheckSection.vue、AboutSection.vue
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
onMounted(() => {
  loadAiProviders()
})

</script>

<template>
  <div class="settings-scroll">
            <div class="card">
              <div class="card-title"><AppIcon name="sparkles" :size="14" /> AI 助手</div>
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
                    :disabled="p.id === 'local'"
                  >
                    {{ p.name }}{{ p.id === 'local' ? '（暂缓）' : '' }}
                  </option>
                  <option v-if="aiProviders.length === 0" value="agnes">AGNES 云端</option>
                </select>
                <div class="field-hint" v-if="settingsStore.get('ai_provider') === 'local'">
                  本地模型集成暂缓（2026-09-19 决策），请选择 AGNES 云端。
                </div>
              </div>
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
.field {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}
.field-desc {
  flex: 1;
  font-size: 12px;
  color: var(--text-secondary);
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
.field label {
  font-size: 13px;
  color: var(--text-secondary);
  flex-shrink: 0;
}
.field select {
  flex: 1;
  max-width: 300px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 7px 11px;
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
  /* WebKitGTK 下 select/input 默认用系统白色渲染，强制自定义样式 */
  -webkit-appearance: none;
  appearance: none;
}
.field select {
  padding-right: 26px;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23c6d0df' d='M2.5 4.5L6 8l3.5-3.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 12px;
}
.field select:focus {
  border-color: var(--accent);
}
.seg-group {
  display: flex;
  gap: 6px;
  flex: 1;
  max-width: 300px;
}
.seg-btn {
  flex: 1;
  padding: 6px 0;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: 7px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}
.seg-btn:hover {
  background: var(--bg-hover);
}
.seg-btn.active {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
  font-weight: 600;
}
.switch {
  position: relative;
  display: inline-block;
  width: 36px;
  height: 20px;
  flex-shrink: 0;
}
.switch input {
  opacity: 0;
  width: 0;
  height: 0;
}
.slider {
  position: absolute;
  cursor: pointer;
  inset: 0;
  background: var(--bg-active);
  border-radius: 10px;
  transition: 0.2s;
}
.switch input:checked + .slider {
  background: var(--accent);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: #fff;
}
</style>

<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useExtensionsStore } from '../../stores/extensions'
import { useToastStore } from '../../stores/toast'
import type { Extension } from '../../types/extension'

const extensionsStore = useExtensionsStore()

const extPath = ref('')
const extError = ref('')
const extLoading = ref(false)

/** 加载扩展（目录或 manifest.json 路径） */
async function onLoadExtension() {
  const path = extPath.value.trim()
  if (!path) {
    extError.value = '请输入扩展目录或 manifest.json 路径'
    return
  }
  extLoading.value = true
  extError.value = ''
  try {
    await extensionsStore.loadExtension(path)
    extPath.value = ''
  } catch (e) {
    extError.value = String(e)
  } finally {
    extLoading.value = false
  }
}

/** 切换启用状态 */
async function onToggleExtension(e: Extension) {
  try {
    await extensionsStore.setEnabled(e.id, !e.enabled)
  } catch (err) {
    console.error('[settings] 切换扩展状态失败:', err)
    useToastStore().error('切换扩展状态失败')
  }
}

/** 删除扩展 */
async function onDeleteExtension(id: number) {
  try {
    await extensionsStore.deleteExtension(id)
  } catch (err) {
    console.error('[settings] 删除扩展失败:', err)
    useToastStore().error('删除扩展失败')
  }
}

// 列表在分区挂载时加载（R2-1d：原由 SettingsDialog 的 show watch 统一预加载）
onMounted(() => {
  extensionsStore.loadExtensions().catch((e) => {
    console.error('[settings] 加载扩展失败:', e)
  })
})
</script>

<template>
  <div class="settings-scroll">
            <div class="card">
              <div class="card-title"><AppIcon name="puzzle" :size="14" /> 扩展</div>
              <div class="ext-load-row">
                <input
                  v-model="extPath"
                  type="text"
                  placeholder="扩展目录或 manifest.json 路径"
                  spellcheck="false"
                  @keyup.enter="onLoadExtension"
                />
                <button class="mini-btn primary" :disabled="extLoading" @click="onLoadExtension">
                  {{ extLoading ? '加载中…' : '加载' }}
                </button>
              </div>
              <div v-if="extError" class="ext-error">{{ extError }}</div>
              <div class="ext-list" v-if="extensionsStore.list.length > 0">
                <div v-for="e in extensionsStore.list" :key="e.id" class="ext-item">
                  <div class="ext-info">
                    <span class="ext-name">{{ e.name }}</span>
                    <span class="ext-desc">
                      {{ e.version ? 'v' + e.version : '' }}
                      {{ e.description ? '· ' + e.description : '' }}
                    </span>
                  </div>
                  <div class="ext-actions">
                    <span v-if="e.js_count" class="ext-badge">JS</span>
                    <span v-if="e.css_count" class="ext-badge">CSS</span>
                    <label class="switch">
                      <input type="checkbox" :checked="e.enabled" @change="onToggleExtension(e)" />
                      <span class="slider"></span>
                    </label>
                    <button class="mini-btn danger" @click="onDeleteExtension(e.id)">删除</button>
                  </div>
                </div>
              </div>
              <div v-else class="ext-empty">还没有扩展，输入路径加载一个试试</div>
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

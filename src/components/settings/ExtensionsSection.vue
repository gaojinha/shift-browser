<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useExtensionsStore } from '../../stores/extensions'
import { useToastStore } from '../../stores/toast'
import type { Extension } from '../../types/extension'
import './ui/settings-shared.css'

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
  <div class="section-stack">
            <div class="card" data-anchor="extensions">
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
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>

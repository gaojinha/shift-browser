<script setup lang="ts">
// 设置弹窗（通用 / 外观 / AI 助手 / 隐私 / 扩展）
// 设置项存 settings 表（key-value），保存即生效
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../stores/settings'
import { useExtensionsStore } from '../stores/extensions'
import type { Extension } from '../types/extension'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const settingsStore = useSettingsStore()
const extensionsStore = useExtensionsStore()

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
watch(
  () => props.show,
  async (v) => {
    if (v) {
      if (!settingsStore.loaded) settingsStore.loadSettings().catch(() => {})
      extensionsStore.loadExtensions().catch(() => {})
      loadKeywords().catch(() => {})
    }
    await invoke('set_webview_visible', { visible: !v })
  }
)

const SEARCH_ENGINES = [
  { id: 'bing', label: '必应', url: 'https://www.bing.com/search?q=' },
  { id: 'baidu', label: '百度', url: 'https://www.baidu.com/s?wd=' },
  { id: 'google', label: '谷歌', url: 'https://www.google.com/search?q=' },
]

const clearedMsg = ref('')

// 扩展管理：加载路径输入
const extPath = ref('')
const extError = ref('')
const extLoading = ref(false)

async function onSet(key: string, value: string) {
  await settingsStore.set(key, value)
}

async function onClearHistory() {
  await settingsStore.clearHistory()
  clearedMsg.value = '历史记录已清除'
  setTimeout(() => (clearedMsg.value = ''), 2000)
}

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
  await extensionsStore.setEnabled(e.id, !e.enabled).catch(() => {})
}

/** 删除扩展 */
async function onDeleteExtension(id: number) {
  await extensionsStore.deleteExtension(id).catch(() => {})
}

/** 广告拦截开关 */
async function onToggleAdblock() {
  const current = settingsStore.get('adblock_enabled') !== 'false'
  await settingsStore.set('adblock_enabled', (!current).toString())
  await invoke('set_adblock_enabled', { enabled: !current })
}

/** 用户自定义广告关键词 */
const customKeywords = ref<string[]>([])
const newKeyword = ref('')
const kwMsg = ref('')

async function loadKeywords() {
  try {
    customKeywords.value = await invoke<string[]>('get_adblock_keywords')
  } catch {
    customKeywords.value = []
  }
}

async function saveKeywords(list: string[]) {
  customKeywords.value = list
  await invoke('set_adblock_keywords', { keywords: list })
  kwMsg.value = '已保存，刷新页面后生效'
  setTimeout(() => (kwMsg.value = ''), 2500)
}

async function onAddKeyword() {
  const k = newKeyword.value.trim()
  if (!k) return
  if (customKeywords.value.includes(k)) {
    kwMsg.value = '这个词已经加过了'
    setTimeout(() => (kwMsg.value = ''), 2000)
    return
  }
  newKeyword.value = ''
  await saveKeywords([...customKeywords.value, k])
}

async function onRemoveKeyword(k: string) {
  await saveKeywords(customKeywords.value.filter((x) => x !== k))
}
</script>

<template>
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <span>⚙ 设置</span>
        <button class="close-btn" @click="emit('close')">✕</button>
      </div>
      <div class="dialog-body">
        <!-- 通用 -->
        <div class="section">
          <div class="section-title">通用</div>
          <div class="field">
            <label>主页 URL</label>
            <input
              type="text"
              :value="settingsStore.get('homepage')"
              placeholder="https://www.bing.com"
              spellcheck="false"
              @change="onSet('homepage', ($event.target as HTMLInputElement).value.trim())"
            />
          </div>
          <div class="field">
            <label>搜索引擎</label>
            <select
              :value="settingsStore.get('search_engine')"
              @change="onSet('search_engine', ($event.target as HTMLSelectElement).value)"
            >
              <option v-for="e in SEARCH_ENGINES" :key="e.id" :value="e.id">
                {{ e.label }}
              </option>
            </select>
          </div>
        </div>

        <!-- 外观 -->
        <div class="section">
          <div class="section-title">外观</div>
          <div class="field">
            <label>主题</label>
            <select
              :value="settingsStore.get('theme')"
              @change="onSet('theme', ($event.target as HTMLSelectElement).value)"
            >
              <option value="dark">深色</option>
              <option value="light">浅色</option>
            </select>
          </div>
          <div class="field">
            <label>显示书签栏</label>
            <select
              :value="settingsStore.get('show_bookmarks_bar')"
              @change="onSet('show_bookmarks_bar', ($event.target as HTMLSelectElement).value)"
            >
              <option value="true">显示</option>
              <option value="false">隐藏</option>
            </select>
          </div>
        </div>

        <!-- AI 助手 -->
        <div class="section">
          <div class="section-title">AI 助手</div>
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
              :value="settingsStore.get('ai_api_key')"
              placeholder="留空使用环境变量 AGNES_API_KEY"
              spellcheck="false"
              @change="onSet('ai_api_key', ($event.target as HTMLInputElement).value.trim())"
            />
          </div>
        </div>

        <!-- 扩展 -->
        <div class="section">
          <div class="section-title">扩展</div>
          <div class="field ext-load-row">
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
          <div v-else class="ext-empty">还没有扩展</div>
        </div>

        <!-- 隐私 -->
        <div class="section">
          <div class="section-title">隐私</div>
          <div class="field">
            <label>广告拦截</label>
            <select
              :value="settingsStore.get('adblock_enabled') !== 'false' ? 'true' : 'false'"
              @change="onToggleAdblock"
            >
              <option value="true">开启</option>
              <option value="false">关闭</option>
            </select>
          </div>
          <div class="kw-block" v-if="settingsStore.get('adblock_enabled') !== 'false'">
            <div class="kw-hint">内置已含「关闭广告 / 头条报 / 推广」等。下面可手工加词，刷新页面后生效。</div>
            <div class="kw-list" v-if="customKeywords.length > 0">
              <span v-for="k in customKeywords" :key="k" class="kw-chip">
                {{ k }}
                <button class="kw-x" title="删除" @click="onRemoveKeyword(k)">✕</button>
              </span>
            </div>
            <div class="kw-add">
              <input
                v-model="newKeyword"
                type="text"
                placeholder="输入关键词，回车添加"
                spellcheck="false"
                @keyup.enter="onAddKeyword"
              />
              <button class="mini-btn primary" @click="onAddKeyword">添加</button>
            </div>
            <div v-if="kwMsg" class="cleared-msg">{{ kwMsg }}</div>
          </div>
          <div class="field">
            <label>记录浏览历史</label>
            <select
              :value="settingsStore.get('record_history')"
              @change="onSet('record_history', ($event.target as HTMLSelectElement).value)"
            >
              <option value="true">记录</option>
              <option value="false">不记录</option>
            </select>
          </div>
          <div class="field">
            <label>历史记录</label>
            <div class="history-actions">
              <button class="danger-btn" @click="onClearHistory">清除历史数据</button>
              <span v-if="clearedMsg" class="cleared-msg">{{ clearedMsg }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.dialog {
  width: 480px;
  max-height: 80vh;
  background: #222;
  border-radius: 10px;
  border: 1px solid #444;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  color: #e0e0e0;
  font-size: 14px;
  border-bottom: 1px solid #333;
}
.close-btn {
  border: none;
  background: transparent;
  color: #999;
  font-size: 14px;
  cursor: pointer;
}
.dialog-body {
  padding: 12px 16px;
  overflow-y: auto;
}
.section {
  margin-bottom: 14px;
}
.section-title {
  font-size: 12px;
  color: #4a9eff;
  font-weight: 600;
  margin-bottom: 8px;
  border-bottom: 1px solid #333;
  padding-bottom: 4px;
}
.field {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}
.field label {
  font-size: 13px;
  color: #ccc;
  flex-shrink: 0;
}
.field input,
.field select {
  flex: 1;
  max-width: 260px;
  border: 1px solid #444;
  background: #2a2a2a;
  color: #e0e0e0;
  border-radius: 6px;
  padding: 6px 10px;
  font-size: 13px;
  outline: none;
  /* WebKitGTK 下 select/input 默认用系统白色渲染，强制自定义样式 */
  -webkit-appearance: none;
  appearance: none;
}
.field input:focus,
.field select:focus {
  border-color: #4a9eff;
}
.history-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.danger-btn {
  background: #3d3d3d;
  color: #e53935;
  border: 1px solid #5a2a2a;
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
}
.danger-btn:hover {
  background: #4a3a3a;
}
.cleared-msg {
  font-size: 12px;
  color: #43a047;
}
.ext-load-row {
  align-items: stretch;
}
.ext-load-row input {
  flex: 1;
  max-width: none;
}
.ext-error {
  color: #ff6b6b;
  font-size: 12px;
  margin-bottom: 6px;
}
.ext-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 4px;
}
.ext-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #2a2a2a;
  border-radius: 8px;
  padding: 8px 10px;
  gap: 8px;
}
.ext-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.ext-name {
  color: #e0e0e0;
  font-size: 13px;
  font-weight: 500;
}
.ext-desc {
  color: #888;
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ext-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
}
.ext-badge {
  font-size: 10px;
  color: #4a9eff;
  background: rgba(74, 158, 255, 0.15);
  padding: 2px 6px;
  border-radius: 4px;
}
.ext-empty {
  color: #666;
  font-size: 12px;
  text-align: center;
  padding: 8px 0;
}
.mini-btn {
  background: #3d3d3d;
  color: #ccc;
  border: none;
  border-radius: 4px;
  padding: 5px 10px;
  font-size: 12px;
  cursor: pointer;
}
.mini-btn:hover {
  background: #4a4a4a;
}
.mini-btn.primary {
  background: #2b5a8a;
  color: #cfe4ff;
}
.mini-btn.primary:hover {
  background: #3370ab;
}
.mini-btn.danger:hover {
  background: #5c2b2b;
  color: #ff8a8a;
}
.switch {
  position: relative;
  display: inline-block;
  width: 34px;
  height: 18px;
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
  background: #3d3d3d;
  border-radius: 9px;
  transition: 0.2s;
}
.slider::before {
  content: '';
  position: absolute;
  width: 12px;
  height: 12px;
  left: 3px;
  top: 3px;
  background: #999;
  border-radius: 50%;
  transition: 0.2s;
}
.switch input:checked + .slider {
  background: rgba(74, 158, 255, 0.4);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: #4a9eff;
}
.kw-block {
  margin: 4px 0 10px 0;
  padding: 8px 10px;
  background: #2a2a2a;
  border-radius: 8px;
}
.kw-hint {
  font-size: 11px;
  color: #888;
  margin-bottom: 8px;
  line-height: 1.4;
}
.kw-list {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}
.kw-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: rgba(74, 158, 255, 0.15);
  color: #cfe4ff;
  border-radius: 12px;
  padding: 3px 8px;
  font-size: 12px;
}
.kw-x {
  border: none;
  background: transparent;
  color: #999;
  cursor: pointer;
  font-size: 11px;
  padding: 0;
  line-height: 1;
}
.kw-x:hover {
  color: #ff8a8a;
}
.kw-add {
  display: flex;
  gap: 8px;
}
.kw-add input {
  flex: 1;
  border: 1px solid #444;
  background: #222;
  color: #e0e0e0;
  border-radius: 6px;
  padding: 6px 10px;
  font-size: 13px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.kw-add input:focus {
  border-color: #4a9eff;
}
</style>

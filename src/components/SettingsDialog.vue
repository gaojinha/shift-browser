<script setup lang="ts">
// 设置弹窗（通用 / 外观 / 隐私）
// 设置项存 settings 表（key-value），保存即生效
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../stores/settings'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const settingsStore = useSettingsStore()

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
watch(
  () => props.show,
  async (v) => {
    if (v) {
      if (!settingsStore.loaded) settingsStore.loadSettings().catch(() => {})
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

async function onSet(key: string, value: string) {
  await settingsStore.set(key, value)
}

async function onClearHistory() {
  await settingsStore.clearHistory()
  clearedMsg.value = '历史记录已清除'
  setTimeout(() => (clearedMsg.value = ''), 2000)
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

        <!-- 隐私 -->
        <div class="section">
          <div class="section-title">隐私</div>
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
</style>
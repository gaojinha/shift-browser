<script setup lang="ts">
// 设置弹窗 · 隐私与安全分区的两张卡（IA 重组：原 DataSection「数据管理」）
// 身份与传输（User-Agent / 证书中心）+ 历史与清理（记录开关 / 清历史 / 清除浏览数据）
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'
import SettingSwitch from './ui/SettingSwitch.vue'
import './ui/settings-shared.css'

const emit = defineEmits<{ (e: 'openCerts'): void }>()

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

const clearedMsg = ref('')
// User-Agent 管理（方案 B · 2026-09-22）
const uaConfig = ref<{ global: string; default: string; overrides: Record<string, string> }>({ global: '', default: '', overrides: {} })
const uaInput = ref('')
const uaSiteHost = ref('')
const uaSiteValue = ref('')
const uaMsg = ref('')
async function loadUa() {
  try {
    uaConfig.value = await invoke('get_user_agent_config')
    uaInput.value = uaConfig.value.global
  } catch (e) {
    console.warn('[ua] 读取失败:', e)
  }
}
async function saveUa() {
  try {
    await invoke('set_user_agent', { ua: uaInput.value })
    await loadUa()
    uaMsg.value = '已保存（新标签生效）'
    useToastStore().show('User-Agent 已更新', 'success')
  } catch (e) {
    uaMsg.value = '保存失败：' + String(e)
  }
}
async function resetUa() {
  uaInput.value = uaConfig.value.default
  await saveUa()
}
/** 三预设：点名要系统/端别的站，按顺序试，不用手写 */
function applyUaPreset(k: string) {
  if (k === 'windows') {
    uaInput.value =
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36'
  } else if (k === 'mobile') {
    uaInput.value =
      'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36'
  } else {
    uaInput.value = uaConfig.value.default
  }
}
async function saveUaOverride() {
  if (!uaSiteHost.value.trim()) {
    uaMsg.value = '请填站点域名'
    return
  }
  try {
    await invoke('set_user_agent_override', { host: uaSiteHost.value, ua: uaSiteValue.value })
    uaSiteHost.value = ''
    uaSiteValue.value = ''
    await loadUa()
    uaMsg.value = '站点覆盖已保存'
  } catch (e) {
    uaMsg.value = '保存失败：' + String(e)
  }
}
async function delUaOverride(h: string) {
  try {
    await invoke('set_user_agent_override', { host: h, ua: '' })
    await loadUa()
  } catch (e) {
    uaMsg.value = '删除失败：' + String(e)
  }
}
// 一键清除浏览数据（隐私 · 2026-09-22）
const clearScopes = ref({ history: true, cookies: true, cache: true, storage: true, downloads: false, sessions: false })
const clearMsg = ref('')
async function onClearBrowsingData() {
  const scopes = Object.entries(clearScopes.value)
    .filter(([, v]) => v)
    .map(([k]) => k)
  if (scopes.length === 0) {
    clearMsg.value = '请至少选择一项'
    return
  }
  const zh: Record<string, string> = { history: '历史', cookies: 'Cookie', cache: '缓存', storage: '本地存储', downloads: '下载记录', sessions: '会话' }
  clearMsg.value = '清除中…'
  try {
    const done = await invoke<string[]>('clear_browsing_data', { scopes })
    clearMsg.value = '已清除：' + done.map((d) => zh[d] || d).join('、')
    useToastStore().show('已清除浏览数据', 'success')
  } catch (e) {
    clearMsg.value = '清除失败：' + String(e)
  }
}

async function onClearHistory() {
  await settingsStore.clearHistory()
  clearedMsg.value = '历史记录已清除'
  setTimeout(() => (clearedMsg.value = ''), 2000)
}

onMounted(loadUa)
</script>

<template>
  <!-- 身份与传输 -->
  <div class="card" data-anchor="identity">
    <div class="card-title"><AppIcon name="globe" :size="14" /> 身份与传输</div>
    <div class="field">
      <label>证书管理</label>
      <div class="history-actions">
        <button class="mini-btn" @click="emit('openCerts')">打开证书中心</button>
        <span class="field-desc">查看/管理证书、例外与监测事件</span>
      </div>
    </div>
    <div class="field ua-field">
      <label>User-Agent</label>
      <div class="ua-body">
        <div class="field-desc">
          部分网站按 UA 版本判定「浏览器不受支持」，可在此升级/自定义（也是一种身份伪装手段）
        </div>
        <div class="ua-presets">
          <button class="mini-btn" @click="applyUaPreset('default')">默认</button>
          <button class="mini-btn" @click="applyUaPreset('windows')">Windows 版</button>
          <button class="mini-btn" @click="applyUaPreset('mobile')">移动版</button>
          <span class="field-desc">不会手写就点预设，再按保存（清空即回默认）</span>
        </div>
        <input class="ua-input" v-model="uaInput" placeholder="留空 = 使用默认 UA" />
        <div class="history-actions">
          <button class="mini-btn" @click="saveUa">保存</button>
          <button class="mini-btn" @click="resetUa">恢复默认</button>
          <span v-if="uaMsg" class="cleared-msg">{{ uaMsg }}</span>
        </div>
        <div class="field-desc">站点级覆盖（某些站需特定 UA）</div>
        <div class="ua-override-row">
          <input class="ua-input sm" v-model="uaSiteHost" placeholder="域名，如 example.com" />
          <input class="ua-input sm" v-model="uaSiteValue" placeholder="该站 UA" />
          <button class="mini-btn" @click="saveUaOverride">添加</button>
        </div>
        <div v-if="Object.keys(uaConfig.overrides).length" class="ua-overrides">
          <div v-for="(_, h) in uaConfig.overrides" :key="h" class="ua-ov-item">
            <span class="mono">{{ h }}</span>
            <button class="btn-x" @click="delUaOverride(h)">删除</button>
          </div>
        </div>
      </div>
    </div>
  </div>

  <!-- 历史与清理 -->
  <div class="card" data-anchor="history-cleanup">
    <div class="card-title"><AppIcon name="trash" :size="14" /> 历史与清理</div>
    <div class="field">
      <label>记录浏览历史</label>
      <SettingSwitch
        :checked="settingsStore.get('record_history') !== 'false'"
        label="记录浏览历史"
        @change="(v) => onSet('record_history', v ? 'true' : 'false')"
      />
    </div>
    <div class="field danger-field">
      <label>历史记录</label>
      <div class="history-actions">
        <button class="danger-btn" @click="onClearHistory">清除历史数据</button>
        <span v-if="clearedMsg" class="cleared-msg">{{ clearedMsg }}</span>
      </div>
    </div>
    <div class="field danger-field clear-data-field">
      <label>清除浏览数据</label>
      <div class="clear-scopes">
        <label><input type="checkbox" v-model="clearScopes.history" /> 浏览历史</label>
        <label><input type="checkbox" v-model="clearScopes.cookies" /> Cookie</label>
        <label><input type="checkbox" v-model="clearScopes.cache" /> 缓存</label>
        <label><input type="checkbox" v-model="clearScopes.storage" /> 本地存储</label>
        <label><input type="checkbox" v-model="clearScopes.downloads" /> 下载记录</label>
        <label><input type="checkbox" v-model="clearScopes.sessions" /> 会话</label>
      </div>
      <div class="history-actions">
        <button class="danger-btn" @click="onClearBrowsingData">立即清除</button>
        <span v-if="clearMsg" class="cleared-msg">{{ clearMsg }}</span>
      </div>
    </div>
    <div class="kw-hint">
      定时自动清理在「扩展与自动化 → 定时任务」中配置。
    </div>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>

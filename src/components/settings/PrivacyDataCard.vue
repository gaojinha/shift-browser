<script setup lang="ts">
// 设置弹窗 · 隐私与安全分区的两张卡（IA 重组：原 DataSection「数据管理」）
// 身份与传输（User-Agent / 证书中心）+ 历史与清理（记录开关 / 清历史 / 清除浏览数据）
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'

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
      <label class="switch">
        <input
          type="checkbox"
          :checked="settingsStore.get('record_history') !== 'false'"
          @change="onSet('record_history', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
        />
        <span class="slider"></span>
      </label>
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
.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.switch { position: relative; display: inline-block; width: 36px; height: 20px; flex-shrink: 0; }.switch input { opacity: 0; width: 0; height: 0; }.slider { position: absolute; cursor: pointer; inset: 0; background: var(--bg-active); border-radius: 10px; transition: 0.2s; }.slider::before { content: ''; position: absolute; width: 14px; height: 14px; left: 3px; top: 3px; background: var(--text-muted); border-radius: 50%; transition: 0.2s; }.switch input:checked + .slider { background: var(--accent); }.switch input:checked + .slider::before { transform: translateX(16px); background: #fff; }.kw-hint { font-size: 11px; color: var(--text-muted); margin-bottom: 8px; line-height: 1.4; }.danger-field { margin-top: 14px; padding-top: 12px; border-top: 1px dashed var(--border-subtle); }.danger-btn { background: rgba(229, 57, 53, 0.12); color: #ff6b6b; border: 1px solid rgba(229, 57, 53, 0.35); border-radius: 7px; padding: 6px 14px; font-size: 12px; cursor: pointer; transition: all 0.15s; }.danger-btn:hover { background: rgba(229, 57, 53, 0.22); }.cleared-msg { font-size: 12px; color: #43a047; }.clear-data-field { flex-direction: column; align-items: stretch; gap: 4px; }.clear-data-field .clear-scopes { margin: 2px 0 4px; }.clear-scopes { display: flex; flex-wrap: wrap; gap: 10px 16px; margin: 6px 0 8px; font-size: 12px; color: var(--text-secondary); }.clear-scopes label { display: flex; align-items: center; gap: 5px; cursor: pointer; }.ua-field { align-items: flex-start; }.ua-body { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 6px; }.ua-input { width: 100%; box-sizing: border-box; padding: 7px 10px; background: var(--bg-primary); border: 1px solid var(--border-color); border-radius: 8px; color: var(--text-primary); font-size: 12px; font-family: ui-monospace, monospace; outline: none; }.ua-input:focus { border-color: var(--accent); }.ua-input.sm { width: auto; flex: 1; }.ua-override-row { display: flex; gap: 6px; align-items: center; }.ua-overrides { margin-top: 8px; display: flex; flex-direction: column; gap: 4px; }.ua-ov-item { display: flex; align-items: center; justify-content: space-between; padding: 5px 8px; background: var(--bg-hover); border-radius: 6px; font-size: 12px; }.mono { font-family: ui-monospace, monospace; }.btn-x { border: none; background: transparent; color: var(--text-muted); cursor: pointer; font-size: 11px; padding: 0 4px; line-height: 1; }.btn-x:hover { color: #ff8a8a; }.history-actions { display: flex; align-items: center; gap: 8px; }.mini-btn { background: var(--bg-active); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 7px; padding: 5px 12px; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 0.15s; white-space: nowrap; }.mini-btn:hover { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }.mini-btn:disabled { opacity: 0.45; cursor: default; }
</style>

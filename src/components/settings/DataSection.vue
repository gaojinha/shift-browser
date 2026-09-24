<script setup lang="ts">
// 设置弹窗 · 数据安全分区（R2-1d 拆分：原 SettingsDialog.vue 内联节抽出）
// 密码库主密码 / 上云 E2E 加密同步 / 启动锁屏 / 数据管理（历史、清除、证书、UA）
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

/** 密码库主密码（阶段 2 · 数据主权底线） */
interface VaultStatus {
  protected: boolean
  unlocked: boolean
  idle_lock_minutes: number
}
const vaultStatus = ref<VaultStatus | null>(null)
const vaultPassword = ref('')
const vaultBusy = ref(false)
/** 空闲自动锁定分钟数（0 = 关闭，上限 1440） */
const vaultIdleMinutes = ref<number>(15)
async function loadVaultStatus() {
  try {
    vaultStatus.value = await invoke<VaultStatus>('vault_status')
    vaultIdleMinutes.value = vaultStatus.value.idle_lock_minutes
  } catch (e) {
    console.error('[settings] 读取密码库状态失败:', e)
  }
}
async function saveVaultIdleLock() {
  const mins = Number(vaultIdleMinutes.value)
  if (!Number.isFinite(mins) || mins < 0 || mins > 1440) {
    useToastStore().error('空闲锁定需为 0–1440 分钟（0 = 关闭）')
    return
  }
  try {
    await invoke('set_vault_idle_lock', { minutes: Math.floor(mins) })
    await loadVaultStatus()
    useToastStore().success(mins === 0 ? '空闲自动锁定已关闭' : `空闲 ${mins} 分钟后自动锁定`)
  } catch (e) {
    console.error('[settings] 保存空闲锁定设置失败:', e)
    useToastStore().error('保存失败：' + e)
  }
}
async function setVaultPassword() {
  if (vaultPassword.value.length < 8) {
    useToastStore().error('主密码至少 8 位')
    return
  }
  vaultBusy.value = true
  try {
    await invoke('set_vault_password', { password: vaultPassword.value })
    vaultPassword.value = ''
    await loadVaultStatus()
    useToastStore().success('主密码已设置，密码库进入保护状态')
  } catch (e) {
    console.error('[settings] 设置主密码失败:', e)
    useToastStore().error('设置失败：' + e)
  } finally {
    vaultBusy.value = false
  }
}
async function unlockVault() {
  vaultBusy.value = true
  try {
    await invoke('unlock_vault', { password: vaultPassword.value })
    vaultPassword.value = ''
    await loadVaultStatus()
    useToastStore().success('密码库已解锁')
  } catch (e) {
    console.error('[settings] 解锁失败:', e)
    useToastStore().error('解锁失败：' + e)
  } finally {
    vaultBusy.value = false
  }
}
async function lockVault() {
  try {
    await invoke('lock_vault')
    vaultPassword.value = ''
    await loadVaultStatus()
    useToastStore().success('密码库已锁定')
  } catch (e) {
    console.error('[settings] 锁定失败:', e)
  }
}
async function removeVaultPassword() {
  vaultBusy.value = true
  try {
    await invoke('remove_vault_password', { password: vaultPassword.value })
    vaultPassword.value = ''
    await loadVaultStatus()
    useToastStore().success('主密码已移除（回到密钥文件模式）')
  } catch (e) {
    console.error('[settings] 移除主密码失败:', e)
    useToastStore().error('移除失败：' + e)
  } finally {
    vaultBusy.value = false
  }
}

/** 上云 E2E 加密同步（第 45 章 · 服务器零知识） */
interface SyncStatus {
  vault_protected: boolean
  unlocked: boolean
  configured: boolean
  ready: boolean
  host: string | null
  last_push_at: string | null
  last_pull_at: string | null
}
const syncStatus = ref<SyncStatus | null>(null)
const syncUrl = ref('')
const syncUser = ref('')
const syncPass = ref('')
const syncBusy = ref(false)
async function loadSyncStatus() {
  try {
    syncStatus.value = await invoke<SyncStatus>('sync_status')
    if (syncStatus.value.host && !syncUrl.value) {
      syncUrl.value = 'https://' + syncStatus.value.host
    }
  } catch (e) {
    console.error('[settings] 读取同步状态失败:', e)
  }
}
async function saveSyncConfig() {
  if (!syncUrl.value.trim() || !syncUser.value.trim()) {
    useToastStore().error('请填写 WebDAV 地址与用户名')
    return
  }
  syncBusy.value = true
  try {
    await invoke('sync_configure', {
      url: syncUrl.value,
      username: syncUser.value,
      password: syncPass.value,
    })
    syncPass.value = ''
    await loadSyncStatus()
    useToastStore().success('WebDAV 已配置（凭据已用主密码加密落盘）')
  } catch (e) {
    console.error('[settings] 配置同步失败:', e)
    useToastStore().error('配置失败：' + e)
  } finally {
    syncBusy.value = false
  }
}
async function pushSync() {
  syncBusy.value = true
  try {
    const msg = await invoke<string>('sync_push')
    await loadSyncStatus()
    useToastStore().success(msg)
  } catch (e) {
    console.error('[settings] 推送失败:', e)
    useToastStore().error('推送失败：' + e)
  } finally {
    syncBusy.value = false
  }
}
async function pullSync() {
  syncBusy.value = true
  try {
    const msg = await invoke<string>('sync_pull')
    await loadSyncStatus()
    useToastStore().success(msg)
  } catch (e) {
    console.error('[settings] 拉取失败:', e)
    useToastStore().error('拉取失败：' + e)
  } finally {
    syncBusy.value = false
  }
}
async function clearSync() {
  try {
    await invoke('sync_clear')
    await loadSyncStatus()
    useToastStore().success('已清除本地同步配置（云端包保留）')
  } catch (e) {
    console.error('[settings] 清除同步配置失败:', e)
  }
}

onMounted(() => {
  loadVaultStatus()
  loadSyncStatus()
  loadUa()
})
</script>

<template>
  <div class="settings-scroll">
    <!-- ③ 数据安全 -->
    <div class="card">
      <div class="card-title"><AppIcon name="lock" :size="14" /> 数据安全</div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>密码库主密码</b>：设置后密码库启动即<strong>锁定</strong>，须输入主密码解锁
          ——同机拿到密钥文件也无法解密（数据主权底线）。
          <span v-if="vaultStatus">
            当前：<b>{{
              vaultStatus.protected
                ? vaultStatus.unlocked
                  ? '已设主密码 · 已解锁'
                  : '已设主密码 · 已锁定'
                : '未设主密码（仅密钥文件保护）'
            }}</b>
          </span>
        </div>
        <div class="kw-add">
          <input
            v-model="vaultPassword"
            type="password"
            :placeholder="
              vaultStatus?.protected ? '输入主密码' : '设置主密码（至少 8 位）'
            "
            spellcheck="false"
          />
          <button
            v-if="!vaultStatus?.protected"
            class="mini-btn primary"
            :disabled="vaultBusy"
            @click="setVaultPassword"
          >
            设置主密码
          </button>
          <template v-else>
            <button
              v-if="!vaultStatus.unlocked"
              class="mini-btn primary"
              :disabled="vaultBusy"
              @click="unlockVault"
            >
              解锁
            </button>
            <button v-else class="mini-btn" @click="lockVault">锁定</button>
            <button
              class="mini-btn danger"
              :disabled="vaultBusy"
              @click="removeVaultPassword"
            >
              移除主密码
            </button>
          </template>
        </div>
        <div v-if="vaultStatus?.protected" class="kw-add" style="margin-top: 8px">
          <span class="kw-hint" style="margin-right: 8px">
            空闲自动锁定（无操作 N 分钟后清内存主密码，0 = 关闭）：
          </span>
          <input
            v-model.number="vaultIdleMinutes"
            type="number"
            min="0"
            max="1440"
            style="width: 80px"
          />
          <span style="margin: 0 8px">分钟</span>
          <button class="mini-btn primary" @click="saveVaultIdleLock">保存</button>
        </div>
      </div>
      <div class="kw-block">
        <div class="kw-hint">
          <b>上云 E2E 加密同步（服务器零知识）</b>：把<strong>密码库 + 书签 + 会话 + 历史</strong>
          加密推送到自建 Nextcloud（WebDAV）。同步包用<strong>主密码</strong>整包加密，服务器只存密文，
          没主密码谁也解不开。须先设主密码并解锁。
          <span v-if="syncStatus">
            当前：<b>{{
              !syncStatus.vault_protected
                ? '未设主密码（禁止上云）'
                : !syncStatus.unlocked
                  ? '已锁定（请先解锁）'
                  : syncStatus.configured
                    ? '就绪 · ' + (syncStatus.host || '')
                    : '待配置 WebDAV'
            }}</b>
          </span>
        </div>
        <div class="kw-add sync-config">
          <input
            v-model="syncUrl"
            placeholder="WebDAV 地址，如 https://host/remote.php/dav/files/user"
            spellcheck="false"
          />
          <input
            v-model="syncUser"
            placeholder="用户名"
            spellcheck="false"
          />
          <input
            v-model="syncPass"
            type="password"
            placeholder="密码"
            spellcheck="false"
          />
          <button
            class="mini-btn"
            :disabled="syncBusy || !syncStatus?.vault_protected || !syncStatus?.unlocked"
            @click="saveSyncConfig"
          >
            保存配置
          </button>
        </div>
        <div class="btn-row">
          <button
            class="mini-btn primary"
            :disabled="syncBusy || !syncStatus?.ready"
            @click="pushSync"
          >
            ⬆ 推送到云
          </button>
          <button
            class="mini-btn"
            :disabled="syncBusy || !syncStatus?.ready"
            @click="pullSync"
          >
            ⬇ 从云合并
          </button>
          <button
            v-if="syncStatus?.configured"
            class="mini-btn danger"
            :disabled="syncBusy"
            @click="clearSync"
          >
            清除配置
          </button>
          <span v-if="syncStatus?.last_push_at" class="sync-meta">
            上次推送：{{ syncStatus.last_push_at }}
          </span>
          <span v-if="syncStatus?.last_pull_at" class="sync-meta">
            上次拉取：{{ syncStatus.last_pull_at }}
          </span>
        </div>
      </div>
      <div class="field">
        <label>启动锁屏（每次启动输入账号密码解锁）</label>
        <label class="switch">
          <input
            type="checkbox"
            :checked="settingsStore.get('lock_enabled') === 'true'"
            @change="onSet('lock_enabled', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
          />
          <span class="slider"></span>
        </label>
      </div>
    </div>

    <!-- ④ 数据管理 -->
    <div class="card">
      <div class="card-title"><AppIcon name="trash" :size="14" /> 数据管理</div>
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
            部分网站按 UA 版本判定「浏览器不受支持」，可在此升级/自定义
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
.field label {
  font-size: 13px;
  color: var(--text-secondary);
  flex-shrink: 0;
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
.slider::before {
  content: '';
  position: absolute;
  width: 14px;
  height: 14px;
  left: 3px;
  top: 3px;
  background: var(--text-muted);
  border-radius: 50%;
  transition: 0.2s;
}
.switch input:checked + .slider {
  background: var(--accent);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: #fff;
}
.kw-block {
  margin: 4px 0 12px 0;
  padding: 10px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}
.kw-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 8px;
  line-height: 1.4;
}
.kw-add {
  display: flex;
  gap: 8px;
}
.kw-add input {
  flex: 1;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 6px 10px;
  font-size: 13px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.kw-add input:focus {
  border-color: var(--accent);
}
.btn-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}
.sync-config {
  flex-wrap: wrap;
  margin-top: 6px;
}
.sync-config input {
  min-width: 180px;
}
.sync-meta {
  font-size: 11px;
  color: var(--text-muted);
}
.history-actions {
  display: flex;
  align-items: center;
  gap: 8px;
}
.danger-field {
  margin-top: 14px;
  padding-top: 12px;
  border-top: 1px dashed var(--border-subtle);
}
.danger-btn {
  background: rgba(229, 57, 53, 0.12);
  color: #ff6b6b;
  border: 1px solid rgba(229, 57, 53, 0.35);
  border-radius: 7px;
  padding: 6px 14px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.danger-btn:hover {
  background: rgba(229, 57, 53, 0.22);
}
.cleared-msg {
  font-size: 12px;
  color: #43a047;
}
.clear-data-field {
  flex-direction: column;
  align-items: stretch;
  gap: 4px;
}
.clear-data-field .clear-scopes {
  margin: 2px 0 4px;
}
.clear-scopes {
  display: flex;
  flex-wrap: wrap;
  gap: 10px 16px;
  margin: 6px 0 8px;
  font-size: 12px;
  color: #bbb;
}
.clear-scopes label {
  display: flex;
  align-items: center;
  gap: 5px;
  cursor: pointer;
}
.ua-field {
  align-items: flex-start;
}
.ua-body {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.ua-input {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 10px;
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid rgba(255, 255, 255, 0.14);
  border-radius: 8px;
  color: #ddd;
  font-size: 12px;
  font-family: ui-monospace, monospace;
}
.ua-input.sm {
  width: auto;
  flex: 1;
}
.ua-override-row {
  display: flex;
  gap: 6px;
  align-items: center;
}
.ua-overrides {
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ua-ov-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 5px 8px;
  background: rgba(255, 255, 255, 0.04);
  border-radius: 6px;
  font-size: 12px;
}
.mono {
  font-family: ui-monospace, monospace;
}
.btn-x {
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11px;
  padding: 0 4px;
  line-height: 1;
}
.btn-x:hover {
  color: #ff8a8a;
}
.mini-btn {
  background: var(--bg-active);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 7px;
  padding: 5px 12px;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}
.mini-btn:hover {
  background: var(--accent-soft);
  color: var(--accent);
  border-color: var(--accent);
}
.mini-btn:disabled {
  opacity: 0.45;
  cursor: default;
}
.mini-btn.primary {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}
.mini-btn.primary:hover {
  background: var(--accent-2);
  color: #fff;
  border-color: var(--accent-2);
}
.mini-btn.danger {
  color: #ff8a8a;
  background: rgba(229, 57, 53, 0.16);
  border-color: rgba(229, 57, 53, 0.45);
}
.mini-btn.danger:hover {
  background: rgba(229, 57, 53, 0.34);
  color: #ffb3b3;
  border-color: rgba(229, 57, 53, 0.6);
}
</style>

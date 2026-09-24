<script setup lang="ts">
// 设置弹窗（通用 / 外观 / AI 助手 / 隐私 / 扩展）
// 设置项存 settings 表（key-value），保存即生效
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useSettingsSet } from '../composables/useSettingsSet'
import { useExtensionsStore } from '../stores/extensions'
import { useToastStore } from '../stores/toast'
import SelfCheckSection from './settings/SelfCheckSection.vue'
import AboutSection from './settings/AboutSection.vue'
import AiSection from './settings/AiSection.vue'
import ExtensionsSection from './settings/ExtensionsSection.vue'
import ApiSection from './settings/ApiSection.vue'
import McpSection from './settings/McpSection.vue'
import AppearanceSection from './settings/AppearanceSection.vue'
import GeneralSection from './settings/GeneralSection.vue'
// 搜索引擎清单（单一来源，与地址栏/新标签页/开放 API 同源）

const props = defineProps<{ show: boolean; initialSection?: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'openCerts'): void }>()

const settingsStore = useSettingsStore()
const extensionsStore = useExtensionsStore()

// 侧边导航：当前激活区块
const activeSection = ref('general')
// 外部指定初始选项卡（状态栏隐私战绩点击 → 跳隐私防护；打开后消费一次并复位）
watch(
  () => props.show,
  (v) => {
    if (v && props.initialSection) {
      activeSection.value = props.initialSection
    }
  },
)
const SECTIONS = [
  { id: 'general', icon: 'house', label: '通用' },
  { id: 'appearance', icon: 'palette', label: '外观' },
  { id: 'ai', icon: 'sparkles', label: 'AI 助手' },
  { id: 'mcp', icon: 'plug', label: 'MCP 工具' },
  { id: 'extensions', icon: 'puzzle', label: '扩展' },
  { id: 'privacy', icon: 'shield', label: '隐私防护' },
  { id: 'data', icon: 'lock', label: '数据安全' },
  { id: 'api', icon: 'plug', label: '开放 API' },
  { id: 'selfcheck', icon: 'activity', label: '自检' },
  { id: 'about', icon: 'info', label: '关于' },
]

// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        if (!settingsStore.loaded) {
          settingsStore.loadSettings().catch((e) => {
            console.error('[settings] 加载设置失败:', e)
            useToastStore().error('加载设置失败')
          })
        }
        extensionsStore.loadExtensions().catch((e) => {
          console.error('[settings] 加载扩展失败:', e)
          useToastStore().error('加载扩展失败')
        })
        loadKeywords().catch((e) => {
          console.error('[settings] 加载搜索引擎失败:', e)
          useToastStore().error('加载搜索引擎失败')
        })
        // 加噪白名单（隐私分区用）
        loadWhitelist()
        // 隐私防护统计
        loadPrivacyStats()
        // 密码库主密码状态（阶段 2）
        loadVaultStatus()
        // 上云 E2E 加密同步状态（第 45 章）
        loadSyncStatus()
      }
    } catch (e) {
      console.error('[settings] 加载失败:', e)
      useToastStore().error('加载设置失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'settings-dialog')
      }
    }
  },
)

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

// 扩展管理：加载路径输入

const { onSet } = useSettingsSet()

async function onClearHistory() {
  await settingsStore.clearHistory()
  clearedMsg.value = '历史记录已清除'
  setTimeout(() => (clearedMsg.value = ''), 2000)
}

const privacyLevel = computed(() => {
  const l = settingsStore.get('privacy_shield_level')
  if (l === 'off' || l === 'standard' || l === 'strict') return l
  // 兼容旧布尔开关
  return settingsStore.get('privacy_shield') === 'false' ? 'off' : 'standard'
})

async function onChangePrivacyLevel(e: Event) {
  const level = (e.target as HTMLSelectElement).value
  await settingsStore.set('privacy_shield_level', level)
  try {
    await invoke('set_privacy_shield_level', { level })
    const msg =
      level === 'off' ? '隐私防护已关闭' : level === 'strict' ? '已切换到严格档' : '已切换到标准档'
    useToastStore().success(msg)
  } catch (err) {
    console.error('[settings] 隐私防护档位同步失败:', err)
    useToastStore().error('隐私防护设置失败')
  }
}

/** 反指纹模式（阶段 2 反指纹 2.0）：noise 加噪（默认）/ uniform 统一化 */
const fpMode = computed(() => {
  const m = settingsStore.get('privacy_fp_mode')
  return m === 'uniform' ? 'uniform' : 'noise'
})
async function onChangeFpMode(e: Event) {
  const mode = (e.target as HTMLSelectElement).value
  await settingsStore.set('privacy_fp_mode', mode)
  try {
    await invoke('set_privacy_fp_mode', { mode })
    useToastStore().success(
      mode === 'uniform' ? '已切换到统一化（Tor/Mullvad 派）' : '已切换到加噪（推荐）'
    )
  } catch (err) {
    console.error('[settings] 反指纹模式同步失败:', err)
    useToastStore().error('反指纹模式设置失败')
  }
}

/** 加噪白名单：这些站点不做加噪（Canvas 敏感站点可加进来），域名按行分隔 */
const whitelistText = ref('')
async function loadWhitelist() {
  try {
    const list = await invoke<string[]>('get_privacy_whitelist')
    whitelistText.value = list.join('\n')
  } catch (e) {
    console.error('[settings] 读取加噪白名单失败:', e)
  }
}
async function saveWhitelist() {
  const domains = whitelistText.value
    .split(/[\n,，\s]+/)
    .map((s) => s.trim())
    .filter(Boolean)
  try {
    await invoke('set_privacy_whitelist', { domains })
    const list = await invoke<string[]>('get_privacy_whitelist')
    whitelistText.value = list.join('\n')
    useToastStore().success(`白名单已保存（${list.length} 个站点）`)
  } catch (e) {
    console.error('[settings] 保存加噪白名单失败:', e)
    useToastStore().error('白名单保存失败')
  }
}

/** 隐私防护统计（本次运行期间的内存计数，重启归零） */
const privacyStats = ref<{
  cleaned_params: number
  privacy_injections: number
  adblock_injections: number
} | null>(null)
async function loadPrivacyStats() {
  try {
    privacyStats.value = await invoke<{
      cleaned_params: number
      privacy_injections: number
      adblock_injections: number
    }>('get_privacy_stats')
  } catch (e) {
    console.error('[settings] 读取隐私统计失败:', e)
  }
}
async function resetPrivacyStats() {
  try {
    await invoke('reset_privacy_stats')
    await loadPrivacyStats()
    useToastStore().success('统计已重置')
  } catch (e) {
    console.error('[settings] 重置统计失败:', e)
    useToastStore().error('重置统计失败')
  }
}

/** 隐私防护自测（阶段 2：信任需要证明） */
interface PrivacySelfCheck {
  id: string
  label: string
  status: string
  detail: string
}
interface PrivacySelfTestReport {
  shield_level: string
  fp_mode: string
  hostname: string
  whitelisted: boolean
  total: number
  passed: number
  checks: PrivacySelfCheck[]
}
const privacySelfTest = ref<PrivacySelfTestReport | null>(null)
const privacySelfTestRunning = ref(false)
async function runPrivacySelfTest() {
  privacySelfTestRunning.value = true
  try {
    const r = await invoke<PrivacySelfTestReport>('privacy_selftest')
    privacySelfTest.value = r
    if (r.passed === r.total) {
      useToastStore().success(`自测通过：${r.passed}/${r.total} 项防护已生效`)
    } else {
      useToastStore().show(`自测完成：${r.passed}/${r.total} 项通过`)
    }
  } catch (e) {
    console.error('[settings] 隐私自测失败:', e)
    useToastStore().error('自测失败：请先打开一个网页')
  } finally {
    privacySelfTestRunning.value = false
  }
}

/** GPC 站点声明检测（阶段 2：GPC 完整化） */
interface GpcSiteStatus {
  origin: string
  url: string
  reachable: boolean
  declared: boolean | null
  last_update: string | null
  summary: string
  error: string | null
}
const gpcSite = ref<GpcSiteStatus | null>(null)
const gpcChecking = ref(false)
async function checkGpcSite() {
  gpcChecking.value = true
  try {
    gpcSite.value = await invoke<GpcSiteStatus>('gpc_site_check')
  } catch (e) {
    console.error('[settings] GPC 站点检测失败:', e)
    useToastStore().error('检测失败：请先打开一个网页')
  } finally {
    gpcChecking.value = false
  }
}

/** 零遥测审计（阶段 2：全量网络出口清单） */
interface EgressConn {
  pid: number
  proto: string
  remote_ip: string
  remote_port: number
  state: string
}
interface EgressReport {
  processes: number
  connections: EgressConn[]
  distinct_remotes: number
  loopback: number
  external: number
  checked_at: number
  note: string
}
const egressReport = ref<EgressReport | null>(null)
const egressRunning = ref(false)
async function runEgressAudit() {
  egressRunning.value = true
  try {
    egressReport.value = await invoke<EgressReport>('egress_audit')
  } catch (e) {
    console.error('[settings] 外联审计失败:', e)
    useToastStore().error('外联审计失败')
  } finally {
    egressRunning.value = false
  }
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

/** 广告拦截开关：settings 表是唯一状态源，set_adblock_enabled 只负责把开关同步给注入器 */
async function onToggleAdblock() {
  const current = settingsStore.get('adblock_enabled') !== 'false'
  const next = (!current).toString()
  await settingsStore.set('adblock_enabled', next)
  try {
    await invoke('set_adblock_enabled', { enabled: next === 'true' })
  } catch (e) {
    console.error('[settings] 广告拦截同步失败:', e)
    useToastStore().error('广告拦截切换失败')
  }
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
  const prev = customKeywords.value
  customKeywords.value = list
  try {
    await invoke('set_adblock_keywords', { keywords: list })
    kwMsg.value = '已保存，刷新页面后生效'
    setTimeout(() => (kwMsg.value = ''), 2500)
  } catch (e) {
    console.error('[settings] 保存关键词失败:', e)
    // 回滚，避免 UI 与数据库不一致
    customKeywords.value = prev
    useToastStore().error('保存关键词失败')
  }
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
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog settings-dialog">
      <div class="dialog-header">
        <span class="dialog-title"><AppIcon name="gear" :size="15" /> 设置</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
      </div>
      <div class="dialog-body settings-body">
        <!-- 左侧导航 -->
        <div class="settings-nav">
          <div
            v-for="s in SECTIONS"
            :key="s.id"
            class="nav-item"
            :class="{ active: activeSection === s.id }"
            @click="activeSection = s.id"
          >
            <span class="nav-icon"><AppIcon :name="s.icon" :size="16" /></span>
            <span class="nav-label">{{ s.label }}</span>
          </div>
        </div>

        <!-- 右侧内容 -->
        <div class="settings-content">
          <!-- 通用 -->
          <GeneralSection v-if="activeSection === 'general'" />
          <AppearanceSection v-if="activeSection === 'appearance'" />

          <!-- AI 助手 -->
          <AiSection v-if="activeSection === 'ai'" />

          <!-- MCP 工具（阶段 1.3：AI 面板可调用外部工具） -->
          <McpSection v-if="activeSection === 'mcp'" />

          <!-- 扩展 -->
          <ExtensionsSection v-if="activeSection === 'extensions'" />

          <!-- 隐私防护 -->
          <div v-if="activeSection === 'privacy'" class="settings-scroll">
            <!-- ① 浏览防护 -->
            <div class="card">
              <div class="card-title"><AppIcon name="shield" :size="14" /> 浏览防护</div>
              <div class="field">
                <label>隐私防护</label>
                <select class="privacy-select" :value="privacyLevel" @change="onChangePrivacyLevel">
                  <option value="standard">标准（推荐）</option>
                  <option value="strict">严格</option>
                  <option value="off">关闭</option>
                </select>
              </div>
              <div class="privacy-desc">
                <b>标准</b>：Canvas / WebGL / 音频 / WebGPU 反指纹加噪，GPC 隐私信号，WebRTC 内网 IP 防护，
                字体与设备探针收敛，追踪参数清洗，第三方 Cookie 阻断。<br />
                <b>严格</b>：再移除 WebUSB / 蓝牙 / 串口 / HID / Gamepad / 传感器等高危 API，归一化语音音色、
                降级高精度计时（部分网站功能可能受影响）。<br />
                切换后刷新页面即对已开标签生效。
              </div>
              <div class="field">
                <label>反指纹模式</label>
                <select class="privacy-select" :value="fpMode" @change="onChangeFpMode">
                  <option value="noise">加噪（默认）</option>
                  <option value="uniform">统一化（Tor/Mullvad 派）</option>
                </select>
              </div>
              <div class="privacy-desc">
                <b>加噪</b>：Canvas / WebGL / 音频加「同源稳定、跨源不同」的确定性噪声——网站仍能正常用，
                但你的指纹不再稳定可追踪。<br />
                <b>统一化</b>：返回<strong>固定值</strong>，所有使用者呈现一致指纹（更强匿名，但个别依赖
                画布/音频的应用可能受影响）。<br />
                切换后刷新页面即生效。
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>加噪白名单</b>：下列站点<strong>不做加噪</strong>（Canvas 敏感的在线地图、设计工具、
                  视频剪辑等可加进来）。每行一个域名，子域名自动包含。
                </div>
                <textarea
                  class="kw-textarea"
                  v-model="whitelistText"
                  rows="4"
                  placeholder="ditu.amap.com&#10;www.figma.com"
                  spellcheck="false"
                ></textarea>
                <button class="mini-btn primary" @click="saveWhitelist">保存白名单</button>
              </div>
              <div class="stat-row">
                <span class="stat-text">
                  <b>防护统计</b>（本次运行）：清洗追踪参数 <b>{{ privacyStats?.cleaned_params ?? 0 }}</b> 次 ·
                  隐私注入 <b>{{ privacyStats?.privacy_injections ?? 0 }}</b> 次 ·
                  广告拦截 <b>{{ privacyStats?.adblock_injections ?? 0 }}</b> 次
                </span>
                <button class="mini-btn" @click="resetPrivacyStats">重置</button>
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>防护自测</b>：在当前标签页核对各项防护是否<strong>真的生效</strong>（对齐「信任需要证明」）。
                  需先打开一个网页。
                </div>
                <div class="btn-row">
                  <button
                    class="mini-btn primary"
                    :disabled="privacySelfTestRunning"
                    @click="runPrivacySelfTest"
                  >
                    {{ privacySelfTestRunning ? '自测中…' : '运行自测' }}
                  </button>
                </div>
                <div v-if="privacySelfTest" class="test-summary">
                  档位 <b>{{ privacySelfTest.shield_level }}</b> · 反指纹
                  <b>{{ privacySelfTest.fp_mode === 'uniform' ? '统一化' : '加噪' }}</b> · 站点
                  <b>{{ privacySelfTest.hostname || '(未知)' }}</b> · 通过
                  <b>{{ privacySelfTest.passed }}/{{ privacySelfTest.total }}</b>
                  <span v-if="privacySelfTest.whitelisted">（白名单站点，加噪已跳过）</span>
                </div>
                <div v-if="privacySelfTest" class="ps-list">
                  <div
                    v-for="c in privacySelfTest.checks"
                    :key="c.id"
                    class="ps-item"
                    :class="c.status"
                  >
                    <span class="ps-icon">{{
                      c.status === 'pass' ? '✅' : c.status === 'fail' ? '❌' : '➖'
                    }}</span>
                    <span class="ps-label">{{ c.label }}</span>
                    <span class="ps-detail">{{ c.detail }}</span>
                  </div>
                </div>
              </div>
            </div>

            <!-- ② 网络与追踪 -->
            <div class="card">
              <div class="card-title"><AppIcon name="globe" :size="14" /> 网络与追踪</div>
              <div class="field">
                <label>GPC 请求头（Sec-GPC: 1）</label>
                <span class="field-desc">W3C Global Privacy Control：网络层告诉站点「别卖我的数据」</span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('gpc_header') !== 'false'"
                    @change="onSet('gpc_header', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>广告拦截</label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('adblock_enabled') !== 'false'"
                    @change="onToggleAdblock"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="kw-block" v-if="settingsStore.get('adblock_enabled') !== 'false'">
                <div class="kw-hint">
                  内置已含「关闭广告 / 头条报 / 推广」等。下面可手工加词，刷新页面后生效。
                </div>
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
              <div class="kw-block">
                <div class="kw-hint">
                  <b>网络层拦截规则</b>（阶段3.2）：每行一个 URL 子串，命中即<strong>取消该请求</strong>
                  （请求出网前拦，比内容层更彻底）。保存后约 <strong>2 秒自动生效</strong>（热更新）。
                </div>
                <textarea
                  class="net-rules"
                  :value="settingsStore.get('net_block_rules')"
                  placeholder="每行一个 URL 子串，例如：doubleclick.net"
                  spellcheck="false"
                  rows="4"
                  @change="onSet('net_block_rules', ($event.target as HTMLTextAreaElement).value)"
                ></textarea>
              </div>
              <div class="kw-block">
                <div class="kw-hint">
                  <b>自定义请求头</b>（阶段3.2）：每行 <code>Name: Value</code>，注入到每个出网请求
                  （如 <code>DNT: 1</code>、自定义 UA）。保存后约 <strong>2 秒自动生效</strong>。
                  <br />注：<code>Sec-GPC: 1</code> 已默认常开，无需在此重复。
                </div>
                <textarea
                  class="net-rules"
                  :value="settingsStore.get('net_headers')"
                  placeholder="每行 Name: Value，例如：DNT: 1"
                  spellcheck="false"
                  rows="4"
                  @change="onSet('net_headers', ($event.target as HTMLTextAreaElement).value)"
                ></textarea>
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>站点 GPC 声明</b>：抓取本站的 <code>/.well-known/gpc.json</code>，看站点是否声明遵守
                  GPC（需先打开网页）。
                </div>
                <div class="btn-row">
                  <button
                    class="mini-btn primary"
                    :disabled="gpcChecking"
                    @click="checkGpcSite"
                  >
                    {{ gpcChecking ? '检测中…' : '检测本站声明' }}
                  </button>
                </div>
                <div v-if="gpcSite" class="test-summary">
                  <b>{{ gpcSite.origin }}</b> · {{ gpcSite.summary }}
                  <span v-if="gpcSite.last_update">（更新于 {{ gpcSite.last_update }}）</span>
                </div>
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>外联审计</b>：列出本应用及其子进程当前的全部网络出口连接（只读 <code>/proc</code>，
                  零出网）——「可验证无外联」。
                </div>
                <div class="btn-row">
                  <button
                    class="mini-btn primary"
                    :disabled="egressRunning"
                    @click="runEgressAudit"
                  >
                    {{ egressRunning ? '审计中…' : '运行审计' }}
                  </button>
                </div>
                <div v-if="egressReport" class="test-summary">
                  进程 <b>{{ egressReport.processes }}</b> · 连接
                  <b>{{ egressReport.connections.length }}</b> · 外网
                  <b>{{ egressReport.external }}</b> · 本机回环
                  <b>{{ egressReport.loopback }}</b> · 去重端点
                  <b>{{ egressReport.distinct_remotes }}</b>
                </div>
                <div v-if="egressReport && egressReport.connections.length > 0" class="ps-list">
                  <div
                    v-for="(c, i) in egressReport.connections"
                    :key="i"
                    class="ps-item"
                    :class="{ fail: c.remote_ip.indexOf('127.') !== 0 && c.remote_ip !== '::1' }"
                  >
                    <span class="ps-icon">
                      <AppIcon
                        :name="c.remote_ip.indexOf('127.') === 0 || c.remote_ip === '::1' ? 'house' : 'globe'"
                        :size="12"
                      />
                    </span>
                    <span class="ps-label">{{ c.remote_ip }}:{{ c.remote_port }}</span>
                    <span class="ps-detail">{{ c.proto }} · {{ c.state }} · pid {{ c.pid }}</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
          <!-- 数据安全 -->
          <div v-if="activeSection === 'data'" class="settings-scroll">

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
          <!-- 开放 API -->
          <ApiSection v-if="activeSection === 'api'" />
          <!-- 自检 -->
          <SelfCheckSection v-if="activeSection === 'selfcheck'" />
          <AboutSection v-if="activeSection === 'about'" />
        </div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.settings-dialog {
  width: min(860px, calc(100vw - 48px));
  height: min(660px, calc(100vh - 96px));
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-lg);
}
.settings-body {
  display: flex;
  gap: 0;
  padding: 0;
  overflow: hidden;
  flex: 1;
  min-height: 0;
}

/* 左侧导航 */
.settings-nav {
  width: 184px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-subtle);
  padding: 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  background: var(--bg-primary);
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition:
    background var(--dur),
    color var(--dur),
    box-shadow var(--dur);
  user-select: none;
}
.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-item.active {
  background: var(--grad-accent);
  color: #fff;
  font-weight: 600;
  box-shadow: var(--shadow-sm);
}
.nav-icon {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: inherit;
}
.nav-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 右侧内容 */
.settings-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
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

/* 卡片 */
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
.api-actions .selfcheck-btn,
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
.dialog-title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
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
/* 区块提示文字（此前无样式定义，字迹不清晰） */
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
.field input,
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
.field input:focus,
.field select:focus {
  border-color: var(--accent);
}
/* 隐私防护/反指纹下拉框：与 .field select 一致但独立命名，避免样式漂移 */
.privacy-select {
  max-width: 240px;
}

/* 搜索引擎分段按钮 */





/* 主题选择卡片 */








/* 开关 */
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

/* 危险操作 */
.ua-input { width: 100%; box-sizing: border-box; padding: 7px 10px; background: rgba(255,255,255,0.06); border: 1px solid rgba(255,255,255,0.14); border-radius: 8px; color: #ddd; font-size: 12px; font-family: ui-monospace, monospace; }
.ua-input.sm { width: auto; flex: 1; }
.ua-override-row { display: flex; gap: 6px; align-items: center; }
.ua-overrides { margin-top: 8px; display: flex; flex-direction: column; gap: 4px; }
.ua-ov-item { display: flex; align-items: center; justify-content: space-between; padding: 5px 8px; background: rgba(255,255,255,0.04); border-radius: 6px; font-size: 12px; }
.clear-scopes { display: flex; flex-wrap: wrap; gap: 10px 16px; margin: 6px 0 8px; font-size: 12px; color: #bbb; }
.clear-scopes label { display: flex; align-items: center; gap: 5px; cursor: pointer; }
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

/* 隐私分组（浏览防护 / 网络与追踪 / 数据安全 / 数据管理 / 开放 API） */
.privacy-desc {
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-muted);
  margin: -4px 0 12px;
  padding: 8px 10px;
  background: var(--bg-primary);
  border-left: 2px solid var(--accent);
  border-radius: 0 6px 6px 0;
}
.privacy-desc b {
  color: var(--text-secondary);
}
.stat-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  margin: 4px 0 12px;
  padding: 8px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}
.stat-text {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
}
.stat-text b {
  color: var(--accent);
}
.btn-row {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 8px;
}
.test-summary {
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  margin-top: 8px;
  padding: 6px 10px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
}
.test-summary b {
  color: var(--accent);
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
.exp-badge {
  display: inline-block;
  margin-left: 6px;
  padding: 1px 7px;
  font-size: 10px;
  font-weight: 600;
  color: #ffb454;
  background: rgba(255, 180, 84, 0.14);
  border: 1px solid rgba(255, 180, 84, 0.35);
  border-radius: 999px;
  vertical-align: 1px;
}
.webmcp-hint {
  width: 100%;
  margin-top: 8px;
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
.clear-data-field {
  flex-direction: column;
  align-items: stretch;
  gap: 4px;
}
.clear-data-field .clear-scopes {
  margin: 2px 0 4px;
}

/* 扩展 */
.ext-load-row {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.ext-load-row input {
  flex: 1;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 7px 11px;
  font-size: 13px;
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.ext-load-row input:focus {
  border-color: var(--accent);
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
}
.ext-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 8px 10px;
  gap: 8px;
  transition: border-color 0.15s;
}
.ext-item:hover {
  border-color: var(--border-color);
}
/* MCP 工具层（阶段 1.3） */

















.ext-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.ext-name {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
}
.ext-desc {
  color: var(--text-muted);
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
  color: var(--accent);
  background: var(--accent-soft);
  padding: 2px 6px;
  border-radius: 4px;
}
.ext-empty {
  color: var(--text-muted);
  font-size: 12px;
  text-align: center;
  padding: 14px 0;
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

/* 关键词 */
.kw-block {
  margin: 4px 0 12px 0;
  padding: 10px 12px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
}
.net-rules {
  width: 100%;
  box-sizing: border-box;
  min-height: 64px;
  padding: 8px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  font-size: 12px;
  font-family: monospace;
  resize: vertical;
  outline: none;
}
.net-rules:focus {
  border-color: var(--accent);
}
.task-num {
  width: 72px;
  height: 28px;
  padding: 0 8px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  outline: none;
}
.task-num:focus {
  border-color: var(--accent);
}
.kw-hint {
  font-size: 11px;
  color: var(--text-muted);
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
  background: var(--accent-soft);
  color: #cfe4ff;
  border-radius: 12px;
  padding: 3px 8px;
  font-size: 12px;
}
.kw-x {
  border: none;
  background: transparent;
  color: var(--text-muted);
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
.kw-textarea {
  width: 100%;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-primary);
  border-radius: 7px;
  padding: 6px 10px;
  font-size: 12px;
  line-height: 1.5;
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  outline: none;
  resize: vertical;
  margin-bottom: 8px;
  box-sizing: border-box;
}
.kw-textarea:focus {
  border-color: var(--accent);
}
.ps-list {
  margin-top: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ps-item {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  padding: 4px 8px;
  border-radius: 6px;
  background: rgba(127, 127, 127, 0.08);
}
.ps-item.fail {
  background: rgba(220, 60, 60, 0.12);
}
.ps-icon {
  flex: 0 0 auto;
}
.ps-label {
  flex: 0 0 auto;
  font-weight: 600;
}
.ps-detail {
  flex: 1 1 auto;
  opacity: 0.75;
  text-align: right;
}

/* 关于页 */
.about-hero {
  text-align: center;
  padding: 22px 16px 18px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
}
.about-logo {
  width: 56px;
  height: 56px;
  border-radius: 16px;
  background: var(--grad-accent);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  box-shadow: var(--shadow-md);
  margin-bottom: 4px;
  overflow: hidden;
}
.about-logo-img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
}
.about-name {
  font-size: 18px;
  font-weight: 700;
  color: var(--text-primary);
}
.about-version {
  font-size: 13px;
  color: var(--text-secondary);
  display: flex;
  align-items: center;
  gap: 8px;
}
.about-badge {
  font-size: 10px;
  font-weight: 600;
  padding: 1px 8px;
  border-radius: 999px;
  background: var(--accent-soft);
  color: var(--accent);
}
.about-tagline {
  font-size: 13px;
  color: var(--text-muted);
  margin-top: 2px;
}
.about-env {
  display: flex;
  gap: 14px;
  margin-top: 8px;
  flex-wrap: wrap;
  justify-content: center;
}
.about-env-item {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  font-size: 12px;
  color: var(--text-secondary);
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 999px;
  padding: 3px 10px;
}
.about-env-item .app-icon {
  color: var(--accent);
}
.about-feature {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  padding: 7px 0;
  font-size: 13px;
  color: var(--text-primary);
}
.about-feature b {
  font-weight: 600;
}
.about-feature-icon {
  flex: 0 0 auto;
  font-size: 16px;
  line-height: 1.3;
}
.about-feature-desc {
  font-size: 12px;
  color: var(--text-muted);
  margin-top: 2px;
  line-height: 1.5;
}
.about-link-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 0;
  border-bottom: 1px dashed var(--border-subtle);
}
.about-link-label {
  font-size: 13px;
  color: var(--text-primary);
}
</style>

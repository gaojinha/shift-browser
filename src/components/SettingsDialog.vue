<script setup lang="ts">
// 设置弹窗（通用 / 外观 / AI 助手 / 隐私 / 扩展）
// 设置项存 settings 表（key-value），保存即生效
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useExtensionsStore } from '../stores/extensions'
import { useToastStore } from '../stores/toast'
import type { Extension } from '../types/extension'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const settingsStore = useSettingsStore()
const extensionsStore = useExtensionsStore()

// 侧边导航：当前激活区块
const activeSection = ref('general')
const SECTIONS = [
  { id: 'general', icon: '🏠', label: '通用' },
  { id: 'appearance', icon: '🎨', label: '外观' },
  { id: 'ai', icon: '🤖', label: 'AI 助手' },
  { id: 'mcp', icon: '🔌', label: 'MCP 工具' },
  { id: 'extensions', icon: '🧩', label: '扩展' },
  { id: 'privacy', icon: '🛡', label: '隐私' },
  { id: 'selfcheck', icon: '🩺', label: '自检' },
]

// ---- 系统自检（可观测性）：一键汇总版本/存储/运行时/能力状态 ----
interface CheckItem {
  name: string
  ok: boolean
  detail: string
}
interface SelfCheckReport {
  healthy: boolean
  timestamp: number
  versions: CheckItem[]
  storage: CheckItem[]
  runtime: CheckItem[]
  capabilities: CheckItem[]
}
const selfCheckReport = ref<SelfCheckReport | null>(null)
const selfCheckLoading = ref(false)
const selfCheckError = ref('')

async function runSelfCheck() {
  if (selfCheckLoading.value) return
  selfCheckLoading.value = true
  selfCheckError.value = ''
  try {
    selfCheckReport.value = await invoke<SelfCheckReport>('selfcheck_cmd')
  } catch (e) {
    selfCheckError.value = String(e)
  } finally {
    selfCheckLoading.value = false
  }
}

/** 自检项分组（模板里遍历用） */
const selfCheckGroups = computed(() => {
  const r = selfCheckReport.value
  if (!r) return []
  return [
    { title: '版本与环境', items: r.versions },
    { title: '数据存储', items: r.storage },
    { title: '运行时', items: r.runtime },
    { title: '能力状态', items: r.capabilities },
  ]
})

/** 打开自检分区时自动跑一次 */
watch(activeSection, (v) => {
  if (v === 'selfcheck' && !selfCheckReport.value) runSelfCheck()
})

// AI Key 输入框：不回显掩码（get_all_settings 返回前4后4掩码），
// 留空 = 保持原值；输入新值才保存。hasAiKey 判断是否已配置（掩码非空即已配置）
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
watch(
  () => props.show,
  (v) => {
    if (v) loadAiProviders()
  },
)

// ---- MCP 工具层（阶段 1.3）：server 配置 + 工具列表 + 手动调用 + 审计 ----
interface McpServer {
  id: number
  name: string
  command: string
  args: string[]
  enabled: boolean
  allowedTools: string[]
  toolCount: number
}
interface McpTool {
  name: string
  serverName: string
  description: string
  inputSchema: Record<string, unknown>
}
interface AuditEntry {
  id: number
  serverName: string
  toolName: string
  arguments: string
  resultSummary: string
  success: boolean
  createdAt: string
}

const mcpServers = ref<McpServer[]>([])
const mcpTools = ref<McpTool[]>([])
const mcpAudit = ref<AuditEntry[]>([])
const mcpLoading = ref(false)
const mcpError = ref('')
// 新增表单
const mcpNewName = ref('')
const mcpNewCommand = ref('')
const mcpNewArgs = ref('')
const mcpNewAllowed = ref('')
// 手动调用
const mcpCallTool = ref('')
const mcpCallArgs = ref('{}')
const mcpCallResult = ref('')
const mcpCalling = ref(false)

async function loadMcp() {
  mcpLoading.value = true
  mcpError.value = ''
  try {
    mcpServers.value = await invoke<McpServer[]>('mcp_list_servers')
    mcpTools.value = await invoke<McpTool[]>('mcp_list_tools')
    mcpAudit.value = await invoke<AuditEntry[]>('mcp_get_audit_log', { limit: 30 })
  } catch (e) {
    mcpError.value = String(e)
  } finally {
    mcpLoading.value = false
  }
}

async function addMcpServer() {
  mcpError.value = ''
  const name = mcpNewName.value.trim()
  const command = mcpNewCommand.value.trim()
  if (!name || !command) {
    mcpError.value = '名称与命令不能为空'
    return
  }
  let args: string[] = []
  let allowed: string[] = []
  try {
    args = mcpNewArgs.value.trim() ? JSON.parse(mcpNewArgs.value) : []
    allowed = mcpNewAllowed.value.trim() ? JSON.parse(mcpNewAllowed.value) : []
  } catch {
    mcpError.value = '参数/白名单必须是合法 JSON 数组'
    return
  }
  try {
    await invoke('mcp_add_server', { name, command, args, allowedTools: allowed })
    mcpNewName.value = ''
    mcpNewCommand.value = ''
    mcpNewArgs.value = ''
    mcpNewAllowed.value = ''
    useToastStore().success('MCP server 已添加')
    await loadMcp()
  } catch (e) {
    mcpError.value = String(e)
  }
}

async function removeMcpServer(id: number) {
  try {
    await invoke('mcp_remove_server', { id })
    useToastStore().success('已删除')
    await loadMcp()
  } catch (e) {
    mcpError.value = String(e)
  }
}

async function toggleMcpServer(s: McpServer) {
  try {
    await invoke('mcp_set_server_enabled', { id: s.id, enabled: !s.enabled })
    s.enabled = !s.enabled
    await loadMcp()
  } catch (e) {
    mcpError.value = String(e)
  }
}

async function callMcpTool() {
  if (!mcpCallTool.value) return
  mcpCalling.value = true
  mcpCallResult.value = ''
  mcpError.value = ''
  try {
    const args = JSON.parse(mcpCallArgs.value || '{}')
    mcpCallResult.value = await invoke<string>('mcp_call_tool', {
      name: mcpCallTool.value,
      arguments: args,
    })
  } catch (e) {
    mcpCallResult.value = `❌ ${String(e)}`
  } finally {
    mcpCalling.value = false
  }
}

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
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
        // WebMCP polyfill 试验开关（阶段 1.5）
        loadWebmcpPolyfill()
        // 打开时清空 AI Key 输入框：不回显掩码，避免用户无意把掩码写回库
        aiKeyInput.value = ''
        // MCP 工具配置（阶段 1.3）
        loadMcp()
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

/** 标签休眠超时选项（分钟，0=关闭；后端 hibernate_idle_tabs 读该设置） */
const HIBERNATE_OPTIONS = [
  { v: '0', label: '关闭' },
  { v: '10', label: '10 分钟' },
  { v: '30', label: '30 分钟' },
  { v: '60', label: '60 分钟' },
]

/** 翻译源（后端 translate.rs 按 settings.translate_engine 派发，失败自动回退另一方） */
const TRANSLATE_ENGINES = [
  { id: 'google', label: 'Google' },
  { id: 'bing', label: 'Bing' },
]

/** 翻译目标语言（Google/Bing 语言码，Bing 后端对 zh-CN 自动映射 zh-Hans） */
const TRANSLATE_TARGETS = [
  { id: 'zh-CN', label: '中文（简体）' },
  { id: 'zh-TW', label: '中文（繁体）' },
  { id: 'en', label: '英语' },
  { id: 'ja', label: '日语' },
  { id: 'ko', label: '韩语' },
  { id: 'fr', label: '法语' },
  { id: 'de', label: '德语' },
  { id: 'ru', label: '俄语' },
  { id: 'es', label: '西班牙语' },
  { id: 'pt', label: '葡萄牙语' },
  { id: 'it', label: '意大利语' },
]

const SEARCH_ENGINES = [  { id: 'bing', label: '必应', url: 'https://www.bing.com/search?q=' },
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
  // 书签栏显隐切换需同步通知 Rust 侧重算 top_offset
  if (key === 'show_bookmarks_bar') {
    try {
      await invoke('set_bookmarks_bar_visible', { visible: value === 'true' })
    } catch (e) {
      console.error('[settings] 书签栏显隐同步失败:', e)
      useToastStore().error('书签栏切换失败')
    }
  }
  // 启动页相关设置：保存后重新生成新标签页 HTML
  if (key === 'show_bg' || key === 'font_size' || key === 'search_engine') {
    try {
      await invoke('refresh_newtab')
    } catch (e) {
      console.error('[settings] 启动页刷新失败:', e)
    }
  }
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

/** 隐私防护档位（off / standard / strict），默认标准档 */
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

/** WebMCP polyfill 试验开关（阶段 1.5，默认关）：捕获页面暴露的 WebMCP 工具声明 */
const webmcpPolyfill = ref(false)
async function loadWebmcpPolyfill() {
  try {
    webmcpPolyfill.value = await invoke<boolean>('get_webmcp_polyfill')
  } catch (e) {
    console.error('[settings] 读取 WebMCP polyfill 开关失败:', e)
  }
}
async function onToggleWebmcpPolyfill() {
  const next = !webmcpPolyfill.value
  webmcpPolyfill.value = next
  try {
    await invoke('set_webmcp_polyfill', { enabled: next })
    useToastStore().success(next ? 'WebMCP polyfill 已开启（刷新页面后生效）' : 'WebMCP polyfill 已关闭')
  } catch (e) {
    webmcpPolyfill.value = !next
    console.error('[settings] WebMCP polyfill 切换失败:', e)
    useToastStore().error('WebMCP polyfill 切换失败')
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
        <span>⚙ 设置</span>
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
            <span class="nav-icon">{{ s.icon }}</span>
            <span class="nav-label">{{ s.label }}</span>
          </div>
        </div>

        <!-- 右侧内容 -->
        <div class="settings-content">
          <!-- 通用 -->
          <div v-if="activeSection === 'general'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🖥 通用</div>
              <div class="field">
                <label>主页 URL</label>
                <input
                  type="text"
                  :value="settingsStore.get('homepage')"
                  placeholder="newtab 或 https://..."
                  spellcheck="false"
                  @change="onSet('homepage', ($event.target as HTMLInputElement).value.trim())"
                />
              </div>
              <div class="field">
                <label>搜索引擎</label>
                <div class="seg-group">
                  <button
                    v-for="e in SEARCH_ENGINES"
                    :key="e.id"
                    class="seg-btn"
                    :class="{ active: (settingsStore.get('search_engine') || 'bing') === e.id }"
                    @click="onSet('search_engine', e.id)"
                  >
                    {{ e.label }}
                  </button>
                </div>
              </div>
              <div class="field">
                <label>标签休眠</label>
                <div class="seg-group">
                  <button
                    v-for="h in HIBERNATE_OPTIONS"
                    :key="h.v"
                    class="seg-btn"
                    :class="{ active: (settingsStore.get('tab_hibernate_minutes') || '30') === h.v }"
                    @click="onSet('tab_hibernate_minutes', h.v)"
                  >
                    {{ h.label }}
                  </button>
                </div>
              </div>
              <div class="kw-hint">
                闲置超过该时长的标签会自动休眠：<strong>销毁它的渲染进程、立刻释放内存</strong>（一个标签
                240-500MB），点击该标签时按原网址重新加载。标签开得多、内存吃紧时特别有用。
              </div>
              <div class="field">
                <label>翻译源</label>
                <div class="seg-group">
                  <button
                    v-for="t in TRANSLATE_ENGINES"
                    :key="t.id"
                    class="seg-btn"
                    :class="{ active: (settingsStore.get('translate_engine') || 'google') === t.id }"
                    @click="onSet('translate_engine', t.id)"
                  >
                    {{ t.label }}
                  </button>
                </div>
              </div>
              <div class="kw-hint">
                「翻译当前页面」、右键划词翻译、翻译面板都用这个源。<br />
                <b>Google</b>：译文质量好，但国内网络常连不上；<b>Bing</b>（Edge 端点）国内一般可直连。
                任一源失败会自动回退到另一方，改用即时生效、无需重启。
              </div>
              <div class="field">
                <label>翻译目标语言</label>
                <select
                  :value="settingsStore.get('translate_target') || 'zh-CN'"
                  @change="onSet('translate_target', ($event.target as HTMLSelectElement).value)"
                >
                  <option v-for="t in TRANSLATE_TARGETS" :key="t.id" :value="t.id">
                    {{ t.label }}
                  </option>
                </select>
              </div>
              <div class="kw-hint">
                翻译面板、划词翻译、整页翻译都按这个目标语言输出，切换即时生效。
              </div>
              <div class="field">
                <label>翻译超时（秒）</label>
                <input
                  type="number"
                  min="5"
                  max="120"
                  step="5"
                  :value="settingsStore.get('translate_timeout') || '15'"
                  @change="onSet('translate_timeout', ($event.target as HTMLInputElement).value)"
                />
              </div>
              <div class="kw-hint">
                翻译请求等待的最长时间（5–120 秒，默认 15）。网络慢或译文较长时调大；
                保存后新打开的页面立即生效，已打开页面刷新后生效。
              </div>
            </div>
          </div>
          <div v-if="activeSection === 'appearance'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🎨 外观</div>
              <div class="field">
                <label>主题</label>
                <div class="theme-picker">
                  <button
                    class="theme-card"
                    :class="{ active: (settingsStore.get('theme') || 'dark') === 'dark' }"
                    @click="onSet('theme', 'dark')"
                  >
                    <span class="theme-swatch dark-swatch"></span>
                    <span>深色</span>
                  </button>
                  <button
                    class="theme-card"
                    :class="{ active: (settingsStore.get('theme') || 'dark') === 'light' }"
                    @click="onSet('theme', 'light')"
                  >
                    <span class="theme-swatch light-swatch"></span>
                    <span>浅色</span>
                  </button>
                </div>
              </div>
              <div class="field">
                <label>显示书签栏</label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('show_bookmarks_bar') !== 'false'"
                    @change="onSet('show_bookmarks_bar', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="card-title" style="margin-top: 16px">🏠 启动页</div>
              <div class="field">
                <label>动态背景</label>
                <span class="field-desc">Bing 每日高清壁纸 + 缓慢缩放动画</span>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="settingsStore.get('show_bg') !== 'false'"
                    @change="onSet('show_bg', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
                  />
                  <span class="slider"></span>
                </label>
              </div>
              <div class="field">
                <label>字体大小</label>
                <select
                  :value="settingsStore.get('font_size') || '14'"
                  @change="onSet('font_size', ($event.target as HTMLSelectElement).value)"
                >
                  <option value="12">12px（小）</option>
                  <option value="13">13px</option>
                  <option value="14">14px（默认）</option>
                  <option value="15">15px</option>
                  <option value="16">16px（大）</option>
                  <option value="18">18px</option>
                  <option value="20">20px（特大）</option>
                </select>
              </div>
            </div>
          </div>

          <!-- AI 助手 -->
          <div v-if="activeSection === 'ai'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🤖 AI 助手</div>
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

          <!-- MCP 工具（阶段 1.3：AI 面板可调用外部工具） -->
          <div v-if="activeSection === 'mcp'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🔌 MCP 工具</div>
              <div class="field-hint">
                配置 MCP server（本地进程，stdio 传输）后，AI 助手对话会自动携带这些工具（function
                calling），可让 AI 读写文件、查日历等。每次工具调用都会写入审计日志。
              </div>
              <div v-if="mcpError" class="ext-error">{{ mcpError }}</div>

              <div class="mcp-add-row">
                <input v-model="mcpNewName" type="text" placeholder="名称（如 filesystem）" spellcheck="false" />
                <input v-model="mcpNewCommand" type="text" placeholder="命令（如 npx）" spellcheck="false" />
                <input v-model="mcpNewArgs" type="text" placeholder='参数 JSON（如 &quot;-y&quot;,&quot;@modelcontextprotocol/server-filesystem&quot;,&quot;/tmp&quot;）' spellcheck="false" />
                <input v-model="mcpNewAllowed" type="text" placeholder='工具白名单 JSON（空=全部，如 &quot;read_file&quot;）' spellcheck="false" />
                <button class="mini-btn primary" @click="addMcpServer" :disabled="mcpLoading">添加</button>
              </div>

              <div v-if="mcpServers.length === 0 && !mcpLoading" class="field-hint">
                还没有 MCP server。示例：命令 <code>npx</code>，参数
                <code>["-y","@modelcontextprotocol/server-filesystem","/tmp"]</code>
              </div>
              <div v-for="s in mcpServers" :key="s.id" class="mcp-server-row">
                <button
                  class="mini-btn"
                  :class="s.enabled ? 'primary' : ''"
                  @click="toggleMcpServer(s)"
                  :title="s.enabled ? '点击停用' : '点击启用'"
                >
                  {{ s.enabled ? '✅ 启用' : '⏸ 停用' }}
                </button>
                <span class="mcp-server-name">{{ s.name }}</span>
                <span class="mcp-server-command">{{ s.command }} {{ s.args.join(' ') }}</span>
                <span class="mcp-server-count">{{ s.toolCount }} 个工具</span>
                <button class="mini-btn danger" @click="removeMcpServer(s.id)">删除</button>
              </div>
            </div>

            <div class="card">
              <div class="card-title">🧰 可用工具（{{ mcpTools.length }}）</div>
              <div v-if="mcpTools.length === 0" class="field-hint">暂无可用工具（需启用且能连通的 server）</div>
              <div v-for="t in mcpTools" :key="t.name" class="mcp-tool-row">
                <span class="mcp-tool-name">{{ t.name }}</span>
                <span class="mcp-tool-desc">{{ t.description || '(无描述)' }}</span>
              </div>
            </div>

            <div class="card">
              <div class="card-title">🖐 手动调用测试</div>
              <div class="mcp-add-row">
                <select v-model="mcpCallTool">
                  <option value="">选择工具…</option>
                  <option v-for="t in mcpTools" :key="t.name" :value="t.name">{{ t.name }}</option>
                </select>
                <input v-model="mcpCallArgs" type="text" placeholder='参数 JSON（如 {"path":"/tmp/a.txt"}）' spellcheck="false" />
                <button class="mini-btn primary" @click="callMcpTool" :disabled="mcpCalling || !mcpCallTool">
                  {{ mcpCalling ? '调用中…' : '调用' }}
                </button>
              </div>
              <pre v-if="mcpCallResult" class="mcp-result">{{ mcpCallResult }}</pre>
            </div>

            <div class="card">
              <div class="card-title">📋 审计日志（最近 {{ mcpAudit.length }} 条）</div>
              <div v-if="mcpAudit.length === 0" class="field-hint">暂无工具调用记录</div>
              <div v-for="a in mcpAudit" :key="a.id" class="mcp-audit-row" :class="{ fail: !a.success }">
                <span class="mcp-audit-time">{{ a.createdAt }}</span>
                <span class="mcp-audit-tool">{{ a.serverName }} · {{ a.toolName }}</span>
                <span class="mcp-audit-args">{{ a.arguments }}</span>
                <span class="mcp-audit-result">{{ a.resultSummary || (a.success ? '(成功)' : '(失败)') }}</span>
              </div>
            </div>
          </div>

          <!-- 扩展 -->
          <div v-if="activeSection === 'extensions'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🧩 扩展</div>
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

          <!-- 隐私 -->
          <div v-if="activeSection === 'privacy'" class="settings-scroll">
            <div class="card">
              <div class="card-title">🛡 隐私</div>
              <div class="field">
                <label>隐私防护</label>
                <select class="privacy-select" :value="privacyLevel" @change="onChangePrivacyLevel">
                  <option value="standard">标准（推荐）</option>
                  <option value="strict">严格</option>
                  <option value="off">关闭</option>
                </select>
              </div>
              <div class="kw-hint">
                <b>标准</b>：Canvas / WebGL / 音频 / WebGPU 反指纹加噪，GPC 隐私信号，WebRTC 内网 IP 防护，
                字体与设备探针收敛，追踪参数清洗，第三方 Cookie 阻断。<br />
                <b>严格</b>：在标准之上再移除 WebUSB / 蓝牙 / 串口 / HID / Gamepad / 传感器等高危 API，
                并归一化语音音色、降级高精度计时（部分网站功能可能受影响）。<br />
                切换后刷新页面即对已开标签生效。
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'">
                <div class="kw-hint">
                  <b>加噪白名单</b>：下列站点<strong>不做加噪</strong>（Canvas 敏感的在线地图、
                  设计工具、视频剪辑等可加进来）。每行一个域名，子域名自动包含。
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
              <div class="kw-hint" style="margin-top: 10px">
                <b>防护统计</b>（本次运行）：已清洗追踪参数
                <b>{{ privacyStats?.cleaned_params ?? 0 }}</b> 次 · 隐私防护注入
                <b>{{ privacyStats?.privacy_injections ?? 0 }}</b> 次 · 广告拦截注入
                <b>{{ privacyStats?.adblock_injections ?? 0 }}</b> 次
                <button class="mini-btn" style="margin-left: 8px" @click="resetPrivacyStats">
                  重置
                </button>
              </div>
              <div class="kw-block" v-if="privacyLevel !== 'off'" style="margin-top: 10px">
                <div class="kw-hint">
                  <b>防护自测</b>：在当前标签页核对各项防护是否<strong>真的生效</strong>（对齐「信任需要证明」）。
                  需先打开一个网页。
                  <button
                    class="mini-btn primary"
                    style="margin-left: 8px"
                    :disabled="privacySelfTestRunning"
                    @click="runPrivacySelfTest"
                  >
                    {{ privacySelfTestRunning ? '自测中…' : '运行自测' }}
                  </button>
                </div>
                <div v-if="privacySelfTest" class="kw-hint" style="margin-top: 6px">
                  档位 <b>{{ privacySelfTest.shield_level }}</b> · 站点
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
              <div class="kw-block" v-if="privacyLevel !== 'off'" style="margin-top: 10px">
                <div class="kw-hint">
                  <b>站点 GPC 声明</b>：抓取本站的
                  <code>/.well-known/gpc.json</code>，看站点是否声明遵守 GPC（需先打开网页）。
                  <button
                    class="mini-btn primary"
                    style="margin-left: 8px"
                    :disabled="gpcChecking"
                    @click="checkGpcSite"
                  >
                    {{ gpcChecking ? '检测中…' : '检测本站声明' }}
                  </button>
                </div>
                <div v-if="gpcSite" class="kw-hint" style="margin-top: 6px">
                  <b>{{ gpcSite.origin }}</b> · {{ gpcSite.summary }}
                  <span v-if="gpcSite.last_update">（更新于 {{ gpcSite.last_update }}）</span>
                </div>
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
              <div class="field webmcp-field" :class="{ 'experiment-tag': webmcpPolyfill }">
                <label>
                  WebMCP polyfill <span class="exp-badge" v-if="webmcpPolyfill">试验中</span>
                </label>
                <label class="switch">
                  <input
                    type="checkbox"
                    :checked="webmcpPolyfill"
                    @change="onToggleWebmcpPolyfill"
                  />
                  <span class="slider"></span>
                </label>
                <div class="kw-hint" style="width: 100%">
                  试验特性（阶段 1.5）· 默认关。开启后向页面注入
                  <code>navigator.modelContext.registerTool</code> polyfill，捕获网站声明的 WebMCP
                  工具，让 AI 助手能感知「网站为智能体暴露了哪些工具」。仅捕获声明、不伪造执行
                  能力，试验完不留痕，可随时关闭。
                </div>
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
              <div class="field danger-field">
                <label>历史记录</label>
                <div class="history-actions">
                  <button class="danger-btn" @click="onClearHistory">清除历史数据</button>
                  <span v-if="clearedMsg" class="cleared-msg">{{ clearedMsg }}</span>
                </div>
              </div>
            </div>
          </div>
          <!-- 自检 -->
          <div v-if="activeSection === 'selfcheck'" class="settings-scroll">
            <div class="card">
              <div class="card-title">
                🩺 系统自检
                <button class="selfcheck-btn" :disabled="selfCheckLoading" @click="runSelfCheck">
                  {{ selfCheckLoading ? '检查中…' : '重新检查' }}
                </button>
              </div>
              <div v-if="selfCheckError" class="selfcheck-error">{{ selfCheckError }}</div>
              <div v-else-if="!selfCheckReport" class="selfcheck-hint">点击「重新检查」开始体检</div>
              <template v-else>
                <div class="selfcheck-summary" :class="selfCheckReport.healthy ? 'ok' : 'bad'">
                  {{ selfCheckReport.healthy ? '✅ 一切正常' : '⚠️ 发现异常项' }}
                </div>
                <div v-for="g in selfCheckGroups" :key="g.title" class="selfcheck-group">
                  <div class="selfcheck-group-title">{{ g.title }}</div>
                  <div v-for="it in g.items" :key="g.title + it.name" class="selfcheck-row">
                    <span class="selfcheck-mark">{{ it.ok ? '✅' : '❌' }}</span>
                    <span class="selfcheck-name">{{ it.name }}</span>
                    <span class="selfcheck-detail">{{ it.detail }}</span>
                  </div>
                </div>
              </template>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.settings-dialog {
  width: min(620px, calc(100vw - 32px));
  height: min(600px, calc(100vh - 120px));
  display: flex;
  flex-direction: column;
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
  width: 132px;
  flex-shrink: 0;
  border-right: 1px solid var(--border-subtle);
  padding: 10px 8px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--bg-secondary);
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 7px;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition:
    background 0.15s,
    color 0.15s;
  user-select: none;
}
.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-item.active {
  background: rgba(74, 158, 255, 0.15);
  color: var(--accent);
  font-weight: 600;
}
.nav-icon {
  font-size: 15px;
  width: 18px;
  text-align: center;
  flex-shrink: 0;
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
.selfcheck-summary {
  margin: 8px 0 12px;
  padding: 8px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.selfcheck-summary.ok {
  background: rgba(46, 160, 67, 0.15);
  color: #3fb950;
}
.selfcheck-summary.bad {
  background: rgba(229, 57, 53, 0.12);
  color: #e53935;
}
.selfcheck-hint {
  color: var(--text-muted);
  font-size: 13px;
  padding: 12px 0;
}
.selfcheck-error {
  color: #e53935;
  font-size: 12px;
  padding: 8px 0;
}
.selfcheck-group {
  margin-bottom: 12px;
}
.selfcheck-group-title {
  font-size: 12px;
  color: var(--text-muted);
  margin-bottom: 4px;
}
.selfcheck-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  padding: 3px 0;
  font-size: 13px;
}
.selfcheck-mark {
  flex-shrink: 0;
}
.selfcheck-name {
  flex-shrink: 0;
  min-width: 96px;
  color: var(--text-primary);
}
.selfcheck-detail {
  color: var(--text-muted);
  word-break: break-all;
}
.card-title {
  font-size: 13px;
  color: var(--text-primary);
  font-weight: 600;
  margin-bottom: 10px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border-subtle);
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
  color: var(--text-muted);
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
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23999' d='M2.5 4.5L6 8l3.5-3.5'/></svg>");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 12px;
}
.field input:focus,
.field select:focus {
  border-color: var(--accent);
}

/* 搜索引擎分段按钮 */
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
  background: rgba(74, 158, 255, 0.15);
  border-color: var(--accent);
  color: var(--accent);
  font-weight: 600;
}

/* 主题选择卡片 */
.theme-picker {
  display: flex;
  gap: 8px;
  flex: 1;
  max-width: 300px;
}
.theme-card {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  padding: 10px 0 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: 8px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.theme-card:hover {
  background: var(--bg-hover);
}
.theme-card.active {
  border-color: var(--accent);
  color: var(--accent);
  box-shadow: 0 0 0 1px var(--accent);
}
.theme-swatch {
  width: 44px;
  height: 26px;
  border-radius: 5px;
  border: 1px solid var(--border-color);
}
.dark-swatch {
  background: linear-gradient(135deg, #16213e 50%, #1e2a45 50%);
}
.light-swatch {
  background: linear-gradient(135deg, #f5f6f8 50%, #e8eaee 50%);
}

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
  background: rgba(74, 158, 255, 0.45);
}
.switch input:checked + .slider::before {
  transform: translateX(16px);
  background: var(--accent);
}

/* 危险操作 */
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
.mcp-add-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 10px;
}
.mcp-add-row input,
.mcp-add-row select {
  flex: 1 1 180px;
  min-width: 120px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  padding: 6px 8px;
  color: var(--text-primary);
  font-size: 12px;
}
.mcp-server-row {
  display: flex;
  align-items: center;
  gap: 8px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  padding: 6px 10px;
  margin-bottom: 6px;
  font-size: 12px;
}
.mcp-server-name {
  font-weight: 600;
  white-space: nowrap;
}
.mcp-server-command {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.mcp-server-count {
  color: var(--text-secondary);
  white-space: nowrap;
}
.mcp-tool-row {
  display: flex;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  border-bottom: 1px dashed var(--border-subtle);
}
.mcp-tool-name {
  font-family: var(--font-mono, monospace);
  color: var(--accent);
  white-space: nowrap;
}
.mcp-tool-desc {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.mcp-result {
  margin-top: 6px;
  background: var(--bg-primary);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  padding: 8px;
  font-size: 12px;
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 160px;
  overflow: auto;
}
.mcp-audit-row {
  display: flex;
  gap: 8px;
  padding: 4px 0;
  font-size: 12px;
  border-bottom: 1px dashed var(--border-subtle);
  align-items: baseline;
}
.mcp-audit-row.fail {
  color: #ff6b6b;
}
.mcp-audit-time {
  color: var(--text-secondary);
  white-space: nowrap;
}
.mcp-audit-tool {
  font-weight: 600;
  white-space: nowrap;
}
.mcp-audit-args {
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}
.mcp-audit-result {
  max-width: 40%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
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
  background: rgba(74, 158, 255, 0.15);
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
  color: var(--text-secondary);
  border: none;
  border-radius: 6px;
  padding: 5px 11px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}
.mini-btn:hover {
  background: var(--text-secondary);
  color: var(--bg-primary);
}
.mini-btn.primary {
  background: rgba(74, 158, 255, 0.18);
  color: #cfe4ff;
}
.mini-btn.primary:hover {
  background: rgba(74, 158, 255, 0.3);
}
.mini-btn.danger {
  color: #ff8a8a;
  background: rgba(229, 57, 53, 0.1);
}
.mini-btn.danger:hover {
  background: rgba(229, 57, 53, 0.25);
}

/* 关键词 */
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
</style>

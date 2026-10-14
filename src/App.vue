<script setup lang="ts">
// 浏览器主窗口布局
// 结构：TabBar(38) + AddressBar(36) + BookmarksBar(26) + 内容区(webview) + StatusBar(22)
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, ref, watch, type Ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useI18n } from 'vue-i18n'
import { invokeOk } from './utils/invoke'
import { isDialogGuardedAction } from './utils/shortcut-guard'
import { useManagedListeners } from './composables/useManagedListeners'
import TabBar from './components/TabBar.vue'
import VerticalTabBar from './components/VerticalTabBar.vue'
import AddressBar from './components/AddressBar.vue'
import BookmarksBar from './components/BookmarksBar.vue'
import StatusBar from './components/StatusBar.vue'
// R1 首屏分包：用户主动打开的弹窗/面板按需加载（点开才拉取对应 chunk）
// 安全即时类（CertWarning/PermissionPrompt）与全局反馈（AppToast）保留同步，必须立即可见
const PasswordsDialog = defineAsyncComponent(() => import('./components/PasswordsDialog.vue'))
const GroupsDialog = defineAsyncComponent(() => import('./components/GroupsDialog.vue'))
const DownloadsDialog = defineAsyncComponent(() => import('./components/DownloadsDialog.vue'))
const SettingsDialog = defineAsyncComponent(() => import('./components/SettingsDialog.vue'))
const AISidebar = defineAsyncComponent(() => import('./components/AISidebar.vue'))
const TranslatePanel = defineAsyncComponent(() => import('./components/TranslatePanel.vue'))
const HistoryDialog = defineAsyncComponent(() => import('./components/HistoryDialog.vue'))
const BookmarksDialog = defineAsyncComponent(() => import('./components/BookmarksDialog.vue'))
const SessionDialog = defineAsyncComponent(() => import('./components/SessionDialog.vue'))
const UserscriptsDialog = defineAsyncComponent(() => import('./components/UserscriptsDialog.vue'))
const CommandPalette = defineAsyncComponent(() => import('./components/CommandPalette.vue'))
const CertificatesDialog = defineAsyncComponent(() => import('./components/CertificatesDialog.vue'))
import CertWarning from './components/CertWarning.vue'
import PermissionPrompt from './components/PermissionPrompt.vue'
import AppToast from './components/AppToast.vue'
import { useTabsStore, setLockedState } from './stores/tabs'
import { useGroupsStore } from './stores/groups'
import { useSettingsStore } from './stores/settings'
import { useWorkspacesStore } from './stores/workspaces'
import { useAIStore } from './stores/ai'
import { useToastStore } from './stores/toast'
import { useBackdropStore } from './stores/backdrop'
// 布局常量单一来源（build.rs 由 src-tauri/src/constants.rs 生成）→ 注入 CSS 变量
import { EVENTS, LAYOUT, SHORTCUTS } from './constants'

const tabsStore = useTabsStore()
const backdropStore = useBackdropStore()
const groupsStore = useGroupsStore()
const settingsStore = useSettingsStore()
const workspacesStore = useWorkspacesStore()
const aiStore = useAIStore()
const { t } = useI18n()

// 垂直标签模式（阶段 3.1）：标签栏在左（true）vs 在上（false）
const verticalTabs = computed(() => settingsStore.get('vertical_tabs') === 'true')

// 布局尺寸注入为 CSS 变量：CSS 与 JS 共用同一常量源（constants.ts），
// 消除"JS 改了、CSS 没改"的第三层漂移（组件 CSS 用 var(--sb-*-h)）
const layoutCssVars = computed(() => ({
  '--sb-tab-bar-h': `${LAYOUT.tabBar}px`,
  '--sb-address-bar-h': `${LAYOUT.addressBar}px`,
  '--sb-bookmarks-bar-h': `${LAYOUT.bookmarksBar}px`,
  '--sb-status-bar-h': `${LAYOUT.statusBar}px`,
  '--sb-ai-panel-w': `${LAYOUT.aiPanelWidth}px`,
}))

/** 切换标签栏方向：写设置 + 通知 Rust 重算内容区几何（top 与 left 都变） */
async function onToggleVerticalTabs() {
  const next = !verticalTabs.value
  await settingsStore.set('vertical_tabs', next ? 'true' : 'false')
  try {
    await invokeOk('set_vertical_tabs', { active: next })
  } catch (e) {
    console.error('[app] 切换标签栏方向失败:', e)
  }
}

// 空间主题色 → 全局强调色（阶段 3.2；空 = 跟随默认）
watch(
  () => workspacesStore.currentColor,
  (c) => {
    const el = document.documentElement
    if (c) el.style.setProperty('--accent', c)
    else el.style.removeProperty('--accent')
  },
  { immediate: true },
)
const showPasswords = ref(false)
const showGroups = ref(false)
const showDownloads = ref(false)
const showSettings = ref(false)
// 打开设置时指定初始选项卡（默认通用；状态栏隐私战绩点击 → 隐私防护）
const settingsInitialSection = ref<string | undefined>(undefined)
function openSettings(section?: string) {
  settingsInitialSection.value = section
  showSettings.value = true
}
const showAI = ref(false)
// 证书中心（P1/P3）+ 证书错误拦截页（P2）
const showCerts = ref(false)
// 权限请求提示（安全 #2）
interface PermissionRequest {
  id: number
  tabId: string
  host: string
  kind: string
  kindZh: string
}
interface TlsErrorInfo {
  tabId: string
  host: string
  port: number
  flags: number
  reasons: string[]
  fingerprint: string
  subject: string
  issuer: string
  notAfter: string | null
}
interface WebProcessCrash {
  reason?: string
  autoReload?: boolean
}
const permReq = ref<PermissionRequest | null>(null)
// 权限请求队列：连续两个请求时旧方案直接覆盖丢弃，页面永久挂起。这里排队逐个处理
const permQueue = ref<PermissionRequest[]>([])
function pushPermReq(req: PermissionRequest) {
  // 同一 id 去重（后端重发时），否则入队
  if (permReq.value?.id === req.id || permQueue.value.some((q) => q.id === req.id)) return
  if (!permReq.value) {
    permReq.value = req
  } else {
    permQueue.value.push(req)
  }
}
function shiftPermReq() {
  permReq.value = permQueue.value.shift() ?? null
}
const tlsError = ref<TlsErrorInfo | null>(null)
// 证书警告队列：多标签同时报 TLS 错误时旧方案直接覆盖，先到的警告永久丢失。
// 与权限队列同机制排队逐个处理，去重按 tabId+指纹（同站重报不堆积）
const tlsQueue = ref<TlsErrorInfo[]>([])
function pushTlsError(info: TlsErrorInfo) {
  const key = (t: TlsErrorInfo) => `${t.tabId}|${t.fingerprint}`
  if (tlsError.value && key(tlsError.value) === key(info)) return
  if (tlsQueue.value.some((q) => key(q) === key(info))) return
  if (!tlsError.value) {
    tlsError.value = info
  } else {
    tlsQueue.value.push(info)
  }
}
function shiftTlsError() {
  tlsError.value = tlsQueue.value.shift() ?? null
}

function openPanel(name: PanelName) {
  for (const k of PANEL_KEYS) panels[k].value = k === name
}

const showTranslate = ref(false)
const showHistory = ref(false)
const showBookmarks = ref(false)
const showSessions = ref(false)
// 命令栏（Ctrl+K 万能入口，阶段 3.1）
const showCommandPalette = ref(false)
// 用户脚本管理（阶段 3.2）
const showUserscripts = ref(false)
const addressBarRef = ref<InstanceType<typeof AddressBar> | null>(null)
// 面板开关收表：再加面板只加一行，不改 openPanel
type PanelName = 'translate' | 'sessions' | 'history' | 'passwords' | 'groups' | 'downloads' | 'settings' | 'ai' | 'bookmarks' | 'userscripts'
const panels: Record<PanelName, Ref<boolean>> = {
  translate: showTranslate,
  sessions: showSessions,
  history: showHistory,
  passwords: showPasswords,
  groups: showGroups,
  downloads: showDownloads,
  settings: showSettings,
  ai: showAI,
  bookmarks: showBookmarks,
  userscripts: showUserscripts,
}
const PANEL_KEYS = Object.keys(panels) as PanelName[]
let saveTimer: number | null = null
// 锁屏兜底轮询句柄（R1-5：提升到组件级，卸载时必须清理，避免组件销毁后轮询仍在跑）
let lockTimer: ReturnType<typeof window.setInterval> | null = null

// Tauri 事件监听统一托管：组件卸载时自动解绑全部 unlisten（R1-5，防重复监听泄漏）
const { add: onEvent } = useManagedListeners()

/** Esc 关闭最上层弹窗（AI 侧栏优先，再设置/下载/分组/密码）；Ctrl 组合键为浏览器标配快捷键 */
/** 弹窗是否打开（弹窗打开时标签级快捷键不响应，避免误关/误建标签） */
function anyDialogOpen() {
  return (
    showTranslate.value ||
    showAI.value ||
    showSettings.value ||
    showDownloads.value ||
    showGroups.value ||
    showPasswords.value ||
    showHistory.value ||
    showSessions.value ||
    showBookmarks.value ||
    showCommandPalette.value ||
    showUserscripts.value ||
    showCerts.value ||
    tlsError.value !== null ||
    permReq.value !== null
  )
}

/**
 * 统一快捷键动作入口。
 * 两个来源共用：① 前端 keydown（UI 层持焦时）② GTK 窗口层转发（页面持焦时，
 * 前端收不到按键，由 Rust 在 GTK 层拦截后 emit `global-shortcut`）。
 * 弹窗守卫与 keydown 同口径（见 utils/shortcut-guard，单测锁定）：
 * GTK 路径不经过 onGlobalKeydown 的守卫，AI 开着时点一下页面再按 Ctrl+T，
 * 两边行为必须一致（焦点统一实施）。
 */
function handleGlobalAction(action: string) {
  if (isDialogGuardedAction(action) && anyDialogOpen()) return
  switch (action) {
    case 'new-tab':
      tabsStore.createTab()
      break
    case 'close-tab':
      if (tabsStore.activeTabId) tabsStore.closeTab(tabsStore.activeTabId)
      break
    case 'reload':
      if (tabsStore.activeTabId) tabsStore.reloadTab(tabsStore.activeTabId)
      break
    case 'focus-address':
      addressBarRef.value?.focusInput()
      break
    case 'history':
      openPanel('history')
      break
    case 'translate-panel':
      openPanel('translate')
      break
    case 'sessions':
      openPanel('sessions')
      break
    case 'downloads':
      openPanel('downloads')
      break
    case 'bookmarks':
      openPanel('bookmarks')
      break
    case 'next-tab':
    case 'prev-tab': {
      const tabs = tabsStore.tabs
      if (tabs.length < 2) break
      const idx = tabs.findIndex((t) => t.id === tabsStore.activeTabId)
      const step = action === 'next-tab' ? 1 : -1
      tabsStore.switchTab(tabs[(idx + step + tabs.length) % tabs.length].id)
      break
    }
    case 'command-palette':
      openCommandPalette()
      break
    case 'devtools':
      // 有意静默：开发者工具开关属调试辅助，webview 侧失败无任何用户可见后果，无需提示
      invokeOk('open_devtools').catch(() => {})
      break
    case 'lock-vault':
      // 手动立即锁定密码库（Ctrl+Shift+L）：未设主密码/已锁定时后端为无害空操作
      invoke('lock_vault')
        .then(() => useToastStore().success('密码库已锁定'))
        .catch(() => {}) // 有意静默：锁定失败后端已记日志，弹窗会打断用户当前操作
      break
  }
}

/** 打开命令栏（Ctrl+K）：先收起其它模态，避免叠层 */
function openCommandPalette() {
  showTranslate.value = false
  showSessions.value = false
  showHistory.value = false
  showPasswords.value = false
  showGroups.value = false
  showDownloads.value = false
  showSettings.value = false
  showBookmarks.value = false
  showCommandPalette.value = true
}

/** 命令栏选中「命令」类：面板类走 openPanel，其余交给统一动作入口 */
function onPaletteCommand(action: string) {
  switch (action) {
    case 'ai':
    case 'history':
    case 'bookmarks':
    case 'downloads':
    case 'passwords':
    case 'sessions':
    case 'translate':
    case 'settings':
    case 'userscripts':
      openPanel(action)
      break
    case 'close-same-domain': {
      const n = tabsStore.closeSameDomain()
      useToastStore().success(`已关闭 ${n} 个同域名标签`)
      break
    }
    case 'close-duplicates': {
      const n = tabsStore.closeDuplicates()
      useToastStore().success(n > 0 ? `已关闭 ${n} 个重复标签` : '没有重复标签')
      break
    }
    case 'mute-all': {
      const muted = tabsStore.toggleMuteAll()
      useToastStore().success(muted ? '已全部静音' : '已全部取消静音')
      break
    }
    default:
      handleGlobalAction(action)
  }
}

/** 命令栏选中「问 AI」：带上当前页面上下文（无才补，不覆盖侧栏已同步的）；
 * 侧栏已开则直接发问，否则暂存待侧栏打开时消费 */
function onPaletteAskAi(query: string) {
  const tab = tabsStore.activeTab
  if (tab && !aiStore.pageContext) aiStore.setPageContext(tab.url, tab.title)
  if (showAI.value) {
    aiStore.sendMessage(query)
  } else {
    aiStore.queuePrompt(query)
    openPanel('ai')
  }
}

/** 快捷键匹配：组合键字符串形如 'ctrl+shift+t'
 *  组合定义来自 constants.ts 的 SHORTCUTS（源：src-tauri/src/contracts.rs），
 *  与 Rust GTK 层兜底表同一份定义，改一处两端同步。 */
function matchShortcut(e: KeyboardEvent, combo: string): boolean {
  const parts = combo.toLowerCase().split('+')
  const key = parts[parts.length - 1]
  return (
    e.key.toLowerCase() === key &&
    e.ctrlKey === parts.includes('ctrl') &&
    e.shiftKey === parts.includes('shift') &&
    !e.altKey &&
    !e.metaKey
  )
}

function onGlobalKeydown(e: KeyboardEvent) {
  // 输入法组字中（拼音候选态）：所有全局快捷键一律放行，让候选框先处理。
  // 否则组字时按 Enter/上下/Esc 会误导航、误关弹窗（与 Chrome 行为一致：组字优先）。
  // 非组字态的输入框内保持浏览器级优先（同 Chrome：Ctrl+W 照关标签）。
  if (e.isComposing) return
  // F12 / Ctrl+Shift+I：打开当前标签的开发者工具（Web Inspector）
  if (matchShortcut(e, SHORTCUTS.DEVTOOLS) || matchShortcut(e, SHORTCUTS.DEVTOOLS_ALT)) {
    e.preventDefault()
    handleGlobalAction('devtools')
    return
  }

  // Ctrl+Tab / Ctrl+Shift+Tab：切换前后标签
  if (matchShortcut(e, SHORTCUTS.NEXT_TAB)) {
    e.preventDefault()
    handleGlobalAction('next-tab')
    return
  }
  if (matchShortcut(e, SHORTCUTS.PREV_TAB)) {
    e.preventDefault()
    handleGlobalAction('prev-tab')
    return
  }

  // Ctrl 组合快捷键（不拦截无 Ctrl 的普通按键）
  if (e.ctrlKey && !e.altKey && !e.metaKey) {
    // Ctrl+L 聚焦地址栏（任何情况下都生效，含输入框内）
    if (matchShortcut(e, SHORTCUTS.FOCUS_ADDRESS)) {
      e.preventDefault()
      handleGlobalAction('focus-address')
      return
    }
    // Ctrl+K 命令栏（切换：任何情况下都生效，含输入框内）
    if (matchShortcut(e, SHORTCUTS.COMMAND_PALETTE)) {
      e.preventDefault()
      showCommandPalette.value = !showCommandPalette.value
      return
    }
    // Ctrl+Shift+L 立即锁定密码库（安全动作，弹窗/输入框内也生效）
    if (matchShortcut(e, SHORTCUTS.LOCK_VAULT)) {
      e.preventDefault()
      handleGlobalAction('lock-vault')
      return
    }
    // 弹窗打开时，其余标签快捷键不响应（避免误关标签/新建标签）
    if (anyDialogOpen()) return
    // Ctrl+Shift+T 翻译面板 / Ctrl+Shift+S 会话管理（不影响 Ctrl+T 建标签）
    if (matchShortcut(e, SHORTCUTS.TRANSLATE_PANEL)) {
      e.preventDefault()
      handleGlobalAction('translate-panel')
      return
    }
    if (matchShortcut(e, SHORTCUTS.SESSIONS)) {
      e.preventDefault()
      handleGlobalAction('sessions')
      return
    }
    if (matchShortcut(e, SHORTCUTS.DOWNLOADS)) {
      e.preventDefault()
      handleGlobalAction('downloads')
      return
    }
    if (matchShortcut(e, SHORTCUTS.BOOKMARKS)) {
      e.preventDefault()
      handleGlobalAction('bookmarks')
      return
    }
    if (matchShortcut(e, SHORTCUTS.NEW_TAB)) {
      e.preventDefault()
      handleGlobalAction('new-tab')
    } else if (matchShortcut(e, SHORTCUTS.CLOSE_TAB)) {
      e.preventDefault()
      handleGlobalAction('close-tab')
    } else if (matchShortcut(e, SHORTCUTS.RELOAD)) {
      e.preventDefault()
      handleGlobalAction('reload')
    } else if (matchShortcut(e, SHORTCUTS.HISTORY)) {
      e.preventDefault()
      handleGlobalAction('history')
    }
    return
  }

  if (e.key !== 'Escape') return
  // Esc 关闭链：安全警告（证书/权限）必须明确二选一，Esc 不关（有意为之，防误点放行）
  if (showCommandPalette.value) {
    showCommandPalette.value = false
  } else if (showTranslate.value) {
    showTranslate.value = false
  } else if (showAI.value) {
    showAI.value = false
  } else if (showSettings.value) {
    showSettings.value = false
  } else if (showDownloads.value) {
    showDownloads.value = false
  } else if (showGroups.value) {
    showGroups.value = false
  } else if (showPasswords.value) {
    showPasswords.value = false
  } else if (showHistory.value) {
    showHistory.value = false
  } else if (showSessions.value) {
    showSessions.value = false
  } else if (showUserscripts.value) {
    showUserscripts.value = false
  } else if (showBookmarks.value) {
    showBookmarks.value = false
  } else if (showCerts.value) {
    showCerts.value = false
  }
}

async function initBrowser(skipStartupTab = false) {
  // 设置尽早加载（主页/搜索引擎/主题/书签栏显隐依赖）
  await settingsStore.loadSettings()
  // 启动锁屏已迁移到 Rust/GTK 层（lib.rs setup 里弹 GTK 原生密码对话框，
  // 在所有 webview 创建之前验证），不再在 Vue 层处理——Vue 锁屏覆盖层只能盖住
  // Vue 自己的 DOM，盖不住另一个独立的 GTK webview（标签页）
  // 同步书签栏显隐到 Rust 侧（初始化 top_offset）
  try {
    await invokeOk('set_bookmarks_bar_visible', { visible: settingsStore.showBookmarksBar })
  } catch (e) {
    console.error('[app] 书签栏初始化同步失败:', e)
  }
  // 同步标签栏方向到 Rust 侧（垂直标签：内容区 top 与 left 都变）
  try {
    await invokeOk('set_vertical_tabs', {
      active: settingsStore.get('vertical_tabs') === 'true',
    })
  } catch (e) {
    console.error('[app] 标签栏方向初始化同步失败:', e)
  }
  // 落盘网络层拦截规则到扩展规则文件（阶段3.2；Web 进程扩展启动时读取）
  try {
    await invoke('sync_ext_rules')
  } catch (e) {
    console.error('[app] 网络层规则落盘失败:', e)
  }
  // 刷新用户脚本注入缓存（阶段3.2；inject 路径读缓存，不能 await）
  try {
    await invoke('refresh_userscripts_cache')
  } catch (e) {
    console.error('[app] 用户脚本缓存刷新失败:', e)
  }
  // 分组尽早加载（不等标签恢复，避免右键菜单/组头迟迟不显示）
  groupsStore.loadGroups().catch((e) => {
    console.error('[app] 加载分组失败:', e)
  })
  try {
    // 工作区列表加载（当前工作区 = 默认）
    await workspacesStore.loadWorkspaces()
    // 启动时恢复标签状态（loadTabs 内部已容错）
    await tabsStore.loadTabs()
    // skipStartupTab：解锁路径由调用方统一建启动页，这里不建（避免与"恢复会话"重复开标签）
    if (!skipStartupTab && tabsStore.tabs.length === 0) {
      // 无标签：先尝试恢复当前工作区最近会话，失败则创建默认标签
      const restored = await tabsStore.restoreLastSession(workspacesStore.currentId)
      if (!restored) {
        await tabsStore.createTab()
      }
    }
  } catch (e) {
    console.error('[app] 启动初始化失败:', e)
    // 兜底：至少保证有一个标签可用（解锁路径由调用方建启动页，这里跳过）
    if (!skipStartupTab && tabsStore.tabs.length === 0) {
      // 有意静默：错误兜底路径里的最后降级，createTab 失败已无法再向用户呈现更多内容
      await tabsStore.createTab().catch(() => {})
    }
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onGlobalKeydown)

  // 前端日志桥：关键节点写进 Rust 日志（排查前端白屏/事件收不到/初始化不执行）
  function frontLog(msg: string) {
    // 有意静默：日志桥本身失败不能再 toast/console（会形成噪音递归），后端不可用时直接丢弃
    invoke('front_log', { msg }).catch(() => {})
  }
  frontLog('onMounted 开始')

  // 初始化只跑一次（app-unlocked 事件、兜底轮询、未锁屏直达三条路径都可能到达）
  let initRunning = false
  async function initBrowserSafe(skipStartupTab = false) {
    if (initRunning) return
    initRunning = true
    try {
      await initBrowser(skipStartupTab)
    } finally {
      initRunning = false
    }
  }
  // 就绪：初始化 + 保证至少一个标签（解锁来的走"开启动页"，否则沿用主页/恢复会话）
  let ready = false
  async function onReady(fromUnlock: boolean) {
    if (ready) return
    ready = true
    setLockedState(false)
    frontLog('onReady 开始 fromUnlock=' + fromUnlock)
    await initBrowserSafe(fromUnlock).catch((e) => {
      frontLog('initBrowser 失败: ' + e)
    })
    if (fromUnlock) {
      // 解锁后自动打开启动页（skipStartupTab=true 时 initBrowser 不建标签，这里统一建）
      await tabsStore.createTab('newtab').catch((e) => {
        frontLog('解锁后创建启动页失败: ' + e)
      })
    } else if (tabsStore.tabs.length === 0) {
      await tabsStore.createTab().catch((e) => {
        frontLog('创建初始标签失败: ' + e)
      })
    }
    tabsStore.startPolling()
    // 分屏状态同步（阶段 3.1；进程内状态，重启后为未分屏）
    tabsStore.loadSplit()
    frontLog('onReady 完成 标签数=' + tabsStore.tabs.length)
    // 启动自检：把版本/存储/运行时/能力状态写进 Rust 日志（只读，便于事后排查）
    invoke<{ healthy: boolean }>('selfcheck_cmd')
      .then((r) => frontLog('启动自检完成 健康=' + r?.healthy))
      .catch((e) => frontLog('启动自检失败: ' + e))
  }

  // ① 先查锁屏状态：await 一次 IPC 确保 IPC 桥已就绪（之后注册事件监听才可靠）
  let locked = false
  try {
    locked = await invoke<boolean>('is_locked')
    frontLog('is_locked=' + locked)
  } catch (e) {
    frontLog('is_locked 调用失败: ' + e)
    locked = false
  }
  setLockedState(locked)

  if (locked) {
    // ② 锁屏中：await 注册 app-unlocked（确保注册完成，避免"解锁事件早于注册"丢失）
    //    监听由 useManagedListeners 托管，卸载自动解绑
    await onEvent(EVENTS.APP_UNLOCKED, () => {
      frontLog('收到 app-unlocked 事件')
      onReady(true)
    })
    frontLog('已注册 app-unlocked 监听')
    // ③ 兜底轮询：万一时序问题导致事件丢失，每秒查一次锁屏状态，解开就初始化
    lockTimer = window.setInterval(async () => {
      if (ready) {
        if (lockTimer) clearInterval(lockTimer)
        lockTimer = null
        return
      }
      try {
        const l = await invoke<boolean>('is_locked')
        if (!l) {
          frontLog('兜底轮询检测到已解锁')
          if (lockTimer) clearInterval(lockTimer)
          lockTimer = null
          onReady(true)
        }
      } catch {
        /* 忽略：等下一轮 */
      }
    }, 1000)
  } else {
    onReady(false)
  }

  // 启动页右侧栏按钮 → 打开主界面组件（newtab webview 通过 IPC 触发）
  onEvent(EVENTS.OPEN_AI_FROM_NEWTAB, () => {
    showAI.value = true
  })
  onEvent(EVENTS.OPEN_SETTINGS_FROM_NEWTAB, () => {
    showSettings.value = true
  })
  // 启动页资讯点击 → 左窗格保持列表页，右窗格分屏打开资讯原文（占满右半区域）
  onEvent<string>(EVENTS.OPEN_URL_FROM_NEWTAB, async (e) => {
    if (!e.payload) return
    // 已有分屏：右窗格直接换内容（"点一条看一条"，不重复开标签）
    const existing = tabsStore.splitTabId
    if (existing && tabsStore.tabs.some((t) => t.id === existing)) {
      await tabsStore.navigateTab(existing, e.payload)
      return
    }
    const leftId = tabsStore.activeTabId
    const tab = await tabsStore.createTab(e.payload)
    if (!tab) return
    // 活动标签切回列表页（左窗格），资讯标签放右窗格分屏
    if (leftId && leftId !== tab.id) await tabsStore.switchTab(leftId)
    await tabsStore.setSplit(tab.id)
  })

  // 系统托盘菜单 → 打开面板（Rust 侧 emit，见 tray.rs）
  onEvent(EVENTS.TRAY_OPEN_DOWNLOADS, () => {
    openPanel('downloads')
  })
  onEvent(EVENTS.TRAY_OPEN_PASSWORDS, () => {
    openPanel('passwords')
  })

  // 首次缩托盘（Rust 侧 delete-event 只发一次）→ 告诉用户没退出
  onEvent(EVENTS.TRAY_FIRST_MINIMIZE, () => {
    useToastStore().show('已缩到托盘（没退出），点托盘图标可恢复；真退出走托盘菜单', 'info', 6000)
  })

  // 证书错误（P2 · Rust 侧 on_tls_error emit）→ 排队弹"危险站点"拦截页，防覆盖丢失
  onEvent<TlsErrorInfo>(EVENTS.TLS_ERROR, (ev) => {
    if (ev.payload) pushTlsError(ev.payload)
  })

  // 权限请求（安全 #2 · Rust 侧 perms::attach emit）→ 排队弹权限提示，防覆盖丢失
  onEvent<PermissionRequest>(EVENTS.PERMISSION_REQUEST, (ev) => {
    if (ev.payload) pushPermReq(ev.payload)
  })

  // Web 进程崩溃（视频崩溃修复 · Rust 侧 connect_web_process_terminated emit）→ 提示
  onEvent<WebProcessCrash>(EVENTS.WEB_PROCESS_CRASHED, (ev) => {
    const p = ev.payload ?? {}
    const reason = p.reason === 'ExceededMemoryLimit' ? '内存超限' : '进程崩溃'
    useToastStore().show(
      p.autoReload ? `网页${reason}，已自动重新加载` : `网页${reason}，请手动刷新`,
      'error',
      4500,
    )
  })

  // GTK 窗口层转发的全局快捷键：页面（child webview）持焦时，前端 keydown 收不到按键，
  // 由 Rust 在 GTK 顶层窗口拦截后发事件过来（见 tabs.rs 的 UI_FOCUSED 焦点跟踪）。
  onEvent<string>(EVENTS.GLOBAL_SHORTCUT, (ev) => {
    handleGlobalAction(ev.payload)
  })

  // 定时 Agent 提示词（阶段3 Agent 编排）：调度器到点发事件 → 打开 AI 侧栏并发送
  onEvent<string>(EVENTS.AUTO_AGENT, (ev) => {
    const prompt = ev.payload
    if (!prompt) return
    openPanel('ai')
    nextTick(() => aiStore.sendMessage(prompt))
  })

  // 密码库空闲自动锁定：后端 watchdog 清空内存主密码后提示用户
  onEvent(EVENTS.VAULT_AUTO_LOCKED, () => {
    useToastStore().show('密码库已因空闲自动锁定', 'info', 5000)
  })

  // 启动查版本提示（R4-4 先行版）：只提示不安装；后端 fail-silent，
  // null = 无新版/查询失败/WAF 拦截，一律静默不打扰
  invoke<{ currentVersion: string; latestVersion: string; releaseUrl: string } | null>(
    'check_for_update',
  )
    .then((u) => {
      if (!u) return
      useToastStore().show(t('update.available', { latest: u.latestVersion, current: u.currentVersion }), 'info', 8000)
    })
    .catch(() => {})
})

onUnmounted(() => {
  window.removeEventListener('keydown', onGlobalKeydown)
  tabsStore.stopPolling()
  if (saveTimer) clearTimeout(saveTimer)
  // 兜底锁屏轮询必须停掉（监听器由 useManagedListeners 自动解绑）
  if (lockTimer) clearInterval(lockTimer)
  lockTimer = null
})

// 标签变化时自动保存会话（防抖 3 秒，按当前工作区保存）
watch(
  () => tabsStore.tabs.map((t) => `${t.id}:${t.url}`).join('|'),
  () => {
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      tabsStore.saveCurrentSession(workspacesStore.currentId)
    }, 3000)
  },
)
</script>

<template>
  <div
    class="app"
    :class="{ 'theme-light': settingsStore.isLight, 'vertical-tabs': verticalTabs }"
    :style="layoutCssVars"
  >
    <!-- 主体：垂直标签时「左侧栏 + 主列」，横向时只有主列 -->
    <div class="app-body">
      <VerticalTabBar v-if="verticalTabs" @toggle="onToggleVerticalTabs" />
      <div class="app-main" role="main">
        <!-- 视觉隐藏的文档级 h1（axe page-has-heading-one / landmark-one-main） -->
        <h1 class="sr-only">ShiftBrowser</h1>
        <TabBar v-if="!verticalTabs" />
        <AddressBar
          ref="addressBarRef"
          @open-passwords="openPanel('passwords')"
          @open-groups="openPanel('groups')"
          @open-downloads="openPanel('downloads')"
          @open-settings="openPanel('settings')"
          @open-ai="openPanel('ai')"
          @open-translate="openPanel('translate')"
          @open-userscripts="openPanel('userscripts')"
          @open-history="openPanel('history')"
          @open-certs="showCerts = true"
          @open-privacy="openSettings('privacy#shield')"
        />
        <BookmarksBar v-if="settingsStore.showBookmarksBar" @open-manager="showBookmarks = true" />
        <!-- 内容区：webview 由 Rust 按内容区几何盖住此区域（top 随书签栏/垂直标签变化） -->
        <!-- 菜单底垫：弹菜单隐藏 webview 时垫一张当前页快照，不露深蓝兜底 -->
        <div class="content-area" id="contentArea">
          <div
            v-if="backdropStore.image"
            class="page-backdrop"
            :style="{ backgroundImage: `url(${backdropStore.image})` }"
          ></div>
        </div>
        <StatusBar
          @open-downloads="showDownloads = true"
          @open-settings="openSettings()"
          @open-privacy="openSettings('privacy#shield')"
        />
      </div>
    </div>
    <PasswordsDialog :show="showPasswords" @close="showPasswords = false" />
    <GroupsDialog :show="showGroups" @close="showGroups = false" />
    <DownloadsDialog :show="showDownloads" @close="showDownloads = false" />
    <SettingsDialog
      :show="showSettings"
      :initial-section="settingsInitialSection"
      @close="showSettings = false"
      @open-certs="showCerts = true"
      @open-userscripts="openPanel('userscripts')"
    />
    <AISidebar :show="showAI" @close="showAI = false" />
    <TranslatePanel :show="showTranslate" @close="showTranslate = false" />
    <HistoryDialog :show="showHistory" @close="showHistory = false" />
    <BookmarksDialog :show="showBookmarks" @close="showBookmarks = false" />
    <SessionDialog :show="showSessions" @close="showSessions = false" />
    <UserscriptsDialog :show="showUserscripts" @close="showUserscripts = false" />
    <CertificatesDialog :show="showCerts" @close="showCerts = false" />
    <CertWarning :info="tlsError" @close="shiftTlsError" />
    <PermissionPrompt :info="permReq" @close="shiftPermReq" />
    <CommandPalette
      :show="showCommandPalette"
      @close="showCommandPalette = false"
      @command="onPaletteCommand"
      @ask-ai="onPaletteAskAi"
    />
    <AppToast />
    <!-- 启动锁屏已迁移到 Rust/GTK 层（lib.rs），不在此处重复实现 -->
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg-app);
  overflow: hidden;
}
/* 主体：横向标签时仅一个主列；垂直标签时左侧栏 + 主列 */
.app-body {
  flex: 1;
  display: flex;
  flex-direction: row;
  min-height: 0;
}
.app-main {
  flex: 1;
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}
.content-area {
  flex: 1;
  position: relative;
  background: var(--bg-app);
}
/* 菜单底垫：当前页快照铺满内容区（快照尺寸=隐藏瞬间的内容区尺寸） */
.page-backdrop {
  position: absolute;
  inset: 0;
  background-size: 100% 100%;
  background-repeat: no-repeat;
  pointer-events: none;
}
</style>

<style>
/* 主题变量：深色（默认）+ 浅色（.theme-light 覆盖）
   各组件 scoped 样式引用 var(--xxx)，实现全局深浅色切换 */
:root {
  /* 应用底色（内容区兜底，生产环境由原生 webview 覆盖） */
  --bg-app: linear-gradient(135deg, #0b1222 0%, #0f1830 52%, #14233f 100%);
  /* Chrome 色阶：primary 条带底 → secondary 控件底 → hover → active 选中态 */
  --bg-primary: #0e1626;
  --bg-secondary: #162033;
  --bg-hover: #1d2942;
  --bg-active: #283a61;
  /* 浮层表面：弹窗 / 菜单 / 下拉，比条带略亮以建立仰角 */
  --bg-elevated: #1a2438;
  --bg-elevated-2: #202c44;
  --text-primary: #e9eef8;
  --text-secondary: #bcc8dd;
  --text-muted: #8a97b0;
  --text-faint: #5f6b83;
  --border-color: rgba(255, 255, 255, 0.13);
  --border-subtle: rgba(255, 255, 255, 0.07);
  --hairline: rgba(255, 255, 255, 0.06);
  --accent: #5b8cff;
  --accent-2: #7c5cff;
  --accent-hover: #6e9bff;
  --accent-press: #4778e6;
  --accent-contrast: #ffffff;
  --accent-soft: rgba(91, 140, 255, 0.16);
  --accent-ring: rgba(91, 140, 255, 0.38);
  --grad-accent: linear-gradient(135deg, #5b8cff, #7c5cff);
  --success: #34c759;
  --success-soft: rgba(52, 199, 89, 0.14);
  --warning: #f5a623;
  --warning-soft: rgba(245, 166, 35, 0.14);
  --danger: #ff605c;
  --danger-soft: rgba(255, 96, 92, 0.14);
  --scrim: rgba(7, 11, 22, 0.56);
  --shadow-pop: 0 8px 28px rgba(0, 0, 0, 0.42), 0 1px 3px rgba(0, 0, 0, 0.3);
  --shadow-dialog: 0 24px 64px rgba(0, 0, 0, 0.5), 0 2px 8px rgba(0, 0, 0, 0.3);
}
.theme-light {
  --bg-app: linear-gradient(135deg, #eef2f8 0%, #f4f6fa 50%, #e9eef7 100%);
  --bg-primary: #e9edf4;
  --bg-secondary: #ffffff;
  --bg-hover: #e6ebf3;
  --bg-active: #dde5f2;
  --bg-elevated: #ffffff;
  --bg-elevated-2: #f4f6fa;
  --text-primary: #1b2333;
  --text-secondary: #46506a;
  --text-muted: #6b768c;
  --text-faint: #97a0b3;
  --border-color: #d2d9e5;
  --border-subtle: #e3e8f0;
  --hairline: #eaedf3;
  --accent: #3a7afe;
  --accent-2: #6c4dff;
  --accent-hover: #5a90ff;
  --accent-press: #2f69df;
  --accent-contrast: #ffffff;
  --accent-soft: rgba(58, 122, 254, 0.12);
  --accent-ring: rgba(58, 122, 254, 0.3);
  --grad-accent: linear-gradient(135deg, #3a7afe, #6c4dff);
  --success: #15a34a;
  --success-soft: rgba(21, 163, 74, 0.1);
  --warning: #d97706;
  --warning-soft: rgba(217, 119, 6, 0.12);
  --danger: #dc2626;
  --danger-soft: rgba(220, 38, 38, 0.1);
  --scrim: rgba(17, 24, 39, 0.32);
  --shadow-pop: 0 8px 24px rgba(31, 41, 59, 0.14), 0 1px 2px rgba(31, 41, 59, 0.08);
  --shadow-dialog: 0 24px 60px rgba(31, 41, 59, 0.22), 0 2px 6px rgba(31, 41, 59, 0.08);
}

/* 全局按钮反馈 */
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

/* 输入类控件占位文字：深色主题下必须可见（默认浏览器灰太暗） */
::placeholder {
  color: var(--text-muted);
  opacity: 0.85;
}
button:active:not(:disabled) {
  transform: scale(0.96);
}

/* ===== 弹窗公共样式（各弹窗组件共用，组件内仅覆盖宽度差异） ===== */
.dialog-overlay {
  position: fixed;
  inset: 0;
  background: var(--scrim);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: var(--z-dropdown);
}
.dialog {
  width: min(var(--dialog-md), calc(100vw - 32px));
  max-height: 80vh;
  background: var(--bg-elevated);
  border-radius: var(--radius-lg);
  border: 1px solid var(--border-subtle);
  box-shadow: var(--shadow-dialog);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: var(--space-lg) var(--space-xl);
  color: var(--text-primary);
  font-size: var(--text-md);
  font-weight: 600;
  letter-spacing: 0.01em;
  border-bottom: 1px solid var(--hairline);
  flex-shrink: 0;
}
.close-btn {
  width: 26px;
  height: 26px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 13px;
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.close-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.dialog-body {
  padding: var(--space-lg) var(--space-xl);
  overflow-y: auto;
}

/* 弹窗/侧栏开关动画（时长/缓动统一走令牌） */
.fade-enter-active,
.fade-leave-active {
  transition: opacity var(--dur) var(--ease);
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
.slide-enter-active,
.slide-leave-active {
  transition:
    transform var(--dur-slow) var(--ease-out),
    opacity var(--dur-slow) var(--ease-out);
}
.slide-enter-from,
.slide-leave-to {
  transform: translateX(40px);
  opacity: 0;
}

/* 通用加载转圈（弹窗/AI 思考共用） */
.spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--border-color);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin 0.7s linear infinite;
  display: inline-block;
  vertical-align: middle;
}
@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

/* 视觉隐藏但读屏可见（文档级 h1 等 landmark 用） */
.sr-only {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  padding: 0;
  overflow: hidden;
  clip: rect(0 0 0 0);
  white-space: nowrap;
  border: 0;
}
</style>

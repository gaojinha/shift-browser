<script setup lang="ts">
// 浏览器主窗口布局
// 结构：TabBar(38) + AddressBar(36) + BookmarksBar(26) + 内容区(webview) + StatusBar(22)
import { computed, defineAsyncComponent, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { invokeOk } from './utils/invoke'
import { listen } from '@tauri-apps/api/event'
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
// 布局常量单一来源（build.rs 由 src-tauri/src/constants.rs 生成）→ 注入 CSS 变量
import { EVENTS, LAYOUT, SHORTCUTS } from './constants'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()
const settingsStore = useSettingsStore()
const workspacesStore = useWorkspacesStore()
const aiStore = useAIStore()

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
const tlsError = ref<TlsErrorInfo | null>(null)

function openPanel(name: 'translate' | 'sessions' | 'history' | 'passwords' | 'groups' | 'downloads' | 'settings' | 'ai' | 'bookmarks' | 'userscripts') {
  showTranslate.value = name === 'translate'
  showSessions.value = name === 'sessions'
  showHistory.value = name === 'history'
  showPasswords.value = name === 'passwords'
  showGroups.value = name === 'groups'
  showDownloads.value = name === 'downloads'
  showSettings.value = name === 'settings'
  showAI.value = name === 'ai'
  showBookmarks.value = name === 'bookmarks'
  showUserscripts.value = name === 'userscripts'
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
let saveTimer: number | null = null

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
    showUserscripts.value
  )
}

/**
 * 统一快捷键动作入口。
 * 两个来源共用：① 前端 keydown（UI 层持焦时）② GTK 窗口层转发（页面持焦时，
 * 前端收不到按键，由 Rust 在 GTK 层拦截后 emit `global-shortcut`）。
 */
function handleGlobalAction(action: string) {
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
      invokeOk('open_devtools').catch(() => {})
      break
    case 'lock-vault':
      // 手动立即锁定密码库（Ctrl+Shift+L）：未设主密码/已锁定时后端为无害空操作
      invoke('lock_vault')
        .then(() => useToastStore().success('密码库已锁定'))
        .catch(() => {})
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

/** 命令栏选中「问 AI」：侧栏已开则直接发问，否则暂存待侧栏打开时消费 */
function onPaletteAskAi(query: string) {
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
      await tabsStore.createTab().catch(() => {})
    }
  }
}

onMounted(async () => {
  window.addEventListener('keydown', onGlobalKeydown)

  // 前端日志桥：关键节点写进 Rust 日志（排查前端白屏/事件收不到/初始化不执行）
  function frontLog(msg: string) {
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
    try {
      await listen(EVENTS.APP_UNLOCKED, () => {
        frontLog('收到 app-unlocked 事件')
        onReady(true)
      })
      frontLog('已注册 app-unlocked 监听')
    } catch (e) {
      frontLog('注册 app-unlocked 失败: ' + e)
    }
    // ③ 兜底轮询：万一时序问题导致事件丢失，每秒查一次锁屏状态，解开就初始化
    const lockTimer = window.setInterval(async () => {
      if (ready) {
        clearInterval(lockTimer)
        return
      }
      try {
        const l = await invoke<boolean>('is_locked')
        if (!l) {
          frontLog('兜底轮询检测到已解锁')
          clearInterval(lockTimer)
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
  listen(EVENTS.OPEN_AI_FROM_NEWTAB, () => {
    showAI.value = true
  }).catch(() => {})
  listen(EVENTS.OPEN_SETTINGS_FROM_NEWTAB, () => {
    showSettings.value = true
  }).catch(() => {})
  // 启动页资讯点击 → 左窗格保持列表页，右窗格分屏打开资讯原文（占满右半区域）
  listen<string>(EVENTS.OPEN_URL_FROM_NEWTAB, async (e) => {
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
  }).catch(() => {})

  // 系统托盘菜单 → 打开面板（Rust 侧 emit，见 tray.rs）
  listen(EVENTS.TRAY_OPEN_DOWNLOADS, () => {
    openPanel('downloads')
  }).catch(() => {})
  listen(EVENTS.TRAY_OPEN_PASSWORDS, () => {
    openPanel('passwords')
  }).catch(() => {})

  // 证书错误（P2 · Rust 侧 on_tls_error emit）→ 弹"危险站点"拦截页
  listen<TlsErrorInfo>(EVENTS.TLS_ERROR, (ev) => {
    tlsError.value = ev.payload
  }).catch(() => {})

  // 权限请求（安全 #2 · Rust 侧 perms::attach emit）→ 弹权限提示
  listen<PermissionRequest>(EVENTS.PERMISSION_REQUEST, (ev) => {
    permReq.value = ev.payload
  }).catch(() => {})

  // Web 进程崩溃（视频崩溃修复 · Rust 侧 connect_web_process_terminated emit）→ 提示
  listen<WebProcessCrash>(EVENTS.WEB_PROCESS_CRASHED, (ev) => {
    const p = ev.payload ?? {}
    const reason = p.reason === 'ExceededMemoryLimit' ? '内存超限' : '进程崩溃'
    useToastStore().show(
      p.autoReload ? `网页${reason}，已自动重新加载` : `网页${reason}，请手动刷新`,
      'error',
      4500,
    )
  }).catch(() => {})

  // GTK 窗口层转发的全局快捷键：页面（child webview）持焦时，前端 keydown 收不到按键，
  // 由 Rust 在 GTK 顶层窗口拦截后发事件过来（见 tabs.rs 的 UI_FOCUSED 焦点跟踪）。
  listen<string>(EVENTS.GLOBAL_SHORTCUT, (ev) => {
    handleGlobalAction(ev.payload)
  }).catch(() => {})

  // 定时 Agent 提示词（阶段3 Agent 编排）：调度器到点发事件 → 打开 AI 侧栏并发送
  listen<string>(EVENTS.AUTO_AGENT, (ev) => {
    const prompt = ev.payload
    if (!prompt) return
    openPanel('ai')
    nextTick(() => aiStore.sendMessage(prompt))
  }).catch(() => {})

  // 密码库空闲自动锁定：后端 watchdog 清空内存主密码后提示用户
  listen(EVENTS.VAULT_AUTO_LOCKED, () => {
    useToastStore().show('密码库已因空闲自动锁定', 'info', 5000)
  }).catch(() => {})
})

onUnmounted(() => {
  window.removeEventListener('keydown', onGlobalKeydown)
  tabsStore.stopPolling()
  if (saveTimer) clearTimeout(saveTimer)
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
      <div class="app-main">
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
        />
        <BookmarksBar v-if="settingsStore.showBookmarksBar" @open-manager="showBookmarks = true" />
        <!-- 内容区：webview 由 Rust 按内容区几何盖住此区域（top 随书签栏/垂直标签变化） -->
        <div class="content-area" id="contentArea"></div>
        <StatusBar
          @open-downloads="showDownloads = true"
          @open-settings="openSettings()"
          @open-privacy="openSettings('privacy')"
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
    />
    <AISidebar :show="showAI" @close="showAI = false" />
    <TranslatePanel :show="showTranslate" @close="showTranslate = false" />
    <HistoryDialog :show="showHistory" @close="showHistory = false" />
    <BookmarksDialog :show="showBookmarks" @close="showBookmarks = false" />
    <SessionDialog :show="showSessions" @close="showSessions = false" />
    <UserscriptsDialog :show="showUserscripts" @close="showUserscripts = false" />
    <CertificatesDialog :show="showCerts" @close="showCerts = false" />
    <CertWarning :info="tlsError" @close="tlsError = null" />
    <PermissionPrompt :info="permReq" @close="permReq = null" />
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
  background: linear-gradient(135deg, #0d1220 0%, #111828 50%, #16233c 100%);
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
  background: linear-gradient(135deg, #0d1220 0%, #111828 50%, #16233c 100%);
}
</style>

<style>
/* 主题变量：深色（默认）+ 浅色（.theme-light 覆盖）
   各组件 scoped 样式引用 var(--xxx)，实现全局深浅色切换 */
:root {
  --bg-primary: #0f1522;
  --bg-secondary: #182033;
  --bg-hover: #1f2940;
  --bg-active: #283450;
  --text-primary: #e6ebf4;
  --text-secondary: #c6d0df;
  --text-muted: #9aa8bd;
  --border-color: rgba(255, 255, 255, 0.14);
  --border-subtle: rgba(255, 255, 255, 0.09);
  --accent: #5b8cff;
  --accent-2: #7c5cff;
  --accent-soft: rgba(91, 140, 255, 0.16);
  --grad-accent: linear-gradient(135deg, #5b8cff, #7c5cff);
  --success: #34c759;
  --danger: #e53935;
}
.theme-light {
  --bg-primary: #f7f8fa;
  --bg-secondary: #ffffff;
  --bg-hover: #eeeef2;
  --bg-active: #e4e6ee;
  --text-primary: #1f2430;
  --text-secondary: #4a5263;
  --text-muted: #6b7488;
  --border-color: #c9ced8;
  --border-subtle: #e0e4eb;
  --accent: #3a7afe;
  --accent-2: #6c4dff;
  --accent-soft: rgba(58, 122, 254, 0.14);
  --grad-accent: linear-gradient(135deg, #3a7afe, #6c4dff);
  --success: #15a34a;
  --danger: #dc2626;
}

/* 全局按钮反馈 */
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

/* 输入类控件占位文字：深色主题下必须可见（默认浏览器灰太暗） */
::placeholder {
  color: var(--text-muted);
  opacity: 0.8;
}
button:active:not(:disabled) {
  transform: scale(0.97);
}

/* ===== 弹窗公共样式（各弹窗组件共用，组件内仅覆盖宽度差异） ===== */
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
  width: min(480px, calc(100vw - 32px));
  max-height: 80vh;
  background: var(--bg-secondary);
  border-radius: 10px;
  border: 1px solid var(--border-color);
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  color: var(--text-primary);
  font-size: 14px;
  border-bottom: 1px solid var(--border-subtle);
}
.close-btn {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 14px;
  cursor: pointer;
}
.close-btn:hover {
  color: var(--text-primary);
}
.dialog-body {
  padding: 12px 16px;
  overflow-y: auto;
}

/* 弹窗/侧栏开关动画 */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.18s ease;
}
.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
.slide-enter-active,
.slide-leave-active {
  transition:
    transform 0.22s ease,
    opacity 0.22s ease;
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
</style>

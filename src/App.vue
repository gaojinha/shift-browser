<script setup lang="ts">
// 浏览器主窗口布局
// 结构：TabBar(40) + AddressBar(42) + BookmarksBar(30) + 内容区(webview) + StatusBar(22)
import { onMounted, onUnmounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import TabBar from './components/TabBar.vue'
import AddressBar from './components/AddressBar.vue'
import BookmarksBar from './components/BookmarksBar.vue'
import StatusBar from './components/StatusBar.vue'
import PasswordsDialog from './components/PasswordsDialog.vue'
import GroupsDialog from './components/GroupsDialog.vue'
import DownloadsDialog from './components/DownloadsDialog.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import AISidebar from './components/AISidebar.vue'
import TranslatePanel from './components/TranslatePanel.vue'
import HistoryDialog from './components/HistoryDialog.vue'
import BookmarksDialog from './components/BookmarksDialog.vue'
import SessionDialog from './components/SessionDialog.vue'
import AppToast from './components/AppToast.vue'
import { useTabsStore, setLockedState } from './stores/tabs'
import { useGroupsStore } from './stores/groups'
import { useSettingsStore } from './stores/settings'
import { useWorkspacesStore } from './stores/workspaces'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()
const settingsStore = useSettingsStore()
const workspacesStore = useWorkspacesStore()
const showPasswords = ref(false)
const showGroups = ref(false)
const showDownloads = ref(false)
const showSettings = ref(false)
const showAI = ref(false)

function openPanel(name: 'translate' | 'sessions' | 'history' | 'passwords' | 'groups' | 'downloads' | 'settings' | 'ai' | 'bookmarks') {
  showTranslate.value = name === 'translate'
  showSessions.value = name === 'sessions'
  showHistory.value = name === 'history'
  showPasswords.value = name === 'passwords'
  showGroups.value = name === 'groups'
  showDownloads.value = name === 'downloads'
  showSettings.value = name === 'settings'
  showAI.value = name === 'ai'
  showBookmarks.value = name === 'bookmarks'
}

const showTranslate = ref(false)
const showHistory = ref(false)
const showBookmarks = ref(false)
const showSessions = ref(false)
const addressBarRef = ref<InstanceType<typeof AddressBar> | null>(null)
let saveTimer: number | null = null

/** Esc 关闭最上层弹窗（AI 侧栏优先，再设置/下载/分组/密码）；Ctrl 组合键为浏览器标配快捷键 */
function onGlobalKeydown(e: KeyboardEvent) {
  // Ctrl+Tab / Ctrl+Shift+Tab：切换前后标签
  if (e.ctrlKey && e.key === 'Tab') {
    e.preventDefault()
    const tabs = tabsStore.tabs
    if (tabs.length < 2) return
    const idx = tabs.findIndex((t) => t.id === tabsStore.activeTabId)
    const next = e.shiftKey ? (idx - 1 + tabs.length) % tabs.length : (idx + 1) % tabs.length
    tabsStore.switchTab(tabs[next].id)
    return
  }

  // Ctrl 组合快捷键（不拦截无 Ctrl 的普通按键）
  if (e.ctrlKey && !e.altKey && !e.metaKey) {
    const k = e.key.toLowerCase()
    // Ctrl+L 聚焦地址栏（任何情况下都生效，含输入框内）
    if (k === 'l') {
      e.preventDefault()
      addressBarRef.value?.focusInput()
      return
    }
    // 弹窗打开时，其余标签快捷键不响应（避免误关标签/新建标签）
    if (showTranslate.value || showAI.value || showSettings.value || showDownloads.value || showGroups.value || showPasswords.value || showHistory.value || showSessions.value || showBookmarks.value) {
      return
    }
    // Ctrl+Shift+T 打开翻译面板（不影响 Ctrl+T 建标签）
    if (k === 't' && e.shiftKey) {
      e.preventDefault()
      openPanel('translate')
      return
    }
    // Ctrl+Shift+S 打开会话管理
    if (k === 's' && e.shiftKey) {
      e.preventDefault()
      openPanel('sessions')
      return
    }
    if (k === 't' && !e.shiftKey) {
      e.preventDefault()
      tabsStore.createTab()
    } else if (k === 'w') {
      e.preventDefault()
      if (tabsStore.activeTabId) tabsStore.closeTab(tabsStore.activeTabId)
    } else if (k === 'r') {
      e.preventDefault()
      if (tabsStore.activeTabId) tabsStore.reloadTab(tabsStore.activeTabId)
    } else if (k === 'h') {
      e.preventDefault()
      openPanel('history')
    }
    return
  }

   if (e.key !== 'Escape') return
  if (showTranslate.value) {
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
    await invoke('set_bookmarks_bar_visible', { visible: settingsStore.showBookmarksBar })
  } catch (e) {
    console.error('[app] 书签栏初始化同步失败:', e)
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
    frontLog('onReady 完成 标签数=' + tabsStore.tabs.length)
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
      await listen('app-unlocked', () => {
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
  listen('open-ai-from-newtab', () => {
    showAI.value = true
  }).catch(() => {})
  listen('open-settings-from-newtab', () => {
    showSettings.value = true
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
  <div class="app" :class="{ 'theme-light': settingsStore.isLight }">
    <TabBar />
    <AddressBar
      ref="addressBarRef"
      @open-passwords="openPanel('passwords')"
      @open-groups="openPanel('groups')"
      @open-downloads="openPanel('downloads')"
      @open-settings="openPanel('settings')"
      @open-ai="openPanel('ai')"
      @open-translate="openPanel('translate')"
    />
    <BookmarksBar v-if="settingsStore.showBookmarksBar" @open-manager="showBookmarks = true" />
    <!-- 内容区：webview 由 Rust 以 (0, 112) 起盖住此区域 -->
    <div class="content-area" id="contentArea"></div>
    <StatusBar
      @open-downloads="showDownloads = true"
      @open-settings="showSettings = true"
    />
    <PasswordsDialog :show="showPasswords" @close="showPasswords = false" />
    <GroupsDialog :show="showGroups" @close="showGroups = false" />
    <DownloadsDialog :show="showDownloads" @close="showDownloads = false" />
    <SettingsDialog :show="showSettings" @close="showSettings = false" />
    <AISidebar :show="showAI" @close="showAI = false" />
    <TranslatePanel :show="showTranslate" @close="showTranslate = false" />
    <HistoryDialog :show="showHistory" @close="showHistory = false" />
    <BookmarksDialog :show="showBookmarks" @close="showBookmarks = false" />
    <SessionDialog :show="showSessions" @close="showSessions = false" />
    <AppToast />
    <!-- 启动锁屏已迁移到 Rust/GTK 层（lib.rs），不在此处重复实现 -->
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
  overflow: hidden;
}
.content-area {
  flex: 1;
  position: relative;
  background: linear-gradient(135deg, #1a1a2e 0%, #16213e 50%, #0f3460 100%);
}
</style>

<style>
/* 主题变量：深色（默认）+ 浅色（.theme-light 覆盖）
   各组件 scoped 样式引用 var(--xxx)，实现全局深浅色切换 */
:root {
  --bg-primary: #16213e;
  --bg-secondary: #1e2a45;
  --bg-hover: #26345a;
  --bg-active: #2b3a5e;
  --text-primary: #e0e0e0;
  --text-secondary: #ccc;
  --text-muted: #9aa5b8;
  --border-color: rgba(255, 255, 255, 0.12);
  --border-subtle: rgba(255, 255, 255, 0.08);
  --accent: #4a9eff;
  --success: #43a047;
  --danger: #e53935;
}
.theme-light {
  --bg-primary: #f5f5f5;
  --bg-secondary: #ffffff;
  --bg-hover: #ececec;
  --bg-active: #e0e0e0;
  --text-primary: #333333;
  --text-secondary: #444444;
  --text-muted: #888888;
  --border-color: #cccccc;
  --border-subtle: #dddddd;
  --accent: #1a73e8;
  --success: #2e7d32;
  --danger: #c62828;
}

/* 全局按钮反馈 */
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
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

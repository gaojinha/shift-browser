<script setup lang="ts">
// 浏览器主窗口布局
// 结构：TabBar(40) + AddressBar(42) + BookmarksBar(30) + 内容区(webview) + StatusBar(22)
import { onMounted, onUnmounted, ref, watch } from 'vue'
import TabBar from './components/TabBar.vue'
import AddressBar from './components/AddressBar.vue'
import BookmarksBar from './components/BookmarksBar.vue'
import StatusBar from './components/StatusBar.vue'
import PasswordsDialog from './components/PasswordsDialog.vue'
import GroupsDialog from './components/GroupsDialog.vue'
import DownloadsDialog from './components/DownloadsDialog.vue'
import SettingsDialog from './components/SettingsDialog.vue'
import { useTabsStore } from './stores/tabs'
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
let saveTimer: number | null = null

onMounted(async () => {
  // 设置尽早加载（主页/搜索引擎/主题/书签栏显隐依赖）
  settingsStore.loadSettings().catch(() => {})
  // 分组尽早加载（不等标签恢复，避免右键菜单/组头迟迟不显示）
  groupsStore.loadGroups().catch(() => {})
  // 工作区列表加载（当前工作区 = 默认）
  await workspacesStore.loadWorkspaces()
  // 启动时恢复标签状态
  await tabsStore.loadTabs()
  if (tabsStore.tabs.length === 0) {
    // 无标签：先尝试恢复当前工作区最近会话，失败则创建默认标签
    const restored = await tabsStore.restoreLastSession(workspacesStore.currentId)
    if (!restored) {
      await tabsStore.createTab()
    }
  }
  // 启动标题/URL 轮询（Rust 侧 on_page_load 更新，前端定时同步）
  tabsStore.startPolling()
})

onUnmounted(() => {
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
  }
)
</script>

<template>
  <div class="app" :class="{ 'theme-light': settingsStore.isLight }">
    <TabBar />
    <AddressBar
      @open-passwords="showPasswords = true"
      @open-groups="showGroups = true"
      @open-downloads="showDownloads = true"
      @open-settings="showSettings = true"
    />
    <BookmarksBar v-if="settingsStore.showBookmarksBar" />
    <!-- 内容区：webview 由 Rust 以 (0, 112) 起盖住此区域 -->
    <div class="content-area" id="contentArea"></div>
    <StatusBar />
    <PasswordsDialog :show="showPasswords" @close="showPasswords = false" />
    <GroupsDialog :show="showGroups" @close="showGroups = false" />
    <DownloadsDialog :show="showDownloads" @close="showDownloads = false" />
    <SettingsDialog :show="showSettings" @close="showSettings = false" />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: var(--bg-primary, #1a1a1a);
  overflow: hidden;
}
.content-area {
  flex: 1;
  position: relative;
  background: var(--bg-primary, #1a1a1a);
}
</style>

<style>
/* 主题变量：深色（默认）+ 浅色（.theme-light 覆盖）
   各组件 scoped 样式引用 var(--xxx)，实现全局深浅色切换 */
:root {
  --bg-primary: #1a1a1a;
  --bg-secondary: #2d2d2d;
  --bg-hover: #383838;
  --bg-active: #3d3d3d;
  --text-primary: #e0e0e0;
  --text-secondary: #ccc;
  --text-muted: #999;
  --border-color: #444;
  --border-subtle: #333;
  --accent: #4a9eff;
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
}
</style>
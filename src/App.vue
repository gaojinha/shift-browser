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
import { useTabsStore } from './stores/tabs'
import { useGroupsStore } from './stores/groups'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()
const showPasswords = ref(false)
const showGroups = ref(false)
const showDownloads = ref(false)
let saveTimer: number | null = null

onMounted(async () => {
  // 分组尽早加载（不等标签恢复，避免右键菜单/组头迟迟不显示）
  groupsStore.loadGroups().catch(() => {})
  // 启动时恢复标签状态
  await tabsStore.loadTabs()
  if (tabsStore.tabs.length === 0) {
    // 无标签：先尝试恢复最近会话，失败则创建默认标签
    const restored = await tabsStore.restoreLastSession()
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

// 标签变化时自动保存会话（防抖 3 秒）
watch(
  () => tabsStore.tabs.map((t) => `${t.id}:${t.url}`).join('|'),
  () => {
    if (saveTimer) clearTimeout(saveTimer)
    saveTimer = window.setTimeout(() => {
      tabsStore.saveCurrentSession()
    }, 3000)
  }
)
</script>

<template>
  <div class="app">
    <TabBar />
    <AddressBar
      @open-passwords="showPasswords = true"
      @open-groups="showGroups = true"
      @open-downloads="showDownloads = true"
    />
    <BookmarksBar />
    <!-- 内容区：webview 由 Rust 以 (0, 112) 起盖住此区域 -->
    <div class="content-area" id="contentArea"></div>
    <StatusBar />
    <PasswordsDialog :show="showPasswords" @close="showPasswords = false" />
    <GroupsDialog :show="showGroups" @close="showGroups = false" />
    <DownloadsDialog :show="showDownloads" @close="showDownloads = false" />
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
  background: #1a1a1a;
  overflow: hidden;
}
.content-area {
  flex: 1;
  position: relative;
  background: #1a1a1a;
}
</style>
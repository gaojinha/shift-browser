<script setup lang="ts">
// 状态栏：显示当前 URL + 标签数 + 版本
import { computed } from 'vue'
import { useTabsStore } from '../stores/tabs'

const tabsStore = useTabsStore()

const currentUrl = computed(() => tabsStore.activeTab?.url ?? '')
const tabCount = computed(() => tabsStore.tabs.length)
</script>

<template>
  <div class="status-bar">
    <span class="status-left">{{ currentUrl }}</span>
    <span class="status-right">{{ tabCount }} 标签 · v5.0.0</span>
  </div>
</template>

<style scoped>
.status-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 22px;
  background: var(--bg-primary);
  border-top: 1px solid var(--border-subtle);
  padding: 0 12px;
  flex-shrink: 0;
  user-select: none;
}
.status-left {
  color: var(--text-muted);
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0; /* flexbox 默认 min-width:auto 会让长 URL 撑满空间、把 .status-right 挤出可视区 */
  margin-right: 12px;
}
.status-right {
  color: var(--text-muted);
  font-size: 11px;
  flex-shrink: 0;
}
</style>
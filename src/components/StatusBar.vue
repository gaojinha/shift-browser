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
  background: #1a1a1a;
  border-top: 1px solid #333;
  padding: 0 12px;
  flex-shrink: 0;
  user-select: none;
}
.status-left {
  color: #888;
  font-size: 11px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  margin-right: 12px;
}
.status-right {
  color: #666;
  font-size: 11px;
  flex-shrink: 0;
}
</style>
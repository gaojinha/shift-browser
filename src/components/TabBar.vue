<script setup lang="ts">
// 标签栏（对应 Electron 版 .tab-bar 设计）
import { useTabsStore } from '../stores/tabs'

const tabsStore = useTabsStore()

// 默认网站图标（蓝色地球 SVG，与 Electron 版一致）
const DEFAULT_FAVICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="#4a9eff"/><ellipse cx="8" cy="8" rx="3" ry="6.5" fill="none" stroke="#fff" stroke-width="0.8"/><path d="M1.5 8h13" stroke="#fff" stroke-width="0.8"/></svg>'
  )

function onTabClick(id: string) {
  if (id !== tabsStore.activeTabId) {
    tabsStore.switchTab(id)
  }
}

function onTabClose(e: MouseEvent, id: string) {
  e.stopPropagation()
  tabsStore.closeTab(id)
}

function onNewTab() {
  tabsStore.createTab()
}

/** favicon 加载失败时移除元素（避免破图） */
function onFaviconError(e: Event) {
  const el = e.target as HTMLImageElement | null
  el?.remove()
}
</script>

<template>
  <div class="tab-bar">
    <div class="tab-scroll">
      <div
        v-for="tab in tabsStore.tabs"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned }"
        @click="onTabClick(tab.id)"
      >
        <img
          v-if="tab.favicon"
          class="favicon"
          :src="tab.favicon"
          alt=""
          @error="onFaviconError"
        />
        <img v-else-if="!tab.is_pinned" class="favicon favicon-default" :src="DEFAULT_FAVICON" alt="" />
        <span v-else class="pin-icon" title="已固定">📌</span>
        <span class="title">{{ tab.title || '启动页' }}</span>
        <button class="close-btn" title="关闭标签" @click="onTabClose($event, tab.id)">×</button>
      </div>
    </div>
    <button class="new-tab-btn" title="新建标签" @click="onNewTab">+</button>
  </div>
</template>

<style scoped>
.tab-bar {
  display: flex;
  align-items: center;
  height: 40px;
  background: #1a1a1a;
  padding: 0 8px;
  flex-shrink: 0;
  user-select: none;
}
.tab-scroll {
  display: flex;
  align-items: center;
  flex: 1;
  overflow-x: auto;
  height: 100%;
}
.tab-scroll::-webkit-scrollbar {
  display: none;
}
.tab {
  position: relative;
  display: flex;
  align-items: center;
  min-width: 180px;
  max-width: 180px;
  width: 180px;
  flex: 0 0 180px;
  height: 32px;
  padding: 0 10px;
  margin-right: 3px;
  background: #2d2d2d;
  border-radius: 8px 8px 0 0;
  border: 1px solid transparent;
  cursor: pointer;
  transition: background 0.2s, box-shadow 0.2s, color 0.2s, border-color 0.2s;
}
.tab:hover {
  background: #383838;
}
.tab.active {
  background: #3d3d3d;
  border-color: #3d3d3d;
  box-shadow: inset 0 3px 0 #1a73e8;
}
.tab .favicon {
  width: 16px;
  height: 16px;
  border-radius: 3px;
  margin-right: 6px;
  flex-shrink: 0;
  object-fit: contain;
}
.tab .title {
  flex: 1;
  font-size: 12px;
  color: #ccc;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tab.active .title {
  color: #fff;
}
.tab .pin-icon {
  font-size: 13px;
  margin-right: 4px;
  flex-shrink: 0;
}
.tab .close-btn {
  display: none;
  width: 16px;
  height: 16px;
  border: none;
  background: transparent;
  color: #999;
  border-radius: 3px;
  cursor: pointer;
  font-size: 14px;
  line-height: 16px;
  margin-left: 4px;
  flex-shrink: 0;
  transition: background 0.15s, color 0.15s;
}
.tab:hover .close-btn {
  display: block;
}
.tab .close-btn:hover {
  background: #e53935;
  color: #fff;
}
.tab.pinned {
  min-width: 32px;
  max-width: 32px;
  width: 32px;
  padding: 0;
  justify-content: center;
  border-radius: 8px;
}
.tab.pinned .favicon {
  margin-right: 0;
}
.tab.pinned .pin-icon {
  margin-right: 0;
}
.new-tab-btn {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: #999;
  border-radius: 6px;
  cursor: pointer;
  font-size: 18px;
  flex-shrink: 0;
  transition: background 0.15s, color 0.15s;
}
.new-tab-btn:hover {
  background: #3d3d3d;
  color: #fff;
}
</style>
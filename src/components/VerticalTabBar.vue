<script setup lang="ts">
// 垂直标签栏（阶段 3.1）：标签栏移到左侧竖排，适配多标签重度使用。
// 开启后 Rust 侧让出左侧 VERTICAL_TAB_BAR_WIDTH，webview 右移（见 engine/layout.rs）。
import { computed } from 'vue'
import { useTabsStore } from '../stores/tabs'
import type { Tab } from '../types/tab'

const emit = defineEmits<{ (e: 'toggle'): void }>()
const tabsStore = useTabsStore()

const DEFAULT_FAVICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="var(--accent)"/></svg>',
  )

// 固定标签永远排最前（与横向标签栏一致）
function pinnedFirst(tabs: Tab[]): Tab[] {
  return [...tabs].sort((a, b) => Number(b.is_pinned) - Number(a.is_pinned))
}
const ordered = computed(() => pinnedFirst(tabsStore.tabs))

function onSelect(id: string) {
  tabsStore.switchTab(id)
}
function onClose(e: Event, id: string) {
  e.stopPropagation()
  tabsStore.closeTab(id)
}
/** 中键点击关闭标签 */
function onAux(e: MouseEvent, id: string) {
  if (e.button === 1) {
    e.preventDefault()
    tabsStore.closeTab(id)
  }
}
function onNew() {
  tabsStore.createTab()
}
</script>

<template>
  <div class="vtab-bar">
    <div class="vtab-head">
      <button class="vtab-btn" title="新建标签页" @click="onNew">＋</button>
      <button class="vtab-btn" title="切换回横向标签栏" @click="emit('toggle')">⇤ 横向</button>
    </div>
    <div class="vtab-list">
      <div
        v-for="t in ordered"
        :key="t.id"
        class="vtab-item"
        :class="{ active: t.id === tabsStore.activeTabId }"
        :title="t.title || t.url"
        @click="onSelect(t.id)"
        @auxclick="onAux($event, t.id)"
      >
        <img
          class="vtab-fav"
          :src="t.favicon || DEFAULT_FAVICON"
          alt=""
          @error="($event.target as HTMLImageElement).src = DEFAULT_FAVICON"
        />
        <span class="vtab-title">{{ t.title || t.url || '新标签页' }}</span>
        <button class="vtab-close" title="关闭" @click="onClose($event, t.id)">✕</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.vtab-bar {
  width: 220px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-secondary);
  border-right: 1px solid var(--border-subtle);
  overflow: hidden;
}
.vtab-head {
  display: flex;
  gap: 6px;
  padding: 8px;
  border-bottom: 1px solid var(--border-subtle);
}
.vtab-btn {
  flex: 1;
  height: 28px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
}
.vtab-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.vtab-list {
  flex: 1;
  overflow-y: auto;
  padding: 6px;
}
.vtab-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  border-radius: 6px;
  cursor: pointer;
}
.vtab-item:hover {
  background: var(--bg-hover);
}
.vtab-item.active {
  background: var(--bg-active);
}
.vtab-fav {
  width: 16px;
  height: 16px;
  flex-shrink: 0;
}
.vtab-title {
  flex: 1;
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.vtab-close {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  line-height: 16px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11px;
  opacity: 0;
}
.vtab-item:hover .vtab-close {
  opacity: 1;
}
.vtab-close:hover {
  background: var(--danger);
  color: #fff;
}
</style>

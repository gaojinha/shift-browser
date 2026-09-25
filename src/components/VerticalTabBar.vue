<script setup lang="ts">
// 垂直标签栏（阶段 3.1）：标签栏移到左侧竖排，适配多标签重度使用。
// 开启后 Rust 侧让出左侧 VERTICAL_TAB_BAR_WIDTH，webview 右移（见 engine/layout.rs）。
import { computed } from 'vue'
import { invokeOk } from '../utils/invoke'
import { useTabsStore } from '../stores/tabs'
import type { Tab } from '../types/tab'
import AppIcon from './AppIcon.vue'
// 默认网站图标（单一来源：constants.ts，与横向标签栏统一为地球图标）
import { UI } from '../constants'

const emit = defineEmits<{ (e: 'toggle'): void }>()
const tabsStore = useTabsStore()

const DEFAULT_FAVICON = UI.defaultFavicon

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
// 窗口按钮（垂直模式下 TabBar 被隐藏，窗口控制移到这里）
function minimize() {
  invokeOk('minimize_window').catch(() => {})
}
function maximize() {
  invokeOk('toggle_maximize').catch(() => {})
}
function closeWin() {
  invokeOk('close_window').catch(() => {})
}
</script>

<template>
  <div class="vtab-bar">
    <div class="vtab-head">
      <button class="vtab-btn" title="新建标签页" @click="onNew">
        <AppIcon name="plus" :size="13" />
      </button>
      <button class="vtab-btn" title="切换回横向标签栏" @click="emit('toggle')">
        <AppIcon name="layout" :size="13" />
        <span>横向</span>
      </button>
    </div>
    <div class="vtab-winrow">
      <button class="vtab-win" title="最小化" @click="minimize">
        <AppIcon name="minus" :size="12" />
      </button>
      <button class="vtab-win" title="最大化/还原" @click="maximize">
        <AppIcon name="square" :size="12" />
      </button>
      <button class="vtab-win danger" title="关闭窗口" @click="closeWin">
        <AppIcon name="x" :size="12" />
      </button>
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
        <button class="vtab-close" title="关闭" @click="onClose($event, t.id)">
          <AppIcon name="x" :size="10" />
        </button>
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
}
.vtab-winrow {
  display: flex;
  gap: 6px;
  padding: 0 8px 8px;
  border-bottom: 1px solid var(--border-subtle);
}
.vtab-win {
  flex: 1;
  height: 24px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
  display: flex;
  align-items: center;
  justify-content: center;
}
.vtab-win:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.vtab-win.danger:hover {
  background: var(--danger);
  color: #fff;
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
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 4px;
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
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11px;
  opacity: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.vtab-item:hover .vtab-close {
  opacity: 1;
}
.vtab-close:hover {
  background: var(--danger);
  color: #fff;
}
</style>

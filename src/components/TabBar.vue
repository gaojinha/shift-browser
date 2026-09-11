<script setup lang="ts">
// 标签栏（分组版）：未分组标签 + 各分组（颜色条 + 折叠）+ 右键分配分组
import { computed, ref } from 'vue'
import { useTabsStore } from '../stores/tabs'
import { useGroupsStore } from '../stores/groups'
import type { Tab } from '../types/tab'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()

// 默认网站图标（蓝色地球 SVG，与 Electron 版一致）
const DEFAULT_FAVICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="#4a9eff"/><ellipse cx="8" cy="8" rx="3" ry="6.5" fill="none" stroke="#fff" stroke-width="0.8"/><path d="M1.5 8h13" stroke="#fff" stroke-width="0.8"/></svg>'
  )

// 分组后的标签结构
const groupedTabs = computed(() => {
  const ungrouped = tabsStore.tabs.filter((t) => t.groupId == null)
  const groups = groupsStore.groups
    .map((g) => ({
      group: g,
      tabs: tabsStore.tabs.filter((t) => t.groupId === g.id),
    }))
    .filter((g) => g.tabs.length > 0 || true)
  return { ungrouped, groups }
})

// ---- 右键菜单 ----
const menu = ref<{ x: number; y: number; tabId: string } | null>(null)

function onTabContextMenu(e: MouseEvent, tab: Tab) {
  e.preventDefault()
  // 兜底：菜单打开前确保分组已加载（App 启动时 loadGroups 可能还没完成）
  if (groupsStore.groups.length === 0) {
    groupsStore.loadGroups().catch(() => {})
  }
  menu.value = { x: e.clientX, y: e.clientY, tabId: tab.id }
}

function closeMenu() {
  menu.value = null
}

async function onMoveToGroup(groupId: number | null) {
  if (!menu.value) return
  tabsStore.assignTabToGroup(menu.value.tabId, groupId)
  closeMenu()
}

async function onNewGroupFromMenu() {
  if (!menu.value) return
  // WebKitGTK 不显示原生 prompt，直接建默认名分组
  const n = groupsStore.groups.length + 1
  const id = await groupsStore.createGroup(`新分组 ${n}`)
  tabsStore.assignTabToGroup(menu.value.tabId, id)
  closeMenu()
}

// ---- 分组操作 ----
function onToggleCollapse(groupId: number) {
  const g = groupsStore.groups.find((x) => x.id === groupId)
  if (g) groupsStore.updateGroup(groupId, { collapsed: !g.collapsed })
}

// ---- 标签操作 ----
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
  <div class="tab-bar" @click="closeMenu">
    <div class="tab-scroll" @contextmenu.prevent>
      <!-- 未分组标签 -->
      <div
        v-for="tab in groupedTabs.ungrouped"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned }"
        @click="onTabClick(tab.id)"
        @contextmenu="onTabContextMenu($event, tab)"
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

      <!-- 分组 -->
      <template v-for="g in groupedTabs.groups" :key="g.group.id">
        <div class="group-head" :style="{ borderLeftColor: g.group.color }">
          <span class="group-name">{{ g.group.name }}</span>
          <span class="group-count">{{ g.tabs.length }}</span>
          <button class="group-toggle" :title="g.group.collapsed ? '展开' : '折叠'" @click="onToggleCollapse(g.group.id)">
            {{ g.group.collapsed ? '▶' : '▼' }}
          </button>
        </div>
        <template v-if="!g.group.collapsed">
          <div
            v-for="tab in g.tabs"
            :key="tab.id"
            class="tab"
            :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned }"
            @click="onTabClick(tab.id)"
            @contextmenu="onTabContextMenu($event, tab)"
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
        </template>
      </template>
    </div>
    <button class="new-tab-btn" title="新建标签" @click="onNewTab">+</button>

    <!-- 右键菜单 -->
    <div
      v-if="menu"
      class="context-menu"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
      @click.stop
    >
      <div class="menu-title">移动到分组</div>
      <div
        v-for="g in groupsStore.groups"
        :key="g.id"
        class="menu-item"
        @click="onMoveToGroup(g.id)"
      >
        <span class="menu-dot" :style="{ background: g.color }"></span>{{ g.name }}
      </div>
      <div class="menu-item" @click="onMoveToGroup(null)">取消分组</div>
      <div class="menu-divider"></div>
      <div class="menu-item accent" @click="onNewGroupFromMenu">＋ 新建分组并移动</div>
    </div>
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
  position: relative;
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

/* 分组头 */
.group-head {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 32px;
  padding: 0 8px;
  margin-right: 3px;
  background: #242424;
  border-radius: 6px 6px 0 0;
  border-left: 3px solid #4a9eff;
  flex-shrink: 0;
  cursor: default;
}
.group-name {
  font-size: 12px;
  color: #bbb;
  white-space: nowrap;
}
.group-count {
  font-size: 11px;
  color: #777;
  background: #2e2e2e;
  border-radius: 8px;
  padding: 0 6px;
}
.group-toggle {
  border: none;
  background: transparent;
  color: #888;
  font-size: 10px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 4px;
}
.group-toggle:hover {
  background: #333;
  color: #fff;
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

/* 右键菜单 */
.context-menu {
  position: fixed;
  z-index: 2000;
  min-width: 160px;
  background: #2a2a2a;
  border: 1px solid #444;
  border-radius: 8px;
  padding: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
}
.menu-title {
  font-size: 11px;
  color: #888;
  padding: 4px 8px;
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  font-size: 13px;
  color: #ddd;
  border-radius: 6px;
  cursor: pointer;
}
.menu-item:hover {
  background: #3d3d3d;
}
.menu-item.accent {
  color: #4a9eff;
}
.menu-dot {
  width: 10px;
  height: 10px;
  border-radius: 3px;
  flex-shrink: 0;
}
.menu-divider {
  height: 1px;
  background: #3a3a3a;
  margin: 4px 0;
}
</style>
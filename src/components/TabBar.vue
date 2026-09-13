<script setup lang="ts">
// 标签栏（分组版）：未分组标签 + 各分组（颜色条 + 折叠）+ 右键分配分组
import { computed, ref, watch } from 'vue'
import { useTabsStore } from '../stores/tabs'
import { useGroupsStore } from '../stores/groups'
import { useWorkspacesStore } from '../stores/workspaces'
import { useToastStore } from '../stores/toast'
import { invoke } from '@tauri-apps/api/core'
import type { Tab } from '../types/tab'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()
const workspacesStore = useWorkspacesStore()

// 默认网站图标（蓝色地球 SVG，与 Electron 版一致）
const DEFAULT_FAVICON =
  'data:image/svg+xml;utf8,' +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.5" fill="var(--accent)"/><ellipse cx="8" cy="8" rx="3" ry="6.5" fill="none" stroke="#fff" stroke-width="0.8"/><path d="M1.5 8h13" stroke="#fff" stroke-width="0.8"/></svg>',
  )

// 分组后的标签结构
const groupedTabs = computed(() => {
  const ungrouped = tabsStore.tabs.filter((t) => t.groupId == null)
  const groups = groupsStore.groups.map((g) => ({
    group: g,
    tabs: tabsStore.tabs.filter((t) => t.groupId === g.id),
  }))
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
  // 贴边检测：菜单估算宽 180px，高按分组项数估算，超右/下边界时翻转
  const menuW = 180
  const menuH = 100 + groupsStore.groups.length * 30
  const x = Math.min(e.clientX, window.innerWidth - menuW - 8)
  const y = Math.min(e.clientY, window.innerHeight - menuH - 8)
  menu.value = { x: Math.max(8, x), y: Math.max(8, y), tabId: tab.id }
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
  try {
    const id = await groupsStore.createGroup(`新分组 ${n}`)
    tabsStore.assignTabToGroup(menu.value.tabId, id)
  } catch {
    // store 已 toast 提示，失败则不归组
  }
  closeMenu()
}

// ---- 右键菜单操作 ----
function onMenuRefresh() {
  if (!menu.value) return
  tabsStore.reloadTab(menu.value.tabId)
  closeMenu()
}
async function onMenuCopyUrl() {
  if (!menu.value) return
  const tab = tabsStore.tabs.find((t) => t.id === menu.value!.tabId)
  if (tab?.url) {
    try {
      await navigator.clipboard.writeText(tab.url)
      useToastStore().success('链接已复制')
    } catch {
      useToastStore().error('复制链接失败')
    }
  }
  closeMenu()
}
function onMenuOpenNewTab() {
  if (!menu.value) return
  const tab = tabsStore.tabs.find((t) => t.id === menu.value!.tabId)
  tabsStore.createTab(tab?.url || 'newtab')
  closeMenu()
}
function onMenuCloseTab() {
  if (!menu.value) return
  tabsStore.closeTab(menu.value.tabId)
  closeMenu()
}

// ---- 标签拖拽排序 ----
const dragTabId = ref<string | null>(null)
function onDragStart(e: DragEvent, id: string) {
  dragTabId.value = id
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move'
}
function onDragOver(e: DragEvent) {
  e.preventDefault()
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
}
function onDrop(e: DragEvent, id: string) {
  e.preventDefault()
  const fromId = dragTabId.value
  if (!fromId || fromId === id) {
    dragTabId.value = null
    return
  }
  const fromTab = tabsStore.tabs.find((t) => t.id === fromId)
  const toTab = tabsStore.tabs.find((t) => t.id === id)
  if (fromTab && toTab && fromTab.groupId !== toTab.groupId) {
    // 跨组拖拽：先归组，再排序到目标位置
    tabsStore.assignTabToGroup(fromId, toTab.groupId)
    tabsStore.reorderTabs(fromId, id)
  } else {
    tabsStore.reorderTabs(fromId, id)
  }
  dragTabId.value = null
}

/** 拖到分组头：归组到该分组 */
const dragOverGroupId = ref<number | null>(null)
function onDragOverGroup(e: DragEvent, groupId: number) {
  e.preventDefault()
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
  dragOverGroupId.value = groupId
}
function onDragLeaveGroup(groupId: number) {
  if (dragOverGroupId.value === groupId) dragOverGroupId.value = null
}
function onDropToGroup(e: DragEvent, groupId: number) {
  e.preventDefault()
  dragOverGroupId.value = null
  const fromId = dragTabId.value
  if (fromId) {
    const fromTab = tabsStore.tabs.find((t) => t.id === fromId)
    if (fromTab && fromTab.groupId !== groupId) {
      tabsStore.assignTabToGroup(fromId, groupId)
    }
  }
  dragTabId.value = null
}
function onDragEnd() {
  dragTabId.value = null
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

/** 鼠标中键关闭标签（auxclick button===1） */
function onTabAuxClick(e: MouseEvent, id: string) {
  if (e.button === 1) {
    e.preventDefault()
    e.stopPropagation()
    tabsStore.closeTab(id)
  }
}

function onNewTab() {
  tabsStore.createTab()
}

// ---- 工作区 ----
const showWorkspaceDialog = ref(false)

// 弹窗打开时隐藏 GTK 层 webview（否则 HTML 弹窗被 webview 盖住），关闭时恢复
watch(showWorkspaceDialog, (v) => {
  invoke('set_webview_visible', { visible: !v }).catch(() => {})
})

function toggleWorkspaceMenu() {
  showWorkspaceDialog.value = !showWorkspaceDialog.value
}

async function onSwitchWorkspace(id: number | null) {
  showWorkspaceDialog.value = false
  await workspacesStore.switchWorkspace(id)
}

async function onCreateWorkspace() {
  showWorkspaceDialog.value = false
  await workspacesStore.createWorkspace()
}

async function onDeleteWorkspace(e: MouseEvent, id: number) {
  e.stopPropagation()
  await workspacesStore.deleteWorkspace(id)
}

/** favicon 加载失败时移除元素（避免破图） */
function onFaviconError(e: Event) {
  const el = e.target as HTMLImageElement | null
  el?.remove()
}
</script>

<template>
  <div class="tab-bar" @click="closeMenu">
    <!-- 工作区按钮 -->
    <button class="workspace-btn" title="切换工作区" @click.stop="toggleWorkspaceMenu">
      <span class="ws-icon">☰</span>
      <span class="ws-name">{{ workspacesStore.currentName }}</span>
      <span class="ws-caret">▼</span>
    </button>

    <!-- 工作区弹窗（弹窗模式：WebKitGTK 浮层菜单点击有 bug，弹窗点击可靠） -->
    <Transition name="fade">
    <div v-if="showWorkspaceDialog" class="ws-overlay" @click.self="showWorkspaceDialog = false">
      <div class="ws-dialog">
        <div class="ws-dialog-header">
          <span>工作区（独立标签集合）</span>
          <button
            class="ws-close"
            title="关闭"
            aria-label="关闭"
            @click="showWorkspaceDialog = false"
          >
            ✕
          </button>
        </div>
        <div class="ws-dialog-body">
          <div
            class="ws-item"
            :class="{ active: workspacesStore.currentId === null }"
            @click="onSwitchWorkspace(null)"
          >
            <span class="ws-dot ws-default"></span>默认工作区
            <span v-if="workspacesStore.currentId === null" class="ws-check">✓</span>
          </div>
          <div
            v-for="w in workspacesStore.workspaces"
            :key="w.id"
            class="ws-item"
            :class="{ active: workspacesStore.currentId === w.id }"
            @click="onSwitchWorkspace(w.id)"
          >
            <span class="ws-dot ws-colored"></span>{{ w.name }}
            <span v-if="workspacesStore.currentId === w.id" class="ws-check">✓</span>
            <button class="ws-del" title="删除工作区" @click="onDeleteWorkspace($event, w.id)">
              ×
            </button>
          </div>
          <div class="ws-divider"></div>
          <div class="ws-item accent" @click="onCreateWorkspace">＋ 新建工作区</div>
        </div>
      </div>
    </div>
    </Transition>

    <div class="tab-scroll" @contextmenu.prevent>
      <!-- 未分组标签 -->
      <div
        v-for="tab in groupedTabs.ungrouped"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned, dragging: dragTabId === tab.id }"
        draggable="true"
        @click="onTabClick(tab.id)"
        @auxclick="onTabAuxClick($event, tab.id)"
        @contextmenu="onTabContextMenu($event, tab)"
        @dragstart="onDragStart($event, tab.id)"
        @dragover="onDragOver"
        @drop="onDrop($event, tab.id)"
        @dragend="onDragEnd"
      >
        <img v-if="tab.favicon" class="favicon" :src="tab.favicon" alt="" @error="onFaviconError" />
        <img
          v-else-if="!tab.is_pinned"
          class="favicon favicon-default"
          :src="DEFAULT_FAVICON"
          alt=""
        />
        <span v-else class="pin-icon" title="已固定">📌</span>
        <span class="title">{{ tab.title || '启动页' }}</span>
        <button class="close-btn" title="关闭标签" @click="onTabClose($event, tab.id)">×</button>
      </div>

      <!-- 分组 -->
      <template v-for="g in groupedTabs.groups" :key="g.group.id">
        <div
          class="group-head"
          :class="{ 'drop-target': dragOverGroupId === g.group.id }"
          :style="{ borderLeftColor: g.group.color }"
          @dragover="onDragOverGroup($event, g.group.id)"
          @dragleave="onDragLeaveGroup(g.group.id)"
          @drop="onDropToGroup($event, g.group.id)"
        >
          <span class="group-name">{{ g.group.name }}</span>
          <span class="group-count">{{ g.tabs.length }}</span>
          <button
            class="group-toggle"
            :title="g.group.collapsed ? '展开' : '折叠'"
            @click="onToggleCollapse(g.group.id)"
          >
            {{ g.group.collapsed ? '▶' : '▼' }}
          </button>
        </div>
        <template v-if="!g.group.collapsed">
          <div
            v-for="tab in g.tabs"
            :key="tab.id"
            class="tab"
            :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned, dragging: dragTabId === tab.id }"
            draggable="true"
            @click="onTabClick(tab.id)"
            @auxclick="onTabAuxClick($event, tab.id)"
            @contextmenu="onTabContextMenu($event, tab)"
            @dragstart="onDragStart($event, tab.id)"
            @dragover="onDragOver"
            @drop="onDrop($event, tab.id)"
            @dragend="onDragEnd"
          >
            <img
              v-if="tab.favicon"
              class="favicon"
              :src="tab.favicon"
              alt=""
              @error="onFaviconError"
            />
            <img
              v-else-if="!tab.is_pinned"
              class="favicon favicon-default"
              :src="DEFAULT_FAVICON"
              alt=""
            />
            <span v-else class="pin-icon" title="已固定">📌</span>
            <span class="title">{{ tab.title || '启动页' }}</span>
            <button class="close-btn" title="关闭标签" @click="onTabClose($event, tab.id)">
              ×
            </button>
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
      <div class="menu-item" @click="onMenuRefresh">↻ 刷新</div>
      <div class="menu-item" @click="onMenuCopyUrl">🔗 复制链接</div>
      <div class="menu-item" @click="onMenuOpenNewTab">＋ 新标签打开</div>
      <div class="menu-item danger" @click="onMenuCloseTab">✕ 关闭标签</div>
      <div class="menu-divider"></div>
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
  background: var(--bg-primary);
  padding: 0 8px;
  flex-shrink: 0;
  user-select: none;
  position: relative;
}

/* 工作区按钮 */
.workspace-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  height: 28px;
  padding: 0 10px;
  margin-right: 6px;
  background: var(--bg-secondary);
  color: var(--text-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
  flex-shrink: 0;
  transition:
    background 0.15s,
    color 0.15s;
}
.workspace-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.ws-icon {
  font-size: 13px;
  color: var(--accent);
}
.ws-caret {
  font-size: 9px;
  color: var(--text-muted);
}

/* 工作区弹窗（弹窗模式：浮层菜单在 WebKitGTK 点击有 bug） */
.ws-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.45);
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
}
.ws-dialog {
  width: 320px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 12px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.6);
  overflow: hidden;
}
.ws-dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  border-bottom: 1px solid var(--border-subtle);
}
.ws-close {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 16px;
  cursor: pointer;
  border-radius: 4px;
}
.ws-close:hover {
  background: #e53935;
  color: var(--text-primary);
}
.ws-dialog-body {
  padding: 8px;
  max-height: 320px;
  overflow-y: auto;
}
.ws-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 10px;
  font-size: 13px;
  color: var(--text-primary);
  border-radius: 6px;
  cursor: pointer;
}
.ws-item:hover {
  background: var(--bg-active);
}
.ws-item.active {
  color: var(--accent);
}
.ws-item.accent {
  color: var(--accent);
}
.ws-dot {
  width: 10px;
  height: 10px;
  border-radius: 3px;
  flex-shrink: 0;
}
.ws-default {
  background: var(--accent);
}
.ws-colored {
  background: #8e24aa;
}
.ws-check {
  margin-left: auto;
  color: var(--accent);
  font-size: 13px;
}
.ws-del {
  margin-left: auto;
  width: 18px;
  height: 18px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 4px;
  font-size: 14px;
  line-height: 18px;
  flex-shrink: 0;
}
.ws-del:hover {
  background: #e53935;
  color: var(--text-primary);
}
.ws-divider {
  height: 1px;
  background: var(--border-color);
  margin: 6px 0;
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
  background: var(--bg-secondary);
  border-radius: 8px 8px 0 0;
  border: 1px solid transparent;
  cursor: pointer;
  transition:
    opacity 0.15s,
    background 0.2s,
    box-shadow 0.2s,
    color 0.2s,
    border-color 0.2s;
}
.tab.dragging {
  opacity: 0.45;
}
.tab:hover {
  background: var(--bg-hover);
}
.tab.active {
  background: var(--bg-active);
  border-color: var(--bg-active);
  box-shadow: inset 0 3px 0 var(--accent);
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
  color: var(--text-secondary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tab.active .title {
  color: var(--text-primary);
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
  color: var(--text-muted);
  border-radius: 3px;
  cursor: pointer;
  font-size: 14px;
  line-height: 16px;
  margin-left: 4px;
  flex-shrink: 0;
  transition:
    background 0.15s,
    color 0.15s;
}
.tab:hover .close-btn,
.tab.active .close-btn {
  display: block;
}
.tab .close-btn:hover {
  background: #e53935;
  color: var(--text-primary);
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
  background: var(--bg-secondary);
  border-radius: 6px 6px 0 0;
  border-left: 3px solid var(--accent);
  flex-shrink: 0;
  cursor: default;
  transition: box-shadow 0.15s;
}
.group-head[dragging] {
  box-shadow: 0 0 0 2px var(--accent);
}
.group-head.drop-target {
  box-shadow: 0 0 0 2px var(--accent);
  background: var(--bg-active);
}
.group-name {
  font-size: 12px;
  color: var(--text-secondary);
  white-space: nowrap;
}
.group-count {
  font-size: 11px;
  color: var(--text-secondary);
  background: var(--bg-hover);
  border-radius: 8px;
  padding: 0 6px;
  line-height: 16px;
}
.group-toggle {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 10px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: 4px;
}
.group-toggle:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.new-tab-btn {
  width: 28px;
  height: 28px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: 6px;
  cursor: pointer;
  font-size: 18px;
  flex-shrink: 0;
  transition:
    background 0.15s,
    color 0.15s;
}
.new-tab-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  z-index: 2000;
  min-width: 160px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
}
.menu-title {
  font-size: 11px;
  color: var(--text-muted);
  padding: 4px 8px;
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  font-size: 13px;
  color: var(--text-primary);
  border-radius: 6px;
  cursor: pointer;
}
.menu-item:hover {
  background: var(--bg-active);
}
.menu-item.accent {
  color: var(--accent);
}
.menu-item.danger {
  color: var(--danger);
}
.menu-dot {
  width: 10px;
  height: 10px;
  border-radius: 3px;
  flex-shrink: 0;
}
.menu-divider {
  height: 1px;
  background: var(--border-color);
  margin: 4px 0;
}
</style>

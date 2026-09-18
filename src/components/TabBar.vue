<script setup lang="ts">
// 标签栏（分组版）：未分组标签 + 各分组（颜色条 + 折叠）+ 右键分配分组
import { computed, onMounted, onUnmounted, ref, watch } from 'vue'
import { useTabsStore } from '../stores/tabs'
import { useGroupsStore } from '../stores/groups'
import { useToastStore } from '../stores/toast'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import type { Tab } from '../types/tab'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()

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

// 菜单打开时隐藏 GTK 层 webview（否则 HTML 菜单被 webview 盖住），关闭时恢复
watch(menu, (v) => {
  setWebviewVisible(!v, 'tab-context-menu')
  // 关闭菜单时清空拖拽状态
  if (!v) {
    menuDragState.value = null
    menuDragging.value = false
  }
})

function onTabContextMenu(e: MouseEvent, tab: Tab) {
  e.preventDefault()
  // 兜底：菜单打开前确保分组已加载（App 启动时 loadGroups 可能还没完成）
  if (groupsStore.groups.length === 0) {
    groupsStore.loadGroups().catch((e) => {
      console.error('[tabbar] 加载分组失败:', e)
    })
  }
  // 贴边检测：菜单估算宽 200px，高按分组项数估算，超右/下边界时翻转
  const menuW = 200
  const menuH = 230 + groupsStore.groups.length * 30
  const x = Math.min(e.clientX, window.innerWidth - menuW - 8)
  const y = Math.min(e.clientY, window.innerHeight - menuH - 8)
  menu.value = { x: Math.max(8, x), y: Math.max(8, y), tabId: tab.id }
}

function closeMenu() {
  menu.value = null
}

// ---- 右键菜单拖拽移动（按住菜单任意处拖动，超阈值才移动，避免误触菜单项）----
const menuDragState = ref<{ startX: number; startY: number; origX: number; origY: number } | null>(null)
const menuDragging = ref(false)
let menuDragEnd = 0

function onMenuMouseDown(e: MouseEvent) {
  // 左键或右键按住均可拖动（右键菜单是右键打开的，用户常习惯用右键继续拖）
  if (e.button !== 0 && e.button !== 2) return
  if (!menu.value) return
  menuDragState.value = {
    startX: e.clientX,
    startY: e.clientY,
    origX: menu.value.x,
    origY: menu.value.y,
  }
  menuDragging.value = false
}
function onMenuMouseMove(e: MouseEvent) {
  if (!menuDragState.value || !menu.value) return
  const dx = e.clientX - menuDragState.value.startX
  const dy = e.clientY - menuDragState.value.startY
  if (!menuDragging.value && Math.abs(dx) + Math.abs(dy) > 5) {
    menuDragging.value = true
  }
  if (menuDragging.value) {
    menu.value = {
      ...menu.value,
      x: menuDragState.value.origX + dx,
      y: menuDragState.value.origY + dy,
    }
  }
}
function onMenuMouseUp() {
  if (menuDragging.value) menuDragEnd = Date.now()
  menuDragState.value = null
  menuDragging.value = false
}

async function onMoveToGroup(groupId: number | null) {
  if (!menu.value || menuClickGuard()) return
  tabsStore.assignTabToGroup(menu.value.tabId, groupId)
  // 用户手动取消分组：会话内豁免，自动分组不再拉回
  if (groupId === null) tabsStore.exemptFromAutoGroup(menu.value.tabId)
  closeMenu()
}

async function onNewGroupFromMenu() {
  if (!menu.value || menuClickGuard()) return
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

// ---- 右键菜单操作（原版标准菜单）----
// 菜单拖动刚结束（200ms 内）的 click 是拖动残留，忽略不执行
function menuClickGuard(): boolean {
  return Date.now() - menuDragEnd < 200
}
function onMenuPin() {
  if (!menu.value || menuClickGuard()) return
  tabsStore.pinTab(menu.value.tabId)
  closeMenu()
}
function onMenuMute() {
  if (!menu.value || menuClickGuard()) return
  tabsStore.muteTab(menu.value.tabId)
  closeMenu()
}
function onMenuCloseTab() {
  if (!menu.value || menuClickGuard()) return
  tabsStore.closeTab(menu.value.tabId)
  closeMenu()
}
function onMenuCloseOthers() {
  if (!menu.value || menuClickGuard()) return
  const keep = menu.value.tabId
  for (const t of [...tabsStore.tabs]) {
    if (t.id !== keep) tabsStore.closeTab(t.id)
  }
  closeMenu()
}
function onMenuCloseRight() {
  if (!menu.value || menuClickGuard()) return
  const id = menu.value.tabId
  const idx = tabsStore.tabs.findIndex((t) => t.id === id)
  if (idx !== -1) {
    for (const t of [...tabsStore.tabs].slice(idx + 1)) {
      tabsStore.closeTab(t.id)
    }
  }
  closeMenu()
}
function onMenuCopyUrl() {
  if (!menu.value || menuClickGuard()) return
  const tab = tabsStore.tabs.find((t) => t.id === menu.value!.tabId)
  if (tab?.url) {
    try {
      navigator.clipboard.writeText(tab.url)
      useToastStore().success('链接已复制')
    } catch {
      useToastStore().error('复制链接失败')
    }
  }
  closeMenu()
}

// ---- 标签拖拽排序（鼠标模拟：WebKitGTK 不支持 HTML5 DnD，dragstart/drop 不触发）----
const dragTabId = ref<string | null>(null)
const dropTarget = ref<{ id: string; kind: 'tab' | 'group' } | null>(null)
let mouseDownTabId: string | null = null
let dragStartX = 0
let dragStartY = 0
let dragging = false
let lastDragEnd = 0

function onTabMouseDown(e: MouseEvent, id: string) {
  // 只响应左键；关闭按钮/右键/中键不触发拖拽
  if (e.button !== 0) return
  if ((e.target as HTMLElement).closest('.close-btn')) return
  mouseDownTabId = id
  dragStartX = e.clientX
  dragStartY = e.clientY
  dragging = false
}

function onWindowMouseMove(e: MouseEvent) {
  if (!mouseDownTabId) return
  const dx = e.clientX - dragStartX
  const dy = e.clientY - dragStartY
  // 超过阈值才进入拖拽模式（避免普通点击误判）
  if (!dragging && Math.abs(dx) + Math.abs(dy) > 5) {
    dragging = true
    dragTabId.value = mouseDownTabId
  }
  if (dragging) {
    // 高亮鼠标落点（标签或分组头）
    const els = document.querySelectorAll<HTMLElement>('.tab, .group-head')
    let found: { id: string; kind: 'tab' | 'group' } | null = null
    for (const el of els) {
      const r = el.getBoundingClientRect()
      if (e.clientX >= r.left && e.clientX <= r.right && e.clientY >= r.top && e.clientY <= r.bottom) {
        const tid = el.dataset.tabId
        const gid = el.dataset.groupId
        found = tid
          ? { id: tid, kind: 'tab' }
          : gid
            ? { id: gid, kind: 'group' }
            : null
        break
      }
    }
    dropTarget.value = found
  }
}

function onWindowMouseUp() {
  if (!mouseDownTabId) return
  const fromId = mouseDownTabId
  mouseDownTabId = null
  if (dragging) {
    lastDragEnd = Date.now()
    if (dropTarget.value) {
      const target = dropTarget.value
      if (target.kind === 'group') {
        // 拖到分组头：归组
        const fromTab = tabsStore.tabs.find((t) => t.id === fromId)
        const gid = Number(target.id)
        if (fromTab && fromTab.groupId !== gid) {
          tabsStore.assignTabToGroup(fromId, gid)
        }
      } else {
        const toId = target.id
        if (toId !== fromId) {
          const fromTab = tabsStore.tabs.find((t) => t.id === fromId)
          const toTab = tabsStore.tabs.find((t) => t.id === toId)
          if (fromTab && toTab && fromTab.groupId !== toTab.groupId) {
            // 跨组拖拽：先归组，再排序到目标位置
            tabsStore.assignTabToGroup(fromId, toTab.groupId)
            tabsStore.reorderTabs(fromId, toId)
          } else {
            tabsStore.reorderTabs(fromId, toId)
          }
        }
      }
    }
  }
  dragging = false
  dragTabId.value = null
  dropTarget.value = null
}

// ---- 标签操作 ----
function onTabClick(id: string) {
  // 拖拽刚结束（200ms 内）的 mouseup 会触发 click，忽略避免误切换
  if (Date.now() - lastDragEnd < 200) return
  if (id !== tabsStore.activeTabId) {
    tabsStore.switchTab(id)
  }
}

/** 双击标签：关闭标签页（浏览器习惯，原版 Electron 无此行为，高哥指定） */
function onTabDblClick(id: string) {
  // 菜单拖动刚结束（200ms 内）的 dblclick 忽略
  if (Date.now() - menuDragEnd < 200) return
  tabsStore.closeTab(id)
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

// ---- 全局鼠标监听（拖拽排序 + 菜单拖动需要）+ 双击标签栏空白最大化 ----
onMounted(() => {
  window.addEventListener('mousemove', onWindowMouseMove)
  window.addEventListener('mouseup', onWindowMouseUp)
  window.addEventListener('mousemove', onMenuMouseMove)
  window.addEventListener('mouseup', onMenuMouseUp)
  // 点击菜单外部任意处关闭右键菜单（菜单内 @click.stop 阻止冒泡）
  window.addEventListener('click', onGlobalClick)
})
onUnmounted(() => {
  window.removeEventListener('mousemove', onWindowMouseMove)
  window.removeEventListener('mouseup', onWindowMouseUp)
  window.removeEventListener('mousemove', onMenuMouseMove)
  window.removeEventListener('mouseup', onMenuMouseUp)
  window.removeEventListener('click', onGlobalClick)
})

function onGlobalClick() {
  if (menu.value) closeMenu()
}

/** 双击标签栏空白处：最大化/还原窗口（浏览器惯例） */
function onTabScrollDblClick(e: MouseEvent) {
  // 只有点击空白处（非标签/分组头/按钮）才触发
  const t = e.target as HTMLElement
  if (t.closest('.tab') || t.closest('.group-head') || t.closest('.new-tab-btn')) return
  invoke('toggle_maximize').catch(() => {})
}

// ---- 分组操作 ----
function onToggleCollapse(groupId: number) {
  const g = groupsStore.groups.find((x) => x.id === groupId)
  if (g) groupsStore.updateGroup(groupId, { collapsed: !g.collapsed })
}

/** favicon 加载失败时移除元素（避免破图） */
function onFaviconError(e: Event) {
  const el = e.target as HTMLImageElement | null
  el?.remove()
}
</script>

<template>
  <div class="tab-bar" @click="closeMenu">
    <div class="tab-scroll" @contextmenu.prevent @dblclick="onTabScrollDblClick">
      <!-- 未分组标签 -->
      <div
        v-for="tab in groupedTabs.ungrouped"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned, dragging: dragTabId === tab.id }"
        :data-tab-id="tab.id"
        @click="onTabClick(tab.id)"
        @dblclick="onTabDblClick(tab.id)"
        @auxclick="onTabAuxClick($event, tab.id)"
        @contextmenu="onTabContextMenu($event, tab)"
        @mousedown="onTabMouseDown($event, tab.id)"
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
            :class="{ 'drop-target': dropTarget?.kind === 'group' && dropTarget.id === String(g.group.id) }"
            :style="{ borderLeftColor: g.group.color }"
            :data-group-id="g.group.id"
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
            :data-tab-id="tab.id"
            @click="onTabClick(tab.id)"
            @dblclick="onTabDblClick(tab.id)"
            @auxclick="onTabAuxClick($event, tab.id)"
            @contextmenu="onTabContextMenu($event, tab)"
            @mousedown="onTabMouseDown($event, tab.id)"
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

    <!-- 右键菜单（原版标准菜单：固定/静音/关闭/复制网址 + 移动到分组；按住可拖动位置） -->
    <div
      v-if="menu"
      class="context-menu"
      :class="{ 'menu-dragging': menuDragging }"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
      @click.stop
      @contextmenu.prevent
      @mousedown="onMenuMouseDown"
    >
      <div class="menu-item" @click="onMenuPin">📌 固定/取消固定</div>
      <div class="menu-item" @click="onMenuMute">🔇 静音/取消静音</div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="onMenuCloseTab">✕ 关闭标签</div>
      <div class="menu-item" @click="onMenuCloseOthers">关闭其他标签</div>
      <div class="menu-item" @click="onMenuCloseRight">关闭右侧标签</div>
      <div class="menu-divider"></div>
      <div class="menu-item" @click="onMenuCopyUrl">🔗 复制网址</div>
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
  cursor: grab;
  user-select: none;
}
.context-menu.menu-dragging {
  cursor: grabbing;
  opacity: 0.9;
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

<script setup lang="ts">
// 标签栏（分组版）：未分组标签 + 各分组（颜色条 + 折叠）+ 右键分配分组
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { useTabsStore } from '../stores/tabs'
import { useGroupsStore } from '../stores/groups'
import { useToastStore } from '../stores/toast'
import { invokeOk } from '../utils/invoke'
import { setWebviewVisible } from '../utils/webview'
import type { Tab } from '../types/tab'
// 默认网站图标（单一来源：constants.ts，地球版）
import { UI } from '../constants'

const tabsStore = useTabsStore()
const groupsStore = useGroupsStore()

// 默认网站图标（地球 SVG，与 Electron 版一致）
const DEFAULT_FAVICON = UI.defaultFavicon

// 固定标签永远排最前（固定标签之间保持原相对顺序），普通标签按原顺序
function pinnedFirst(tabs: Tab[]): Tab[] {
  return [...tabs].sort((a, b) => Number(b.is_pinned) - Number(a.is_pinned))
}

// 分组后的标签结构
const groupedTabs = computed(() => {
  const ungrouped = pinnedFirst(tabsStore.tabs.filter((t) => t.groupId == null))
  const groups = groupsStore.groups.map((g) => ({
    group: g,
    tabs: pinnedFirst(tabsStore.tabs.filter((t) => t.groupId === g.id)),
  }))
  return { ungrouped, groups }
})

// ---- 右键菜单 ----
const menu = ref<{ x: number; y: number; tabId: string } | null>(null)
/** 菜单 DOM 引用（键盘导航与焦点管理用，R4-6 包 2） */
const menuEl = ref<HTMLElement | null>(null)
/** 菜单打开时的触发元素（键盘路径关闭时归还焦点用） */
let menuAnchorEl: HTMLElement | null = null

// 菜单打开时隐藏 GTK 层 webview（否则 HTML 菜单被 webview 盖住），关闭时恢复
watch(menu, (v) => {
  setWebviewVisible(!v, 'tab-context-menu')
  // 关闭菜单时清空拖拽状态
  if (!v) {
    menuDragState.value = null
    menuDragging.value = false
  }
})

// 菜单打开后：焦点移入首项
// （R4-6 包 2：菜单键盘可用——打开即可 ↑/↓ 选择、Enter 激活、Esc 关闭）
// tabindex=-1 已在模板 :tabindex 绑定，此处只处理焦点
watch(menuEl, async (el) => {
  if (!el) return
  await nextTick()
  const items = el.querySelectorAll<HTMLElement>('.menu-item')
  items[0]?.focus()
})

/** 打开标签右键菜单（鼠标右键与键盘菜单键共用；anchor = 焦点归还目标） */
function openTabMenu(x: number, y: number, tabId: string, anchor: HTMLElement | null = null) {
  // 兜底：菜单打开前确保分组已加载（App 启动时 loadGroups 可能还没完成）
  if (groupsStore.groups.length === 0) {
    groupsStore.loadGroups().catch((e) => {
      console.error('[tabbar] 加载分组失败:', e)
    })
  }
  // 贴边检测：菜单估算宽 200px，高按分组项数估算，超右/下边界时翻转
  const menuW = 200
  const menuH = 230 + groupsStore.groups.length * 30
  const px = Math.min(x, window.innerWidth - menuW - 8)
  const py = Math.min(y, window.innerHeight - menuH - 8)
  menuAnchorEl = anchor
  menu.value = { x: Math.max(8, px), y: Math.max(8, py), tabId }
}

function onTabContextMenu(e: MouseEvent, tab: Tab) {
  e.preventDefault()
  openTabMenu(e.clientX, e.clientY, tab.id)
}

function closeMenu() {
  const focusInside = !!menuEl.value && menuEl.value.contains(document.activeElement)
  menu.value = null
  // 键盘路径（焦点在菜单内）关闭时归还焦点到触发标签；鼠标路径不转移焦点
  if (focusInside && menuAnchorEl) menuAnchorEl.focus()
  menuAnchorEl = null
}

// ---- 标签栏键盘导航（R4-6 包 2：←/→ 移动焦点、Enter 激活、菜单键打开菜单；事件委托到 .tab-scroll）----
function onTabListKeydown(e: KeyboardEvent) {
  const target = e.target as HTMLElement
  // 关闭按钮等内部控件自行处理键盘（button 默认 Enter/Space 触发 click）
  if (target.closest('.close-btn')) return
  const tabEl = target.closest('.tab') as HTMLElement | null
  if (!tabEl) return
  const list = (e.currentTarget as HTMLElement).querySelectorAll<HTMLElement>('.tab')
  const idx = Array.from(list).indexOf(tabEl)
  if (idx === -1) return
  if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
    e.preventDefault()
    const step = e.key === 'ArrowRight' ? 1 : -1
    list[(idx + step + list.length) % list.length]?.focus()
    return
  }
  if (e.key === 'Enter') {
    e.preventDefault()
    const id = tabEl.dataset.tabId
    if (id) onTabClick(id)
    return
  }
  if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
    e.preventDefault()
    const id = tabEl.dataset.tabId
    if (!id) return
    const rect = tabEl.getBoundingClientRect()
    openTabMenu(rect.left, rect.bottom + 4, id, tabEl)
  }
}

// ---- 右键菜单键盘导航（↑/↓/Home/End 移动、Enter/Space 激活、Esc 关闭）----
function onMenuKeydown(e: KeyboardEvent) {
  const el = menuEl.value
  if (!el) return
  const items = Array.from(el.querySelectorAll<HTMLElement>('.menu-item'))
  if (items.length === 0) return
  const cur = document.activeElement instanceof HTMLElement ? document.activeElement : null
  const idx = cur ? items.indexOf(cur) : -1
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault()
    const step = e.key === 'ArrowDown' ? 1 : -1
    items[(idx + step + items.length) % items.length]?.focus()
    return
  }
  if (e.key === 'Home' || e.key === 'End') {
    e.preventDefault()
    items[e.key === 'Home' ? 0 : items.length - 1]?.focus()
    return
  }
  if (e.key === 'Enter' || e.key === ' ') {
    if (idx === -1) return
    e.preventDefault()
    cur?.click()
    return
  }
  if (e.key === 'Escape') {
    e.preventDefault()
    closeMenu()
  }
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
/** 在右侧分屏 / 关闭分屏（阶段 3.1） */
function onMenuSplit() {
  if (!menu.value || menuClickGuard()) return
  const id = menu.value.tabId
  closeMenu()
  if (tabsStore.splitTabId === id) {
    // 有意静默：退出分屏是本地状态复位，失败也会在下次切换/关签时被覆盖，无需打扰
    tabsStore.clearSplit().catch(() => {})
  } else {
    tabsStore.setSplit(id).catch((e) => {
      console.error('[tabs] 分屏失败:', e)
      useToastStore().error('分屏失败')
    })
  }
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
/** 关闭同域名标签（保留右键的这个） */
function onMenuCloseSameDomain() {
  if (!menu.value || menuClickGuard()) return
  const n = tabsStore.closeSameDomain(menu.value.tabId)
  useToastStore().success(`已关闭 ${n} 个同域名标签`)
  closeMenu()
}
/** 关闭重复标签（同 URL 只留一个） */
function onMenuCloseDuplicates() {
  if (!menu.value || menuClickGuard()) return
  const n = tabsStore.closeDuplicates()
  useToastStore().success(n > 0 ? `已关闭 ${n} 个重复标签` : '没有重复标签')
  closeMenu()
}
/** 全部静音 / 全部取消静音 */
function onMenuMuteAll() {
  if (!menu.value || menuClickGuard()) return
  const muted = tabsStore.toggleMuteAll()
  useToastStore().success(muted ? '已全部静音' : '已全部取消静音')
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

function onNewPrivateTab() {
  tabsStore.createTab(undefined, undefined, false, true)
}

// ---- 全局鼠标监听（拖拽排序 + 菜单拖动需要）+ 双击标签栏空白最大化 ----
onMounted(() => {
  window.addEventListener('mousemove', onWindowMouseMove)
  window.addEventListener('mouseup', onWindowMouseUp)
  window.addEventListener('mousemove', onMenuMouseMove)
  window.addEventListener('mouseup', onMenuMouseUp)
  // 标签栏空白延迟拖动（拖动判定 + 双击取消）
  window.addEventListener('mousemove', onTabBarWindowMouseMove)
  window.addEventListener('mouseup', onTabBarWindowMouseUp)
  // 点击菜单外部任意处关闭右键菜单（菜单内 @click.stop 阻止冒泡）
  window.addEventListener('click', onGlobalClick)
})
onUnmounted(() => {
  window.removeEventListener('mousemove', onWindowMouseMove)
  window.removeEventListener('mouseup', onWindowMouseUp)
  window.removeEventListener('mousemove', onMenuMouseMove)
  window.removeEventListener('mouseup', onMenuMouseUp)
  window.removeEventListener('mousemove', onTabBarWindowMouseMove)
  window.removeEventListener('mouseup', onTabBarWindowMouseUp)
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
  // 有意静默：OS 窗口动作，失败时窗口本身仍是唯一反馈渠道，toast 无意义
  invokeOk('toggle_maximize').catch(() => {})
}

// ---- 无边框窗口控制（标签栏 = 标题栏，Chrome 式）----
function onMinimize() {
  // 有意静默：OS 窗口动作，失败无用户可见的补偿手段
  invokeOk('minimize_window').catch(() => {})
}
function onToggleMax() {
  // 有意静默：OS 窗口动作，失败无用户可见的补偿手段
  invokeOk('toggle_maximize').catch(() => {})
}
function onClose() {
  // 有意静默：窗口关闭失败仅表现为窗口还在，用户可再点一次
  invokeOk('close_window').catch(() => {})
}

/** 标签栏空白处按住拖动窗口（标签/分组头/按钮上有自己的交互，不触发）
 *  延迟 200ms 再进入原生拖动：立即 start_dragging 会吞掉第二次 mousedown，
 *  导致双击最大化（dblclick）永远不触发。延迟期内 mouseup（单击/双击）取消拖动，
 *  移动超阈值则立即进入拖动——Chrome 式行为。 */
let tabBarDragTimer: number | null = null
let tabBarDragStartX = 0
let tabBarDragStartY = 0
let tabBarDragPending = false

function onTabBarMouseDown(e: MouseEvent) {
  if (e.button !== 0) return
  const t = e.target as HTMLElement
  if (
    t.closest('.tab') ||
    t.closest('.group-head') ||
    t.closest('.new-tab-btn') ||
    t.closest('.wc-btn') ||
    t.closest('.context-menu')
  )
    return
  tabBarDragStartX = e.clientX
  tabBarDragStartY = e.clientY
  tabBarDragPending = true
  if (tabBarDragTimer) window.clearTimeout(tabBarDragTimer)
  tabBarDragTimer = window.setTimeout(() => {
    tabBarDragTimer = null
    if (!tabBarDragPending) return
    tabBarDragPending = false
    // 有意静默：拖动启动失败仅本次手势不生效，瞬时操作无状态残留
    invokeOk('start_drag_window').catch(() => {})
  }, 200)
}

/** 延迟期内移动超阈值：立即进入窗口拖动（不等 200ms 到期） */
function onTabBarWindowMouseMove(e: MouseEvent) {
  if (!tabBarDragPending) return
  const dx = e.clientX - tabBarDragStartX
  const dy = e.clientY - tabBarDragStartY
  if (Math.abs(dx) + Math.abs(dy) > 3) {
    tabBarDragPending = false
    if (tabBarDragTimer) {
      window.clearTimeout(tabBarDragTimer)
      tabBarDragTimer = null
    }
    // 有意静默：拖动启动失败仅本次手势不生效，瞬时操作无状态残留
    invokeOk('start_drag_window').catch(() => {})
  }
}

/** 延迟期内松开：单击/双击，取消延迟拖动（让 dblclick 正常触发最大化） */
function onTabBarWindowMouseUp() {
  tabBarDragPending = false
  if (tabBarDragTimer) {
    window.clearTimeout(tabBarDragTimer)
    tabBarDragTimer = null
  }
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
  <div class="tab-bar" @click="closeMenu" @mousedown="onTabBarMouseDown">
    <div
      class="tab-scroll"
      role="tablist"
      aria-label="打开的标签"
      @contextmenu.prevent
      @dblclick="onTabScrollDblClick"
      @keydown="onTabListKeydown"
    >
      <!-- 未分组标签 -->
      <div
        v-for="tab in groupedTabs.ungrouped"
        :key="tab.id"
        class="tab"
        :class="{ active: tab.id === tabsStore.activeTabId, pinned: tab.is_pinned, dragging: dragTabId === tab.id }"
        :data-tab-id="tab.id"
        role="tab"
        :tabindex="tab.id === tabsStore.activeTabId ? 0 : -1"
        :aria-selected="tab.id === tabsStore.activeTabId"
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
        <span v-if="tab.private" class="priv-mark" title="无痕标签">🕶</span>
            <span class="title">{{ tab.title || '启动页' }}</span>
        <button class="close-btn" title="关闭标签" aria-label="关闭标签" @click="onTabClose($event, tab.id)">×</button>
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
            :aria-label="g.group.collapsed ? '展开分组' : '折叠分组'"
            :aria-expanded="!g.group.collapsed"
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
            role="tab"
            :tabindex="tab.id === tabsStore.activeTabId ? 0 : -1"
            :aria-selected="tab.id === tabsStore.activeTabId"
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
            <button class="close-btn" title="关闭标签" aria-label="关闭标签" @click="onTabClose($event, tab.id)">
              ×
            </button>
          </div>
        </template>
      </template>
    </div>
    <button class="new-tab-btn" title="新建标签" aria-label="新建标签" @click="onNewTab">+</button>
    <button class="new-tab-btn priv-tab-btn" title="新建无痕标签" aria-label="新建无痕标签" @click="onNewPrivateTab">🕶</button>

    <!-- 窗口控制（标签栏 = 标题栏，无边框窗口） -->
    <div class="window-controls">
      <button class="wc-btn" title="最小化" aria-label="最小化" @click="onMinimize">─</button>
      <button class="wc-btn" title="最大化/还原" aria-label="最大化或还原" @click="onToggleMax">□</button>
      <button class="wc-btn wc-close" title="关闭" aria-label="关闭" @click="onClose">✕</button>
    </div>

    <!-- 右键菜单（原版标准菜单：固定/静音/关闭/复制网址 + 移动到分组；按住可拖动位置） -->
    <div
      v-if="menu"
      ref="menuEl"
      class="context-menu"
      role="menu"
      :class="{ 'menu-dragging': menuDragging }"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
      @click.stop
      @contextmenu.prevent
      @mousedown="onMenuMouseDown"
      @keydown="onMenuKeydown"
    >
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuPin">📌 固定/取消固定</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuMute">🔇 静音/取消静音</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuSplit">
        {{ tabsStore.splitTabId === menu?.tabId ? '◫ 关闭分屏' : '◫ 在右侧分屏' }}
      </div>
      <div class="menu-divider" role="separator"></div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCloseTab">✕ 关闭标签</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCloseOthers">关闭其他标签</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCloseRight">关闭右侧标签</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCloseSameDomain">关闭同域名标签</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCloseDuplicates">关闭重复标签</div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuMuteAll">🔇 全部静音/取消</div>
      <div class="menu-divider" role="separator"></div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMenuCopyUrl">🔗 复制网址</div>
      <div class="menu-divider" role="separator"></div>
      <div class="menu-title" role="presentation">移动到分组</div>
      <div
        v-for="g in groupsStore.groups"
        :key="g.id"
        class="menu-item" role="menuitem" :tabindex="-1"
        @click="onMoveToGroup(g.id)"
      >
        <span class="menu-dot" :style="{ background: g.color }"></span>{{ g.name }}
      </div>
      <div class="menu-item" role="menuitem" :tabindex="-1" @click="onMoveToGroup(null)">取消分组</div>
      <div class="menu-divider" role="separator"></div>
      <div class="menu-item accent" role="menuitem" :tabindex="-1" @click="onNewGroupFromMenu">＋ 新建分组并移动</div>
    </div>
  </div>
</template>

<style scoped>
.tab-bar {
  display: flex;
  align-items: flex-end;
  height: var(--sb-tab-bar-h);
  background: var(--bg-primary);
  padding: 0 8px;
  flex-shrink: 0;
  user-select: none;
  position: relative;
}
/* 条带底缘发丝线：标签栏与地址栏同色时提供一层结构分隔 */
.tab-bar::after {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  bottom: 0;
  height: 1px;
  background: var(--hairline);
  pointer-events: none;
}

.tab-scroll {
  display: flex;
  /* 标签贴底：Chrome 式标签是"顶部圆角、底部直角"，底部应直连地址栏，
     align-items:center 会让标签底部留空隙，看起来和地址栏"隔了一截" */
  align-items: flex-end;
  flex: 1;
  overflow-x: auto;
  height: 100%;
}
.tab-scroll::-webkit-scrollbar {
  display: none;
}
.tab {
  position: relative;
  z-index: 1;
  display: flex;
  align-items: center;
  /* 宽度自适应：标签少时撑到最多 180px，标签变多后自动收窄，
     压到 64px 下限后再多就由 .tab-scroll 横向滚动兜底 */
  min-width: 64px;
  max-width: 180px;
  width: 180px;
  flex: 1 1 180px;
  height: 33px;
  padding: 0 10px;
  margin-bottom: -1px; /* 压过条带底缘发丝线，活动标签与下一层连成一体 */
  background: transparent;
  border-radius: 9px 9px 0 0;
  border: 1px solid transparent;
  color: var(--text-secondary);
  cursor: pointer;
  opacity: 0.85;
  transition:
    opacity var(--dur) var(--ease),
    background var(--dur) var(--ease),
    box-shadow var(--dur) var(--ease),
    color var(--dur) var(--ease),
    border-color var(--dur) var(--ease);
}
.tab.dragging {
  opacity: 0.45;
}
.tab:hover {
  background: var(--bg-hover);
  opacity: 1;
}
.tab.active {
  background: var(--bg-secondary);
  border-color: var(--border-subtle);
  border-bottom-color: var(--bg-secondary);
  color: var(--text-primary);
  opacity: 1;
  box-shadow:
    inset 0 2.5px 0 var(--accent),
    0 -3px 10px rgba(0, 0, 0, 0.22);
}
.tab .favicon {
  width: 16px;
  height: 16px;
  border-radius: 4px;
  margin-right: 7px;
  flex-shrink: 0;
  object-fit: contain;
}
.tab .title {
  flex: 1;
  font-size: var(--text-sm);
  color: inherit;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.tab .pin-icon {
  font-size: 12px;
  margin-right: 4px;
  flex-shrink: 0;
}
.tab .close-btn {
  display: none;
  width: 17px;
  height: 17px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: var(--radius-full);
  cursor: pointer;
  font-size: 13px;
  line-height: 17px;
  padding: 0;
  margin-left: 4px;
  flex-shrink: 0;
  text-align: center;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.tab:hover .close-btn,
.tab.active .close-btn,
.tab:focus-within .close-btn {
  display: block;
}
.tab .close-btn:hover {
  background: var(--danger);
  color: var(--accent-contrast);
}

/* 焦点环（R4-6 包 2）：键盘可见焦点，口径与全项目一致（3px accent-ring） */
.tab:focus-visible,
.new-tab-btn:focus-visible,
.wc-btn:focus-visible,
.group-toggle:focus-visible,
.tab .close-btn:focus-visible {
  outline: none;
  box-shadow: 0 0 0 3px var(--accent-ring);
}
/* 活动标签带既有双层阴影：焦点环叠加、保留 active 视觉 */
.tab.active:focus-visible {
  box-shadow:
    inset 0 2.5px 0 var(--accent),
    0 -3px 10px rgba(0, 0, 0, 0.22),
    0 0 0 3px var(--accent-ring);
}
.tab.pinned {
  min-width: 34px;
  max-width: 34px;
  width: 34px;
  padding: 0;
  justify-content: center;
  border-radius: 8px 8px 0 0;
}
.tab.pinned .title {
  display: none;
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
  height: 30px;
  align-self: flex-end;
  padding: 0 9px;
  margin: 0 3px -1px 0;
  background: var(--bg-secondary);
  border-radius: 8px 8px 0 0;
  border-left: 3px solid var(--accent);
  flex-shrink: 0;
  cursor: default;
  transition: box-shadow var(--dur-fast) var(--ease);
}
.group-head[dragging] {
  box-shadow: 0 0 0 2px var(--accent-ring);
}
.group-head.drop-target {
  box-shadow: 0 0 0 2px var(--accent-ring);
  background: var(--bg-active);
}
.group-name {
  font-size: var(--text-sm);
  color: var(--text-secondary);
  white-space: nowrap;
}
.group-count {
  font-size: var(--text-xs);
  color: var(--text-muted);
  background: var(--bg-hover);
  border-radius: var(--radius-full);
  padding: 0 6px;
  line-height: 16px;
  font-variant-numeric: tabular-nums;
}
.group-toggle {
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 9px;
  cursor: pointer;
  padding: 2px 4px;
  border-radius: var(--radius-xs);
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.group-toggle:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.new-tab-btn {
  width: 30px;
  height: 30px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 17px;
  flex-shrink: 0;
  margin-bottom: 2px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease),
    box-shadow var(--dur-fast) var(--ease);
}
.priv-tab-btn {
  font-size: 13px;
}
.priv-mark {
  font-size: 10px;
  margin-right: 3px;
  opacity: 0.75;
}
.new-tab-btn:hover {
  background: var(--accent-soft);
  color: var(--accent);
}

/* 窗口控制按钮（无边框窗口：标签栏 = 标题栏） */
.window-controls {
  display: flex;
  align-items: center;
  flex-shrink: 0;
  margin-left: 4px;
  height: 100%;
}
.wc-btn {
  width: 40px;
  height: 100%;
  border: none;
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.wc-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.wc-close:hover {
  background: var(--danger);
  color: var(--accent-contrast);
}

/* 右键菜单 */
.context-menu {
  position: fixed;
  z-index: var(--z-menu);
  min-width: 168px;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  padding: 6px;
  box-shadow: var(--shadow-pop);
  cursor: grab;
  user-select: none;
}
.context-menu.menu-dragging {
  cursor: grabbing;
  opacity: 0.92;
}
.menu-title {
  font-size: var(--text-xs);
  color: var(--text-muted);
  padding: 5px 8px 3px;
  letter-spacing: 0.04em;
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 8px;
  font-size: var(--text-base);
  color: var(--text-primary);
  border-radius: var(--radius-xs);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
.menu-item:hover {
  background: var(--bg-hover);
}
.menu-item:focus-visible {
  background: var(--bg-hover);
  outline: none;
}
.menu-item.accent {
  color: var(--accent);
}
.menu-item.danger {
  color: var(--danger);
}
.menu-dot {
  width: 9px;
  height: 9px;
  border-radius: 2.5px;
  flex-shrink: 0;
}
.menu-divider {
  height: 1px;
  background: var(--hairline);
  margin: 5px 4px;
}
</style>

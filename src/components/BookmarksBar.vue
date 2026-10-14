<script setup lang="ts">
// 书签栏：显示文件夹分组 + 未分类书签 + 收藏当前页 + 右键管理 + 管理器入口
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { useConfirmDelete } from '../composables/useConfirmDelete'
import UiDialog from './ui/UiDialog.vue'
import type { Bookmark } from '../types/bookmark'

const bookmarksStore = useBookmarksStore()
const tabsStore = useTabsStore()
const confirmDelete = useConfirmDelete()

const emit = defineEmits<{ (e: 'open-manager'): void }>()

/** 书签栏横向滚动容器：纵向滚轮转横向（内容多时滚轮可翻） */
const listRef = ref<HTMLElement | null>(null)
function onWheel(e: WheelEvent) {
  const el = listRef.value
  // 横向已在滚（触控板横滑）或内容没溢出：不拦截
  if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return
  if (!el || el.scrollWidth <= el.clientWidth) return
  e.preventDefault()
  // 行模式（部分内核滚轮按行报）：一行约 16px，像素模式直接用
  const dy = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY
  el.scrollLeft += dy
}

// ===== 文件夹分组（书签栏像原版一样显示 📁 文件夹，点击展开书签）=====
const folders = ref<{ id: number; name: string }[]>([])
const folderMenu = ref<{ folderId: number; x: number; y: number } | null>(null)

onMounted(async () => {
  // 滚轮监听先绑：数据加载失败也不影响滚动
  listRef.value?.addEventListener('wheel', onWheel, { passive: false })
  if (!bookmarksStore.loaded) {
    await bookmarksStore.loadBookmarks()
  }
  folders.value = await bookmarksStore.loadFolders()
})

onUnmounted(() => {
  listRef.value?.removeEventListener('wheel', onWheel)
})

/** 有书签的文件夹才显示在书签栏 */
const foldersWithBookmarks = computed(() =>
  folders.value.filter((f) => folderBookmarks(f.id).length > 0)
)

/** 未分类书签（folder_id 为空）平铺显示 */
const unfiledBookmarks = computed(() =>
  bookmarksStore.bookmarks.filter((b) => b.folder_id == null)
)

function folderBookmarks(folderId: number) {
  return bookmarksStore.bookmarks.filter((b) => b.folder_id === folderId)
}

/** 点击文件夹 → 打开/切换下拉菜单 */
function onFolderClick(e: MouseEvent, folder: { id: number; name: string }) {
  e.stopPropagation()
  const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
  const menuW = 280
  const x = Math.min(rect.left, window.innerWidth - menuW - 8)
  const y = rect.bottom + 2
  if (folderMenu.value?.folderId === folder.id) {
    closeFolderMenu()
  } else {
    folderMenu.value = { folderId: folder.id, x: Math.max(8, x), y }
    syncWebview()
  }
}

function closeFolderMenu() {
  folderMenu.value = null
  syncWebview()
}

/** 点击文件夹内书签 → 导航 */
function onFolderBookmarkClick(url: string) {
  const active = tabsStore.activeTab
  if (active) tabsStore.navigateTab(active.id, url)
  closeFolderMenu()
}

/** 文件夹内书签右键 → 复用管理菜单 */
function onFolderBookmarkContext(e: MouseEvent, b: Bookmark) {
  e.preventDefault()
  e.stopPropagation()
  const menuW = 160
  const menuH = 150
  const x = Math.min(e.clientX, window.innerWidth - menuW - 8)
  const y = Math.min(e.clientY, window.innerHeight - menuH - 8)
  menu.value = { x: Math.max(8, x), y: Math.max(8, y), bookmark: b }
  closeFolderMenu() // 关下拉菜单（内部 syncWebview：menu 非 null → 仍隐藏）
  syncWebview()
}

/** 点击书签 → 导航到书签 URL */
function onClickBookmark(url: string) {
  const active = tabsStore.activeTab
  if (active) tabsStore.navigateTab(active.id, url)
}

/** 删除书签（悬停叉极易误触：二次点击确认） */
async function onDeleteBookmark(id: number) {
  if (!confirmDelete.arm(id)) return
  await bookmarksStore.removeBookmark(id)
}

/** 收藏当前页 */
async function onBookmark() {
  const active = tabsStore.activeTab
  if (!active) return
  const title = active.title || active.url
  await bookmarksStore.addBookmark(title, active.url)
}

// ===== 右键菜单 + 编辑弹窗 =====
const menu = ref<{ x: number; y: number; bookmark: Bookmark | null } | null>(null)
const editTarget = ref<Bookmark | null>(null)
/** 编辑弹窗显隐（对象转布尔，关即清空） */
const editModel = computed({
  get: () => editTarget.value !== null,
  set: (v: boolean) => {
    if (!v) closeEdit()
  },
})

// 统一同步 GTK 层 webview：菜单、下拉菜单或编辑弹窗任一打开就隐藏，全关才恢复。
// 不用 watch——异步时序会竞争（菜单关闭的恢复和弹窗打开的隐藏互相覆盖）。
function syncWebview() {
  const hidden = !!menu.value || !!editTarget.value || !!folderMenu.value
  setWebviewVisible(!hidden, 'bookmarks-bar')
}

// 点击菜单外部任意处关闭菜单
onMounted(() => {
  window.addEventListener('click', onGlobalClick)
})
onUnmounted(() => {
  window.removeEventListener('click', onGlobalClick)
})
function onGlobalClick() {
  if (menu.value) closeMenu()
  if (folderMenu.value) closeFolderMenu()
}

function onContextMenu(e: MouseEvent, bookmark: Bookmark | null) {
  e.preventDefault()
  const menuW = 160
  const menuH = bookmark ? 150 : 90
  const x = Math.min(e.clientX, window.innerWidth - menuW - 8)
  const y = Math.min(e.clientY, window.innerHeight - menuH - 8)
  menu.value = { x: Math.max(8, x), y: Math.max(8, y), bookmark }
  syncWebview()
}

function closeMenu() {
  menu.value = null
  syncWebview()
}

function onMenuOpenNewTab() {
  if (!menu.value?.bookmark) return
  tabsStore.createTab(menu.value.bookmark.url)
  closeMenu()
}

async function onMenuCopyUrl() {
  if (!menu.value?.bookmark) return
  try {
    await navigator.clipboard.writeText(menu.value.bookmark.url)
    useToastStore().success('链接已复制')
  } catch {
    useToastStore().error('复制链接失败')
  }
  closeMenu()
}

function onMenuEdit() {
  if (!menu.value?.bookmark) return
  editTarget.value = menu.value.bookmark
  editForm.value = { title: menu.value.bookmark.title, url: menu.value.bookmark.url }
  closeMenu() // closeMenu 内 syncWebview：menu=null 但 editTarget 非 null → 仍隐藏
}

// 关闭编辑弹窗：恢复 webview
function closeEdit() {
  editTarget.value = null
  syncWebview()
}

async function onMenuDelete() {
  if (!menu.value?.bookmark) return
  await bookmarksStore.removeBookmark(menu.value.bookmark.id)
  closeMenu()
}

async function onMenuNewFolder() {
  const n = new Date().toLocaleDateString('zh-CN') + ' 文件夹'
  const id = await bookmarksStore.createFolder(n)
  if (id !== null) useToastStore().success('文件夹已创建')
  closeMenu()
}

// ===== 编辑弹窗 =====
const editForm = ref({ title: '', url: '' })

async function onSaveEdit() {
  if (!editTarget.value) return
  const title = editForm.value.title.trim()
  const url = editForm.value.url.trim()
  if (!title || !url) {
    useToastStore().error('标题和网址不能为空')
    return
  }
  await bookmarksStore.updateBookmark(editTarget.value.id, title, url)
  closeEdit()
}
</script>

<template>
  <div
    class="bookmarks-bar"
    :class="{ empty: bookmarksStore.bookmarks.length === 0 }"
    @contextmenu="onContextMenu($event, null)"
  >
    <div ref="listRef" class="bookmarks-list">
      <button
        v-for="f in foldersWithBookmarks"
        :key="f.id"
        class="folder-item"
        :class="{ active: folderMenu?.folderId === f.id }"
        :title="`${f.name}（${folderBookmarks(f.id).length} 个书签）`"
        @click="onFolderClick($event, f)"
      >
        📁 {{ f.name }}
      </button>
      <button
        v-for="b in unfiledBookmarks"
        :key="b.id"
        class="bookmark-item"
        :title="b.url"
        @click="onClickBookmark(b.url)"
        @contextmenu.stop="onContextMenu($event, b)"
      >
        {{ b.title }}
        <span
          class="bm-del"
          :class="{ armed: confirmDelete.isPending(b.id) }"
          :title="confirmDelete.isPending(b.id) ? '再点一次确认删除' : '删除书签'"
          @click.stop="onDeleteBookmark(b.id)"
          >{{ confirmDelete.isPending(b.id) ? '?' : '×' }}</span
        >
      </button>
    </div>
    <button class="action-btn" title="收藏当前页" @click="onBookmark">☆</button>
    <button class="action-btn manage-btn" title="书签管理器（导入/导出/管理）" @click="emit('open-manager')">📁</button>

    <!-- 文件夹下拉菜单 -->
    <div
      v-if="folderMenu"
      class="folder-menu"
      :style="{ left: folderMenu.x + 'px', top: folderMenu.y + 'px' }"
      @click.stop
      @contextmenu.prevent
    >
      <div
        v-for="b in folderBookmarks(folderMenu.folderId)"
        :key="b.id"
        class="folder-menu-item"
        :title="`${b.title}\n${b.url}`"
        @click="onFolderBookmarkClick(b.url)"
        @contextmenu.stop="onFolderBookmarkContext($event, b)"
      >
        {{ b.title }}
      </div>
      <div v-if="folderBookmarks(folderMenu.folderId).length === 0" class="folder-menu-empty">
        （空文件夹）
      </div>
    </div>

    <!-- 右键菜单 -->
    <div
      v-if="menu"
      class="context-menu"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
      @click.stop
      @contextmenu.prevent
    >
      <template v-if="menu.bookmark">
        <div class="menu-item" @click="onMenuOpenNewTab">＋ 新标签打开</div>
        <div class="menu-item" @click="onMenuCopyUrl">🔗 复制链接</div>
        <div class="menu-item" @click="onMenuEdit">✏ 编辑</div>
        <div class="menu-item danger" @click="onMenuDelete">✕ 删除</div>
      </template>
      <template v-else>
        <div class="menu-item" @click="onBookmark">☆ 收藏当前页</div>
        <div class="menu-item" @click="onMenuNewFolder">📁 新建文件夹</div>
      </template>
    </div>

    <!-- 编辑弹窗（UiDialog，顶层展示） -->
    <UiDialog v-model:open="editModel" title="编辑书签" width="min(360px, calc(100vw - 32px))">
      <label class="bm-field">
        <span>标题</span>
        <input v-model="editForm.title" type="text" spellcheck="false" />
      </label>
      <label class="bm-field">
        <span>网址</span>
        <input v-model="editForm.url" type="text" spellcheck="false" />
      </label>
      <div class="bm-actions">
        <button class="bm-cancel" @click="closeEdit">取消</button>
        <button class="bm-save" @click="onSaveEdit">保存</button>
      </div>
    </UiDialog>
  </div>
</template>

<style scoped>
.bookmarks-bar {
  display: flex;
  align-items: center;
  height: var(--sb-bookmarks-bar-h);
  background: var(--bg-primary);
  border-bottom: 1px solid var(--hairline);
  padding: 0 8px;
  gap: 4px;
  flex-shrink: 0;
  user-select: none;
}
.bookmarks-list {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow-x: auto;
  /* 横向滚动条藏起：滚轮横翻，不占地方 */
  scrollbar-width: none;
}
.bookmarks-list::-webkit-scrollbar {
  display: none;
}
.bookmark-item {
  white-space: nowrap;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 3px 9px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-full);
  cursor: pointer;
  font-size: var(--text-sm);
  flex-shrink: 0;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.bookmark-item img {
  width: 14px;
  height: 14px;
  border-radius: 3px;
  flex-shrink: 0;
}
.bookmark-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.folder-item {
  white-space: nowrap;
  padding: 3px 9px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: var(--radius-full);
  cursor: pointer;
  font-size: var(--text-sm);
  flex-shrink: 0;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.folder-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.folder-item.active {
  background: var(--accent-soft);
  color: var(--accent);
}
.folder-menu {
  position: fixed;
  z-index: var(--z-menu);
  min-width: 200px;
  max-width: 300px;
  max-height: 320px;
  overflow-y: auto;
  scrollbar-width: none;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-md);
  padding: 6px;
  box-shadow: var(--shadow-pop);
}
.folder-menu::-webkit-scrollbar {
  display: none;
}
.folder-menu-item {
  padding: 7px 10px;
  font-size: 13px;
  color: var(--text-primary);
  border-radius: 5px;
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: background 0.1s;
}
.folder-menu-item:hover {
  background: var(--bg-active);
}
.folder-menu-empty {
  padding: 10px;
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
}
.bm-del {
  display: none;
  margin-left: 4px;
  color: var(--text-muted);
  font-size: 12px;
  line-height: 1;
}
.bookmark-item:hover .bm-del {
  display: inline;
}
.bm-del:hover {
  color: var(--danger);
}
/* 二次确认待命态：常显危险色（触屏无 hover 也看得见） */
.bm-del.armed {
  display: inline;
  color: var(--danger);
  font-weight: 700;
}
.empty {
  justify-content: flex-end;
}
.action-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: 4px;
  cursor: pointer;
  font-size: 14px;
  flex-shrink: 0;
  transition:
    background 0.15s,
    color 0.15s;
}
.action-btn:hover {
  background: var(--bg-active);
  color: var(--warning);
}
.manage-btn:hover {
  color: var(--accent);
}
.context-menu {
  position: fixed;
  z-index: 2000;
  min-width: 150px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
}
.menu-item {
  padding: 7px 10px;
  font-size: 13px;
  color: var(--text-primary);
  border-radius: 5px;
  cursor: pointer;
  white-space: nowrap;
  transition: background 0.1s;
}
.menu-item:hover {
  background: var(--bg-active);
}
.menu-item.danger {
  color: var(--danger);
}
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留书签栏业务样式 */
.bm-field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin-bottom: 10px;
  font-size: 12px;
  color: var(--text-secondary);
}
.bm-field input {
  height: 30px;
  padding: 0 10px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-xs);
  font-size: 13px;
  outline: none;
}
.bm-field input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}
/* 通用 .bm-actions/.bm-cancel/.bm-save 已收口至 styles/components.css */
</style>

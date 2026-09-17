<script setup lang="ts">
// 书签栏：显示文件夹分组 + 未分类书签 + 收藏当前页 + 右键管理 + 管理器入口
import { computed, onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import type { Bookmark } from '../types/bookmark'

const bookmarksStore = useBookmarksStore()
const tabsStore = useTabsStore()

const emit = defineEmits<{ (e: 'open-manager'): void }>()

// ===== 文件夹分组（书签栏像原版一样显示 📁 文件夹，点击展开书签）=====
const folders = ref<{ id: number; name: string }[]>([])
const folderMenu = ref<{ folderId: number; x: number; y: number } | null>(null)

onMounted(async () => {
  if (!bookmarksStore.loaded) {
    await bookmarksStore.loadBookmarks()
  }
  folders.value = await bookmarksStore.loadFolders()
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

/** 删除书签 */
async function onDeleteBookmark(id: number) {
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

// 统一同步 GTK 层 webview：菜单、下拉菜单或编辑弹窗任一打开就隐藏，全关才恢复。
// 不用 watch——异步时序会竞争（菜单关闭的恢复和弹窗打开的隐藏互相覆盖）。
function syncWebview() {
  const hidden = !!menu.value || !!editTarget.value || !!folderMenu.value
  invoke('set_webview_visible', { visible: !hidden, source: 'bm' }).catch(() => {})
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
    <div class="bookmarks-list">
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
        <span class="bm-del" title="删除书签" @click.stop="onDeleteBookmark(b.id)">×</span>
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

    <!-- 编辑弹窗 -->
    <div v-if="editTarget" class="dialog-overlay" @click.self="closeEdit">
      <div class="dialog bm-edit-dialog">
        <div class="dialog-header">
          <span>编辑书签</span>
          <button class="close-btn" aria-label="关闭" @click="closeEdit">×</button>
        </div>
        <div class="dialog-body">
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
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.bookmarks-bar {
  display: flex;
  align-items: center;
  height: 30px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-subtle);
  padding: 0 8px;
  gap: 4px;
  flex-shrink: 0;
  user-select: none;
}
.bookmarks-list {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow-x: auto;
  scrollbar-width: none;
}
.bookmarks-list::-webkit-scrollbar {
  display: none;
}
.bookmark-item {
  white-space: nowrap;
  padding: 3px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  flex-shrink: 0;
  transition: background 0.15s;
}
.bookmark-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.folder-item {
  white-space: nowrap;
  padding: 3px 8px;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  flex-shrink: 0;
  transition: background 0.15s;
}
.folder-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.folder-item.active {
  background: var(--bg-active);
  color: var(--accent);
}
.folder-menu {
  position: fixed;
  z-index: 2000;
  min-width: 200px;
  max-width: 300px;
  max-height: 320px;
  overflow-y: auto;
  scrollbar-width: none;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 6px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
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
  color: #ff6b6b;
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
  color: #f0c040;
}
.manage-btn:hover {
  color: #4a9eff;
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
.bm-edit-dialog {
  width: min(360px, calc(100vw - 32px));
}
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
  border-radius: 6px;
  font-size: 13px;
  outline: none;
}
.bm-field input:focus {
  border-color: var(--accent);
}
.bm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}
.bm-cancel,
.bm-save {
  height: 28px;
  padding: 0 14px;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  border: 1px solid var(--border-color);
  background: var(--bg-hover);
  color: var(--text-secondary);
}
.bm-save {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.bm-save:hover {
  opacity: 0.9;
}
</style>

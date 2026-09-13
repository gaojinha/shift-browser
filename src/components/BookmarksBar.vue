<script setup lang="ts">
// 书签栏：显示书签列表 + 收藏当前页 + 右键管理（编辑/删除/新建文件夹）
import { onMounted, ref } from 'vue'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import type { Bookmark } from '../types/bookmark'

const bookmarksStore = useBookmarksStore()
const tabsStore = useTabsStore()

onMounted(async () => {
  if (!bookmarksStore.loaded) {
    await bookmarksStore.loadBookmarks()
  }
})

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

// ===== 右键菜单 =====
const menu = ref<{ x: number; y: number; bookmark: Bookmark | null } | null>(null)

function onContextMenu(e: MouseEvent, bookmark: Bookmark | null) {
  e.preventDefault()
  const menuW = 160
  const menuH = bookmark ? 150 : 90
  const x = Math.min(e.clientX, window.innerWidth - menuW - 8)
  const y = Math.min(e.clientY, window.innerHeight - menuH - 8)
  menu.value = { x: Math.max(8, x), y: Math.max(8, y), bookmark }
}

function closeMenu() {
  menu.value = null
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
  closeMenu()
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
const editTarget = ref<Bookmark | null>(null)
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
  editTarget.value = null
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
        v-for="b in bookmarksStore.bookmarks"
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

    <!-- 右键菜单 -->
    <div
      v-if="menu"
      class="context-menu"
      :style="{ left: menu.x + 'px', top: menu.y + 'px' }"
      @click.stop
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
    <div v-if="editTarget" class="dialog-overlay" @click.self="editTarget = null">
      <div class="dialog bm-edit-dialog">
        <div class="dialog-header">
          <span>编辑书签</span>
          <button class="close-btn" aria-label="关闭" @click="editTarget = null">×</button>
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
            <button class="bm-cancel" @click="editTarget = null">取消</button>
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

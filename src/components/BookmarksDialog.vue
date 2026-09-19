<script setup lang="ts">
// 书签管理器弹窗：列表 / 搜索 / 添加 / 编辑 / 删除 / 文件夹 / 导入 / 导出
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住，
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import type { Bookmark } from '../types/bookmark'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const bookmarksStore = useBookmarksStore()
const tabsStore = useTabsStore()
const toastStore = useToastStore()

const query = ref('')
const folders = ref<{ id: number; name: string }[]>([])
const filterFolder = ref<number | 'all' | 'none'>('all')

// 弹窗显示时隐藏所有标签 webview，关闭时恢复
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        if (!bookmarksStore.loaded) await bookmarksStore.loadBookmarks()
        folders.value = await bookmarksStore.loadFolders()
      } else {
        query.value = ''
        filterFolder.value = 'all'
        editTarget.value = null
        addOpen.value = false
      }
    } catch (e) {
      console.error('[bookmarks] 加载失败:', e)
      toastStore.error('加载书签失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'bookmarks-dialog')
      }
    }
  },
)

// 过滤 + 搜索
const filtered = computed(() => {
  const q = query.value.trim().toLowerCase()
  return bookmarksStore.bookmarks.filter((b) => {
    if (filterFolder.value === 'all') {
      // 全部
    } else if (filterFolder.value === 'none') {
      if (b.folder_id != null) return false
    } else if (b.folder_id !== filterFolder.value) {
      return false
    }
    if (!q) return true
    return b.title.toLowerCase().includes(q) || b.url.toLowerCase().includes(q)
  })
})

function folderName(id: number | null): string {
  if (id == null) return '未分类'
  return folders.value.find((f) => f.id === id)?.name ?? '未分类'
}

// ---- 操作 ----
function onOpen(url: string) {
  const active = tabsStore.activeTab
  if (active) {
    tabsStore.navigateTab(active.id, url)
  } else {
    tabsStore.createTab(url)
  }
  emit('close')
}

async function onDelete(b: Bookmark) {
  const ok = await bookmarksStore.removeBookmark(b.id)
  if (ok) toastStore.success('书签已删除')
}

// 编辑
const editTarget = ref<Bookmark | null>(null)
const editForm = ref({ title: '', url: '', folderId: 'none' })
function onEdit(b: Bookmark) {
  editTarget.value = b
  editForm.value = {
    title: b.title,
    url: b.url,
    folderId: b.folder_id == null ? 'none' : String(b.folder_id),
  }
}
async function onSaveEdit() {
  if (!editTarget.value) return
  const title = editForm.value.title.trim()
  const url = editForm.value.url.trim()
  if (!title || !url) {
    toastStore.error('标题和网址不能为空')
    return
  }
  const folderId = editForm.value.folderId === 'none' ? null : Number(editForm.value.folderId)
  await bookmarksStore.updateBookmark(editTarget.value.id, title, url)
  await bookmarksStore.updateBookmarkFolder(editTarget.value.id, folderId)
  editTarget.value = null
}

// 添加
const addOpen = ref(false)
const addForm = ref({ title: '', url: '', folderId: 'none' })
function onAdd() {
  addForm.value = { title: '', url: '', folderId: filterFolder.value === 'none' ? 'none' : filterFolder.value === 'all' ? 'none' : String(filterFolder.value) }
  addOpen.value = true
}
async function onSaveAdd() {
  const title = addForm.value.title.trim()
  const url = addForm.value.url.trim()
  if (!title || !url) {
    toastStore.error('标题和网址不能为空')
    return
  }
  const folderId = addForm.value.folderId === 'none' ? null : Number(addForm.value.folderId)
  try {
    await invoke<number>('create_bookmark', {
      input: { title, url, folder_id: folderId },
    })
    await bookmarksStore.loadBookmarks()
    folders.value = await bookmarksStore.loadFolders()
    toastStore.success('书签已添加')
    addOpen.value = false
  } catch (e) {
    console.error('[bookmarks] 添加失败:', e)
    toastStore.error('添加书签失败')
  }
}

// 新建文件夹（WebKitGTK 不显示 prompt，固定"新文件夹"名，用户可后续改名）
async function onNewFolder() {
  const id = await bookmarksStore.createFolder('新文件夹')
  if (id !== null) {
    folders.value = await bookmarksStore.loadFolders()
    toastStore.success('文件夹已创建')
  }
}

// ---- 文件夹管理（删除，级联删子文件夹+书签，需自定义确认）----
const folderDeleteTarget = ref<{ id: number; name: string } | null>(null)
const showFolderConfirm = ref(false)

/** 文件夹内书签数（含未分类统计用） */
function folderCount(id: number): number {
  return bookmarksStore.bookmarks.filter((b) => b.folder_id === id).length
}

function onDeleteFolder(f: { id: number; name: string }) {
  folderDeleteTarget.value = f
  showFolderConfirm.value = true
}

async function doDeleteFolder() {
  const target = folderDeleteTarget.value
  if (!target) return
  showFolderConfirm.value = false
  folderDeleteTarget.value = null
  try {
    await invoke('delete_bookmark_folder', { id: target.id })
    await bookmarksStore.loadBookmarks()
    folders.value = await bookmarksStore.loadFolders()
    if (filterFolder.value === target.id) filterFolder.value = 'all'
    toastStore.success(`文件夹「${target.name}」已删除`)
  } catch (e) {
    console.error('[bookmarks] 删除文件夹失败:', e)
    toastStore.error('删除文件夹失败')
  }
}

// 导入：调系统文件对话框（XDG portal，WebKitGTK 的 file input 不弹窗）
async function onImport() {
  try {
    const count = await invoke<number>('import_bookmarks_from_file')
    await bookmarksStore.loadBookmarks()
    folders.value = await bookmarksStore.loadFolders()
    if (count > 0) toastStore.success(`导入完成：${count} 个书签`)
  } catch (err) {
    // 用户取消不算错误
    if (String(err).includes('取消')) return
    console.error('[bookmarks] 导入失败:', err)
    toastStore.error('导入书签失败')
  }
}

// 导出
async function onExport() {
  try {
    const path = await bookmarksStore.exportHtml()
    toastStore.success(`已导出到：${path}`)
  } catch (err) {
    console.error('[bookmarks] 导出失败:', err)
    toastStore.error('导出书签失败')
  }
}
</script>

<template>
  <Transition name="fade">
    <div v-if="show" class="dialog-overlay" @click.self="emit('close')">
      <div class="dialog bm-dialog">
        <div class="dialog-header">
          <span>书签管理器</span>
          <div class="header-actions">
            <button class="h-btn" title="导入书签（Chrome/Firefox 导出的 HTML）" @click="onImport">📥 导入</button>
            <button class="h-btn" title="导出为 Netscape HTML 到下载目录" @click="onExport">📤 导出</button>
            <button class="h-btn accent" @click="onAdd">＋ 添加书签</button>
            <button class="h-btn" @click="onNewFolder">📁 新建文件夹</button>
            <button class="close-btn" aria-label="关闭" @click="emit('close')">×</button>
          </div>
        </div>
        <div class="dialog-body bm-body">
          <div class="bm-toolbar">
            <input
              v-model="query"
              type="text"
              class="bm-search"
              placeholder="搜索书签..."
              spellcheck="false"
            />
            <select v-model="filterFolder" class="bm-filter">
              <option value="all">全部文件夹</option>
              <option value="none">未分类</option>
              <option v-for="f in folders" :key="f.id" :value="String(f.id)">{{ f.name }}</option>
            </select>
          </div>

          <!-- 文件夹管理区 -->
          <div v-if="folders.length > 0" class="bm-folders">
            <div class="bm-folders-head">文件夹（{{ folders.length }}）</div>
            <div class="bm-folder-row" v-for="f in folders" :key="f.id">
              <span class="bm-folder-name" @click="filterFolder = f.id">📁 {{ f.name }}</span>
              <span class="bm-folder-count">{{ folderCount(f.id) }} 个书签</span>
              <button class="op-btn danger" title="删除文件夹（含其中的书签）" @click="onDeleteFolder(f)">✕</button>
            </div>
          </div>

          <div class="bm-list">
            <div v-if="filtered.length === 0" class="bm-empty">暂无书签，点「＋ 添加书签」或「📥 导入」</div>
            <div v-for="b in filtered" :key="b.id" class="bm-row">
              <div class="bm-info" @click="onOpen(b.url)">
                <span class="bm-title">{{ b.title }}</span>
                <span class="bm-url">{{ b.url }}</span>
              </div>
              <span class="bm-folder">{{ folderName(b.folder_id) }}</span>
              <div class="bm-ops">
                <button class="op-btn" title="打开" @click="onOpen(b.url)">↗</button>
                <button class="op-btn" title="编辑" @click="onEdit(b)">✏</button>
                <button class="op-btn danger" title="删除" @click="onDelete(b)">✕</button>
              </div>
            </div>
          </div>
        </div>
      </div>

      <!-- 编辑弹窗 -->
      <div v-if="editTarget" class="dialog-overlay inner" @click.self="editTarget = null">
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
            <label class="bm-field">
              <span>文件夹</span>
              <select v-model="editForm.folderId">
                <option value="none">未分类</option>
                <option v-for="f in folders" :key="f.id" :value="String(f.id)">{{ f.name }}</option>
              </select>
            </label>
            <div class="bm-actions">
              <button class="bm-cancel" @click="editTarget = null">取消</button>
              <button class="bm-save" @click="onSaveEdit">保存</button>
            </div>
          </div>
        </div>
      </div>

      <!-- 添加弹窗 -->
      <div v-if="addOpen" class="dialog-overlay inner" @click.self="addOpen = false">
        <div class="dialog bm-edit-dialog">
          <div class="dialog-header">
            <span>添加书签</span>
            <button class="close-btn" aria-label="关闭" @click="addOpen = false">×</button>
          </div>
          <div class="dialog-body">
            <label class="bm-field">
              <span>标题</span>
              <input v-model="addForm.title" type="text" spellcheck="false" placeholder="书签标题" />
            </label>
            <label class="bm-field">
              <span>网址</span>
              <input v-model="addForm.url" type="text" spellcheck="false" placeholder="https://..." />
            </label>
            <label class="bm-field">
              <span>文件夹</span>
              <select v-model="addForm.folderId">
                <option value="none">未分类</option>
                <option v-for="f in folders" :key="f.id" :value="String(f.id)">{{ f.name }}</option>
              </select>
            </label>
            <div class="bm-actions">
              <button class="bm-cancel" @click="addOpen = false">取消</button>
              <button class="bm-save" @click="onSaveAdd">添加</button>
            </div>
          </div>
        </div>
      </div>

      <!-- 删除文件夹确认弹窗 -->
      <div v-if="showFolderConfirm" class="dialog-overlay inner" @click.self="showFolderConfirm = false">
        <div class="dialog bm-edit-dialog">
          <div class="dialog-header">
            <span>删除文件夹</span>
            <button class="close-btn" aria-label="关闭" @click="showFolderConfirm = false">×</button>
          </div>
          <div class="dialog-body">
            <div class="bm-confirm-text">
              确定删除文件夹「{{ folderDeleteTarget?.name }}」？
              <br />其中的 {{ folderDeleteTarget ? folderCount(folderDeleteTarget.id) : 0 }} 个书签和子文件夹将一并删除，此操作不可恢复。
            </div>
            <div class="bm-actions">
              <button class="bm-cancel" @click="showFolderConfirm = false">取消</button>
              <button class="bm-save danger-btn" @click="doDeleteFolder">删除</button>
            </div>
          </div>
        </div>
      </div>

      </div>
  </Transition>
</template>

<style scoped>
/* 弹窗基础样式走全局（App.vue 的 .dialog-overlay/.dialog/.dialog-header/.dialog-body），
   这里只保留宽度差异 + 书签管理器专属元素，全部用主题 CSS 变量（深/浅色主题通用） */
.dialog-overlay.inner {
  z-index: 1100;
}
.bm-dialog {
  width: min(680px, calc(100vw - 48px));
}
.header-actions {
  display: flex;
  align-items: center;
  gap: 6px;
}
.h-btn {
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
  transition: background 0.15s;
  display: inline-flex;
  align-items: center;
}
.file-input {
  display: none;
}
.h-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}
.h-btn.accent {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.h-btn.accent:hover {
  opacity: 0.9;
}
.bm-toolbar {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.bm-search {
  flex: 1;
  height: 32px;
  padding: 0 10px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 7px;
  font-size: 13px;
  outline: none;
}
.bm-search:focus {
  border-color: var(--accent);
}
.bm-filter {
  height: 32px;
  padding: 0 8px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 7px;
  font-size: 13px;
  outline: none;
}
.bm-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-height: 200px;
  max-height: 46vh;
  overflow-y: auto;
  scrollbar-width: none;
}
.bm-list::-webkit-scrollbar {
  display: none;
}
.bm-empty {
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 40px 0;
}
.bm-folders {
  margin-bottom: 10px;
  padding: 8px;
  border: 1px solid var(--border-subtle);
  border-radius: 8px;
  background: var(--bg-hover);
}
.bm-folders-head {
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 6px;
  padding-left: 2px;
}
.bm-folder-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 6px;
  border-radius: 6px;
}
.bm-folder-row:hover {
  background: var(--bg-active);
}
.bm-folder-name {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-primary);
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.bm-folder-name:hover {
  color: var(--accent);
}
.bm-folder-count {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-muted);
}
.bm-confirm-text {
  font-size: 13px;
  color: var(--text-primary);
  line-height: 1.7;
  margin-bottom: 4px;
}
.danger-btn {
  background: var(--danger) !important;
  border-color: var(--danger) !important;
  color: #fff !important;
}
.bm-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 10px;
  border-radius: 7px;
  background: var(--bg-hover);
  border: 1px solid var(--border-subtle);
  transition: background 0.15s;
}
.bm-row:hover {
  background: var(--bg-active);
}
.bm-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  cursor: pointer;
}
.bm-title {
  font-size: 13px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.bm-url {
  font-size: 11px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.bm-folder {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--accent);
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  border-radius: 4px;
  padding: 2px 6px;
}
.bm-ops {
  display: flex;
  gap: 2px;
  flex-shrink: 0;
}
.op-btn {
  width: 26px;
  height: 26px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: 5px;
  cursor: pointer;
  font-size: 13px;
}
.op-btn:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}
.op-btn.danger:hover {
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
.bm-field input,
.bm-field select {
  height: 30px;
  padding: 0 10px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 13px;
  outline: none;
}
.bm-field input:focus,
.bm-field select:focus {
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
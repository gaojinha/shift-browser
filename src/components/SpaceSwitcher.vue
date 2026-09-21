<script setup lang="ts">
// 空间切换器（阶段 3.2 · 平台化）：工作区升级为"上下文隔离容器"。
// 每个空间独立标签集 + 主题色；切换 = 保存当前标签 → 恢复目标空间标签。
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，下拉展开时必须隐藏 webview，否则被页面盖住。
import { ref, onMounted, onUnmounted } from 'vue'
import { useWorkspacesStore } from '../stores/workspaces'
import { setWebviewVisible } from '../utils/webview'

const ws = useWorkspacesStore()
const open = ref(false)
const rootRef = ref<HTMLElement | null>(null)
// 行内重命名 / 删除确认状态
const editingId = ref<number | null>(null)
const editName = ref('')
const confirmDeleteId = ref<number | null>(null)

async function toggle() {
  open.value = !open.value
  editingId.value = null
  confirmDeleteId.value = null
  await setWebviewVisible(!open.value, 'space-switcher')
}
async function close() {
  if (!open.value) return
  open.value = false
  editingId.value = null
  confirmDeleteId.value = null
  await setWebviewVisible(true, 'space-switcher')
}
function pick(id: number | null) {
  close()
  ws.switchWorkspace(id)
}
async function addSpace() {
  const id = await ws.createWorkspace()
  await close()
  if (id != null) ws.switchWorkspace(id)
}
function startRename(id: number, name: string) {
  editingId.value = id
  editName.value = name
  confirmDeleteId.value = null
}
async function commitRename() {
  if (editingId.value != null && editName.value.trim()) {
    await ws.renameWorkspace(editingId.value, editName.value.trim())
  }
  editingId.value = null
}
function askDelete(id: number) {
  confirmDeleteId.value = id
  editingId.value = null
}
async function doDelete(id: number) {
  confirmDeleteId.value = null
  await ws.deleteWorkspace(id)
}
function onClickOutside(e: MouseEvent) {
  if (open.value && rootRef.value && !rootRef.value.contains(e.target as Node)) {
    close()
  }
}
onMounted(() => document.addEventListener('mousedown', onClickOutside))
onUnmounted(() => document.removeEventListener('mousedown', onClickOutside))
</script>

<template>
  <div ref="rootRef" class="space-switcher">
    <button class="space-btn" :title="'当前空间：' + ws.currentName" @click="toggle">
      <span class="space-dot" :style="{ background: ws.currentColor || 'var(--accent)' }"></span>
      <span class="space-name">{{ ws.currentName }}</span>
      <span class="space-caret">▾</span>
    </button>
    <div v-if="open" class="space-menu">
      <div class="space-item" :class="{ active: ws.currentId === null }" @click="pick(null)">
        <span class="space-dot" style="background: var(--accent)"></span>
        <span class="space-item-name">默认</span>
      </div>
      <div
        v-for="w in ws.workspaces"
        :key="w.id"
        class="space-item"
        :class="{ active: ws.currentId === w.id }"
      >
        <span class="space-dot" :style="{ background: w.color || 'var(--accent)' }"></span>
        <input
          v-if="editingId === w.id"
          v-model="editName"
          class="space-edit"
          @keydown.enter="commitRename"
          @keydown.esc="editingId = null"
          @blur="commitRename"
          @click.stop
        />
        <span v-else class="space-item-name" @click="pick(w.id)">{{ w.name }}</span>
        <button class="space-mini" title="重命名" @click.stop="startRename(w.id, w.name)">✎</button>
        <button
          v-if="confirmDeleteId !== w.id"
          class="space-mini danger"
          title="删除空间"
          @click.stop="askDelete(w.id)"
        >
          ✕
        </button>
        <button v-else class="space-mini danger" title="确认删除" @click.stop="doDelete(w.id)">
          确认
        </button>
      </div>
      <div class="space-divider"></div>
      <div class="space-item add" @click="addSpace">＋ 新建空间</div>
    </div>
  </div>
</template>

<style scoped>
.space-switcher {
  position: relative;
  flex-shrink: 0;
}
.space-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-secondary);
  color: var(--text-primary);
  cursor: pointer;
  font-size: 12px;
  max-width: 150px;
}
.space-btn:hover {
  background: var(--bg-hover);
}
.space-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  flex-shrink: 0;
}
.space-name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.space-caret {
  color: var(--text-muted);
  font-size: 10px;
}
.space-menu {
  position: absolute;
  top: 32px;
  left: 0;
  z-index: 3000;
  min-width: 200px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 8px;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
  padding: 6px;
}
.space-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 8px;
  border-radius: 6px;
  cursor: pointer;
}
.space-item:hover {
  background: var(--bg-hover);
}
.space-item.active {
  background: var(--bg-active);
}
.space-item-name {
  flex: 1;
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.space-item.add {
  color: var(--accent);
  font-size: 12px;
}
.space-edit {
  flex: 1;
  min-width: 0;
  height: 22px;
  border: 1px solid var(--accent);
  border-radius: 4px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  padding: 0 6px;
  outline: none;
}
.space-mini {
  flex-shrink: 0;
  width: 20px;
  height: 20px;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  font-size: 11px;
}
.space-mini:hover {
  background: var(--bg-active);
  color: var(--text-primary);
}
.space-mini.danger:hover {
  background: var(--danger);
  color: #fff;
}
.space-divider {
  height: 1px;
  background: var(--border-subtle);
  margin: 4px 0;
}
</style>

<script setup lang="ts">
// 标签分组管理弹窗
// 与 PasswordsDialog 一样：组件在 App.vue 始终挂载，用 watch(props.show) 控制显示副作用
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useGroupsStore } from '../stores/groups'
import { useTabsStore } from '../stores/tabs'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const groupsStore = useGroupsStore()
const tabsStore = useTabsStore()

const form = ref({ name: '', color: '#4a9eff' })
const showAdd = ref(false)
const editingId = ref<number | null>(null)
const editName = ref('')
const errorMsg = ref('')

const COLORS = ['#4a9eff', '#e53935', '#43a047', '#fb8c00', '#8e24aa', '#00acc1', '#6d4c41', '#546e7a']

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
watch(
  () => props.show,
  async (v) => {
    if (v) {
      await groupsStore.loadGroups()
    }
    await invoke('set_webview_visible', { visible: !v })
  }
)

function tabCount(groupId: number): number {
  return tabsStore.tabs.filter((t) => t.groupId === groupId).length
}

async function onCreate() {
  errorMsg.value = ''
  if (!form.value.name.trim()) {
    errorMsg.value = '分组名称必填'
    return
  }
  await groupsStore.createGroup(form.value.name.trim(), form.value.color)
  form.value = { name: '', color: '#4a9eff' }
  showAdd.value = false
}

function startEdit(g: { id: number; name: string }) {
  editingId.value = g.id
  editName.value = g.name
}

async function saveEdit(id: number) {
  if (!editName.value.trim()) return
  await groupsStore.updateGroup(id, { name: editName.value.trim() })
  editingId.value = null
}

async function onChangeColor(id: number, color: string) {
  await groupsStore.updateGroup(id, { color })
}

async function onToggleCollapse(g: { id: number; collapsed: boolean }) {
  await groupsStore.updateGroup(g.id, { collapsed: !g.collapsed })
}

async function onDelete(id: number) {
  // WebKitGTK 不显示原生 confirm，直接删除（组内标签回未分组）
  await groupsStore.deleteGroup(id)
  for (const t of tabsStore.tabs) {
    if (t.groupId === id) tabsStore.assignTabToGroup(t.id, null)
  }
}
</script>

<template>
  <div class="dialog-overlay" v-if="props.show">
    <div class="dialog">
      <div class="dialog-header">
        <span>标签分组</span>
        <button class="close-btn" @click="emit('close')">✕</button>
      </div>

      <div class="dialog-body">
        <div class="toolbar">
          <button class="add-btn" @click="showAdd = !showAdd">
            {{ showAdd ? '收起' : '+ 新建分组' }}
          </button>
        </div>

        <form v-if="showAdd" class="add-form" @submit.prevent="onCreate">
          <input v-model="form.name" placeholder="分组名称（如 工作、娱乐）" />
          <div class="color-row">
            <span class="color-label">颜色</span>
            <button
              v-for="c in COLORS"
              :key="c"
              class="color-dot"
              :class="{ selected: form.color === c }"
              :style="{ background: c }"
              type="button"
              @click="form.color = c"
            ></button>
          </div>
          <div class="form-error" v-if="errorMsg">{{ errorMsg }}</div>
          <button type="submit" class="save-btn">创建</button>
        </form>

        <div class="list" v-if="groupsStore.groups.length > 0">
          <div v-for="g in groupsStore.groups" :key="g.id" class="item">
            <div class="item-info">
              <span class="item-name" :style="{ borderLeftColor: g.color }">
                <template v-if="editingId === g.id">
                  <input v-model="editName" class="edit-input" @keydown.enter="saveEdit(g.id)" />
                  <button class="mini-btn" @click="saveEdit(g.id)">保存</button>
                </template>
                <template v-else>
                  {{ g.name }}
                  <span class="item-count">{{ tabCount(g.id) }} 个标签</span>
                </template>
              </span>
            </div>
            <div class="item-actions">
              <button
                class="mini-btn"
                :title="g.collapsed ? '已折叠' : '未折叠'"
                @click="onToggleCollapse(g)"
              >
                {{ g.collapsed ? '▶' : '▼' }}
              </button>
              <div class="color-mini-row">
                <button
                  v-for="c in COLORS"
                  :key="c"
                  class="color-dot mini"
                  :class="{ selected: g.color === c }"
                  :style="{ background: c }"
                  @click="onChangeColor(g.id, c)"
                ></button>
              </div>
              <button v-if="editingId !== g.id" class="mini-btn" @click="startEdit(g)">重命名</button>
              <button class="mini-btn danger" @click="onDelete(g.id)">删除</button>
            </div>
          </div>
        </div>
        <div v-else class="empty">还没有分组。右键标签 → 新建分组并移动，或点"+ 新建分组"</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}
.dialog {
  width: 520px;
  max-height: 80vh;
  background: #222;
  border-radius: 10px;
  border: 1px solid #444;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
.dialog-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  color: #e0e0e0;
  font-size: 14px;
  border-bottom: 1px solid #333;
}
.close-btn {
  border: none;
  background: transparent;
  color: #999;
  font-size: 14px;
  cursor: pointer;
}
.dialog-body {
  padding: 12px 16px;
  overflow-y: auto;
}
.toolbar {
  margin-bottom: 10px;
}
.add-btn {
  background: #3d3d3d;
  color: #e0e0e0;
  border: none;
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
}
.add-btn:hover {
  background: #4a4a4a;
}
.add-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: #2a2a2a;
  border-radius: 8px;
  padding: 12px;
  margin-bottom: 12px;
}
.add-form input,
.edit-input {
  border: 1px solid #444;
  background: #1e1e1e;
  color: #e0e0e0;
  border-radius: 6px;
  padding: 8px 10px;
  font-size: 13px;
  outline: none;
}
.add-form input:focus,
.edit-input:focus {
  border-color: #4a9eff;
}
.color-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.color-label {
  font-size: 12px;
  color: #888;
  margin-right: 4px;
}
.color-dot {
  width: 18px;
  height: 18px;
  border-radius: 50%;
  border: 2px solid transparent;
  cursor: pointer;
  padding: 0;
}
.color-dot.selected {
  border-color: #fff;
}
.color-dot.mini {
  width: 14px;
  height: 14px;
}
.form-error {
  color: #ff6b6b;
  font-size: 12px;
}
.save-btn {
  background: #4a9eff;
  color: #fff;
  border: none;
  border-radius: 6px;
  padding: 8px;
  font-size: 13px;
  cursor: pointer;
}
.save-btn:hover {
  background: #3a8eef;
}
.list {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  background: #2a2a2a;
  border-radius: 8px;
  padding: 10px 12px;
  gap: 8px;
}
.item-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.item-name {
  color: #e0e0e0;
  font-size: 13px;
  font-weight: 500;
  border-left: 3px solid #4a9eff;
  padding-left: 8px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.item-count {
  color: #888;
  font-size: 11px;
  font-weight: normal;
}
.item-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.color-mini-row {
  display: flex;
  align-items: center;
  gap: 3px;
}
.mini-btn {
  background: #3d3d3d;
  color: #ccc;
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.mini-btn:hover {
  background: #4a4a4a;
}
.mini-btn.danger:hover {
  background: #5c2b2b;
  color: #ff8a8a;
}
.empty {
  color: #666;
  font-size: 13px;
  text-align: center;
  padding: 24px 0;
}
</style>
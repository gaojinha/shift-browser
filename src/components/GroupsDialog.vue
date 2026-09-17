<script setup lang="ts">
// 标签分组管理弹窗
// 与 PasswordsDialog 一样：组件在 App.vue 始终挂载，用 watch(props.show) 控制显示副作用
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useGroupsStore } from '../stores/groups'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const groupsStore = useGroupsStore()
const tabsStore = useTabsStore()

const form = ref({ name: '', color: 'var(--accent)' })
const showAdd = ref(false)
const editingId = ref<number | null>(null)
const editName = ref('')
const errorMsg = ref('')

const COLORS = [
  'var(--accent)',
  '#e53935',
  '#43a047',
  '#fb8c00',
  '#8e24aa',
  '#00acc1',
  '#6d4c41',
  '#546e7a',
]

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        await groupsStore.loadGroups()
      }
    } catch (e) {
      console.error('[groups] 加载失败:', e)
      useToastStore().error('加载分组失败')
    } finally {
      if (my === seq) {
        await invoke('set_webview_visible', { visible: !v }).catch(() => {})
      }
    }
  },
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
  try {
    await groupsStore.createGroup(form.value.name.trim(), form.value.color)
    form.value = { name: '', color: 'var(--accent)' }
    showAdd.value = false
  } catch {
    // store 已 toast 提示，这里清空输入避免残留
    form.value = { name: '', color: 'var(--accent)' }
    showAdd.value = false
  }
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
  // 乐观更新 + 失败回滚：双击换色时第二次读到的是新值，不会重复发相同颜色
  const g = groupsStore.groups.find((x) => x.id === id)
  const prev = g?.color
  if (g) g.color = color
  try {
    await groupsStore.updateGroup(id, { color })
  } catch {
    if (g) g.color = prev ?? g.color
  }
}

async function onToggleCollapse(g: { id: number; collapsed: boolean }) {
  // 乐观翻转 + 失败回滚：双击折叠时第二次读到新值，不会重复发相同目标值
  const next = !g.collapsed
  g.collapsed = next
  try {
    await groupsStore.updateGroup(g.id, { collapsed: next })
  } catch {
    g.collapsed = !next
  }
}

async function onDelete(id: number) {
  // WebKitGTK 不显示原生 confirm，用前端确认弹窗（组内标签回未分组）
  // 删除失败时不清标签归属，避免 UI 与数据库脱节（分组还在但标签已清出组）
  try {
    await groupsStore.deleteGroup(id)
  } catch {
    return
  }
  for (const t of tabsStore.tabs) {
    if (t.groupId === id) tabsStore.assignTabToGroup(t.id, null)
  }
}

// 删除确认弹窗（防止误删分组）
const confirmTarget = ref<{ id: number; name: string } | null>(null)
function askDelete(g: { id: number; name: string }) {
  confirmTarget.value = { id: g.id, name: g.name }
}
function cancelDelete() {
  confirmTarget.value = null
}
</script>

<template>
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show">
    <div class="dialog">
      <div class="dialog-header">
        <span>标签分组</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
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
              <button v-if="editingId !== g.id" class="mini-btn" @click="startEdit(g)">
                重命名
              </button>
              <button class="mini-btn danger" @click="askDelete(g)">删除</button>
            </div>
          </div>
        </div>
        <div v-else-if="groupsStore.loading" class="empty loading-hint">加载中…</div>
        <div v-else class="empty">还没有分组。右键标签 → 新建分组并移动，或点"+ 新建分组"</div>
      </div>
    </div>

    <!-- 删除确认弹窗 -->
    <div v-if="confirmTarget" class="dialog-overlay inner" @click.self="cancelDelete">
      <div class="dialog confirm-dialog">
        <div class="dialog-header">
          <span>删除分组</span>
          <button class="close-btn" aria-label="关闭" @click="cancelDelete">×</button>
        </div>
        <div class="dialog-body">
          <p class="confirm-text">
            确定删除分组「{{ confirmTarget.name }}」吗？组内标签将回到未分组。
          </p>
          <div class="confirm-actions">
            <button class="bm-cancel" @click="cancelDelete">取消</button>
            <button class="bm-save danger" @click="onDelete(confirmTarget.id)">删除</button>
          </div>
        </div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
/* 分组弹窗比默认弹窗宽一点（520px） */
.dialog {
  width: min(520px, calc(100vw - 32px));
}
.toolbar {
  margin-bottom: 10px;
}
.add-btn {
  background: var(--bg-active);
  color: var(--text-primary);
  border: none;
  border-radius: 6px;
  padding: 6px 12px;
  font-size: 12px;
  cursor: pointer;
}
.add-btn:hover {
  background: var(--text-secondary);
}
.add-form {
  display: flex;
  flex-direction: column;
  gap: 8px;
  background: var(--bg-secondary);
  border-radius: 8px;
  padding: 12px;
  margin-bottom: 12px;
}
.add-form input,
.edit-input {
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-primary);
  border-radius: 6px;
  padding: 8px 10px;
  font-size: 13px;
  outline: none;
}
.add-form input:focus,
.edit-input:focus {
  border-color: var(--accent);
}
.color-row {
  display: flex;
  align-items: center;
  gap: 6px;
}
.color-label {
  font-size: 12px;
  color: var(--text-muted);
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
  border-color: var(--text-primary);
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
  background: var(--accent);
  color: var(--text-primary);
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
  background: var(--bg-secondary);
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
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
  border-left: 3px solid var(--accent);
  padding-left: 8px;
  display: flex;
  align-items: center;
  gap: 6px;
}
.item-count {
  color: var(--text-muted);
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
  background: var(--bg-active);
  color: var(--text-secondary);
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}
.mini-btn:hover {
  background: var(--text-secondary);
}
.mini-btn.danger:hover {
  background: #5c2b2b;
  color: #ff8a8a;
}
.empty {
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 24px 0;
}
/* 删除确认弹窗（嵌套在分组弹窗之上） */
.dialog-overlay.inner {
  z-index: 1100;
}
.confirm-dialog {
  width: min(360px, calc(100vw - 32px));
}
.confirm-text {
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.6;
  margin: 0 0 16px;
}
.confirm-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
.bm-cancel,
.bm-save {
  height: 28px;
  padding: 0 14px;
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  border: none;
}
.bm-cancel {
  background: var(--bg-active);
  color: var(--text-secondary);
}
.bm-save {
  background: var(--accent);
  color: #fff;
}
.bm-save:hover {
  filter: brightness(1.1);
}
.bm-save.danger {
  background: #c62828;
}
.bm-save.danger:hover {
  background: #b71c1c;
}
</style>

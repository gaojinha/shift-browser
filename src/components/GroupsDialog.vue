<script setup lang="ts">
// 标签分组管理弹窗
// 与 PasswordsDialog 一样：组件在 App.vue 始终挂载，用 watch(props.show) 控制显示副作用
import { ref, watch } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { useGroupsStore } from '../stores/groups'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
// 分组色板（单一来源）
import { UI } from '../constants'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const groupsStore = useGroupsStore()
const tabsStore = useTabsStore()

const form = ref({ name: '', color: 'var(--accent)' })
const showAdd = ref(false)
const editingId = ref<number | null>(null)
const editName = ref('')
const errorMsg = ref('')

// 分组色板来自 constants.ts（唯一来源）

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
        await setWebviewVisible(!v, 'groups-dialog')
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
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
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
              v-for="c in UI.groupPalette"
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
                  v-for="c in UI.groupPalette"
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
/* 通用 .add-btn/.add-form/.form-error/.save-btn 已收口至 styles/components.css */
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
/* 通用 .list/.item/.item-info/.item-actions/.mini-btn/.empty 已收口至 styles/components.css */
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
.color-mini-row {
  display: flex;
  align-items: center;
  gap: 3px;
}
/* 删除确认弹窗（嵌套在分组弹窗之上） */
.dialog-overlay.inner {
  z-index: 1100;
}
.confirm-dialog {
  width: min(360px, calc(100vw - 32px));
}
/* 通用 .confirm-text/.confirm-actions 已收口至 styles/components.css；
   通用 .bm-cancel/.bm-save（含 .danger）已收口至 styles/components.css */
</style>

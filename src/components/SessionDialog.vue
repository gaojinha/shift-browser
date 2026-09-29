<script setup lang="ts">
// 会话管理弹窗：列出/恢复/删除已保存的标签页会话，保存当前为命名会话
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住，
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import UiDialog from './ui/UiDialog.vue'
import type { Session, SessionTab } from '../types/session'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

/** 外层显隐（UiDialog 双向绑定：关即 emit close） */
const showModel = computed({
  get: () => props.show,
  set: (v: boolean) => {
    if (!v) emit('close')
  },
})

const tabsStore = useTabsStore()
const toastStore = useToastStore()

const list = ref<Session[]>([])
const loading = ref(false)
// 保存为命名会话
const saveName = ref('')
// 删除确认（WebKitGTK 下 window.confirm 不弹）
const deleteTarget = ref<Session | null>(null)
const showConfirm = ref(false)

// 弹窗显示时隐藏所有标签 webview，关闭时恢复
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        await loadList()
      } else {
        saveName.value = ''
        deleteTarget.value = null
        showConfirm.value = false
      }
    } catch (e) {
      console.error('[sessions] 加载失败:', e)
      toastStore.error('加载会话失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'session-dialog')
      }
    }
  },
)

async function loadList() {
  loading.value = true
  try {
    list.value = await invoke<Session[]>('list_sessions')
  } finally {
    loading.value = false
  }
}

/** 解析快照标签数（data 损坏时返回 0） */
function tabCount(s: Session): number {
  try {
    const tabs = JSON.parse(s.data) as SessionTab[]
    return Array.isArray(tabs) ? tabs.length : 0
  } catch {
    return 0
  }
}

/** 格式化时间（YYYY-MM-DD HH:mm） */
function formatTime(iso: string): string {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** 恢复会话（替换当前标签页：先清空再逐个打开） */
async function onRestore(s: Session) {
  try {
    if (tabsStore.tabs.length > 0) {
      await tabsStore.clearAllTabs()
    }
    const ok = await tabsStore.restoreSession(s)
    if (ok) {
      toastStore.success(`已恢复会话「${s.name}」`)
      emit('close')
    } else {
      toastStore.error('会话内容为空或已损坏')
    }
  } catch (e) {
    console.error('[sessions] 恢复失败:', e)
    toastStore.error('恢复会话失败')
  }
}

/** 删除会话（自定义确认） */
function onDelete(s: Session) {
  deleteTarget.value = s
  showConfirm.value = true
}

async function doDelete() {
  const target = deleteTarget.value
  if (!target) return
  showConfirm.value = false
  deleteTarget.value = null
  try {
    await invoke('delete_session', { id: target.id })
    list.value = list.value.filter((x) => x.id !== target.id)
    toastStore.success('会话已删除')
  } catch (e) {
    console.error('[sessions] 删除失败:', e)
    toastStore.error('删除会话失败')
  }
}

/** 保存当前标签页为命名会话 */
async function onSave() {
  const name = saveName.value.trim()
  if (!name) {
    toastStore.error('请输入会话名称')
    return
  }
  if (tabsStore.tabs.length === 0) {
    toastStore.error('当前没有标签页可保存')
    return
  }
  const id = await tabsStore.saveNamedSession(name)
  if (id !== null) {
    saveName.value = ''
    await loadList()
    toastStore.success(`已保存会话「${name}」`)
  } else {
    toastStore.error('保存会话失败')
  }
}
</script>

<template>
  <!-- 会话管理：外层 UiDialog（560px 宽）；删除确认是内层 UiDialog（已试点） -->
  <UiDialog v-model:open="showModel" title="会话管理" width="min(560px, calc(100vw - 48px))">
    <div class="ss-body">
          <div class="ss-save">
            <input
              v-model="saveName"
              type="text"
              class="ss-input"
              placeholder="会话名称（保存当前标签页）"
              spellcheck="false"
              @keydown.enter="onSave"
            />
            <button class="ss-btn accent" @click="onSave">保存当前</button>
          </div>

          <div v-if="loading" class="ss-empty">加载中…</div>
          <div v-else-if="list.length === 0" class="ss-empty">还没有保存的会话</div>
          <div v-else class="ss-list">
            <div v-for="s in list" :key="s.id" class="ss-row">
              <div class="ss-info">
                <div class="ss-name">{{ s.name }}</div>
                <div class="ss-meta">
                  {{ tabCount(s) }} 个标签 · {{ formatTime(s.created_at) }}
                </div>
              </div>
              <div class="ss-ops">
                <button class="op-btn" title="恢复此会话" @click="onRestore(s)">↩ 恢复</button>
                <button class="op-btn danger" title="删除会话" @click="onDelete(s)">✕</button>
              </div>
            </div>
          </div>
        </div>

        <!-- 删除确认弹窗（shadcn 试点：UiDialog；Esc/点遮罩关闭与焦点管理由 Reka 接管） -->
        <UiDialog
          v-model:open="showConfirm"
          title="删除会话"
          :description="`确定删除会话「${deleteTarget?.name}」？此操作不可恢复。`"
        >
          <template #footer>
            <button class="confirm-btn cancel" @click="showConfirm = false">取消</button>
            <button class="confirm-btn ok" @click="doDelete">删除</button>
          </template>
        </UiDialog>
  </UiDialog>
</template>

<style scoped>
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留会话业务样式 */
.ss-body {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 220px;
}
.ss-save {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}
.ss-input {
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
.ss-input:focus {
  border-color: var(--accent);
}
.ss-btn {
  height: 32px;
  padding: 0 14px;
  border: 1px solid var(--border-color);
  border-radius: 7px;
  background: var(--bg-hover);
  color: var(--text-secondary);
  font-size: 12px;
  cursor: pointer;
  flex-shrink: 0;
}
.ss-btn.accent {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}
.ss-btn.accent:hover {
  opacity: 0.9;
}
.ss-empty {
  color: var(--text-muted);
  font-size: 13px;
  text-align: center;
  padding: 40px 0;
}
.ss-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  overflow-y: auto;
  max-height: 48vh;
}
.ss-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 10px;
  border-radius: 7px;
  background: var(--bg-hover);
  border: 1px solid var(--border-subtle);
}
.ss-row:hover {
  background: var(--bg-active);
}
.ss-info {
  flex: 1;
  min-width: 0;
}
.ss-name {
  font-size: 13px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ss-meta {
  font-size: 11px;
  color: var(--text-muted);
  margin-top: 2px;
}
.ss-ops {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}
.op-btn {
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--border-color);
  background: var(--bg-primary);
  color: var(--text-secondary);
  border-radius: 6px;
  cursor: pointer;
  font-size: 12px;
}
.op-btn:hover {
  color: var(--accent);
  border-color: var(--accent);
}
.op-btn.danger:hover {
  color: var(--danger);
  border-color: var(--danger);
}
/* 通用 .confirm-text/.confirm-actions/.confirm-btn 已收口至 styles/components.css */
</style>

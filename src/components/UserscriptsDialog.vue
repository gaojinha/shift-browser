<script setup lang="ts">
// 用户脚本管理（Tampermonkey 风格 · 阶段3.2 平台化）
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗打开时必须隐藏 webview。
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { useToastStore } from '../stores/toast'
import UiDialog from './ui/UiDialog.vue'

interface Userscript {
  id: number
  name: string
  matches: string
  code: string
  enabled: boolean
  created_at: string
}

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()
const toast = useToastStore()

/** 外层显隐（UiDialog 双向绑定：关即 emit close） */
const showModel = computed({
  get: () => props.show,
  set: (v: boolean) => {
    if (!v) emit('close')
  },
})

const list = ref<Userscript[]>([])
const editing = ref<Userscript | null>(null)
const form = ref({ name: '', matches: '*://*/*', code: '' })

async function load() {
  try {
    list.value = await invoke<Userscript[]>('list_userscripts')
  } catch (e) {
    console.error('[userscripts] 加载失败:', e)
    toast.error('加载用户脚本失败')
  }
}

watch(
  () => props.show,
  async (v) => {
    await setWebviewVisible(!v, 'userscripts-dialog')
    if (v) {
      startAdd()
      load()
    }
  },
)

function startAdd() {
  editing.value = null
  form.value = { name: '', matches: '*://*/*', code: '' }
}
function startEdit(u: Userscript) {
  editing.value = u
  form.value = { name: u.name, matches: u.matches, code: u.code }
}
async function save() {
  if (!form.value.name.trim() || !form.value.code.trim()) {
    toast.error('名称与代码不能为空')
    return
  }
  try {
    if (editing.value) {
      await invoke('update_userscript', {
        id: editing.value.id,
        name: form.value.name,
        matches: form.value.matches,
        code: form.value.code,
        enabled: null,
      })
    } else {
      await invoke('add_userscript', {
        name: form.value.name,
        matches: form.value.matches,
        code: form.value.code,
      })
    }
    startAdd()
    await load()
    toast.success('已保存（刷新页面或重开标签生效）')
  } catch (e) {
    console.error('[userscripts] 保存失败:', e)
    toast.error('保存失败：' + e)
  }
}
async function toggle(u: Userscript) {
  try {
    await invoke('update_userscript', {
      id: u.id,
      name: null,
      matches: null,
      code: null,
      enabled: !u.enabled,
    })
    await load()
  } catch (e) {
    console.error('[userscripts] 切换失败:', e)
  }
}
async function remove(u: Userscript) {
  try {
    await invoke('delete_userscript', { id: u.id })
    if (editing.value?.id === u.id) startAdd()
    await load()
  } catch (e) {
    console.error('[userscripts] 删除失败:', e)
  }
}
</script>

<template>
  <!-- 用户脚本：外层 UiDialog（760px 宽）；左右分栏原样搬进默认槽 -->
  <UiDialog
    v-model:open="showModel"
    title="用户脚本"
    description="Tampermonkey 风格 · 按 matches 匹配页面注入"
    width="min(760px, calc(100vw - 48px))"
  >
    <div class="us-body">
        <div class="us-list">
          <button class="us-btn primary full" @click="startAdd">＋ 新建脚本</button>
          <div v-if="list.length === 0" class="us-empty">还没有用户脚本</div>
          <div
            v-for="u in list"
            :key="u.id"
            class="us-item"
            :class="{ active: editing?.id === u.id }"
          >
            <input type="checkbox" :checked="u.enabled" title="启用/停用" @change="toggle(u)" />
            <div class="us-meta" @click="startEdit(u)">
              <div class="us-name">{{ u.name }}</div>
              <div class="us-match">{{ u.matches }}</div>
            </div>
            <button class="us-btn danger" title="删除" @click="remove(u)">✕</button>
          </div>
        </div>
        <div class="us-form">
          <input v-model="form.name" class="us-input" placeholder="名称" spellcheck="false" />
          <input
            v-model="form.matches"
            class="us-input"
            placeholder="匹配模式（每行一条，如 *://*.example.com/*）"
            spellcheck="false"
          />
          <textarea
            v-model="form.code"
            class="us-code"
            placeholder="// 用户脚本 JS 代码（document_end 注入）"
            spellcheck="false"
          ></textarea>
          <div class="us-actions">
            <button class="us-btn primary" @click="save">
              {{ editing ? '保存修改' : '添加' }}
            </button>
            <button v-if="editing" class="us-btn" @click="startAdd">取消编辑</button>
          </div>
        </div>
    </div>
  </UiDialog>
</template>

<style scoped>
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留用户脚本业务样式 */
.us-body {
  flex: 1;
  display: flex;
  min-height: 0;
}
.us-list {
  width: 300px;
  flex-shrink: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
  border-right: 1px solid var(--border-subtle);
  overflow-y: auto;
}
.us-empty {
  color: var(--text-muted);
  font-size: 12px;
  text-align: center;
  padding: 16px;
}
.us-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px;
  border-radius: 6px;
  background: var(--bg-primary);
}
.us-item.active {
  outline: 1px solid var(--accent);
}
.us-meta {
  flex: 1;
  min-width: 0;
  cursor: pointer;
}
.us-name {
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.us-match {
  font-size: 10px;
  color: var(--text-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.us-form {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  min-width: 0;
}
.us-input,
.us-code {
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 12px;
  padding: 8px;
  outline: none;
}
.us-code {
  flex: 1;
  min-height: 200px;
  font-family: monospace;
  resize: none;
}
.us-input:focus,
.us-code:focus {
  border-color: var(--accent);
}
.us-actions {
  display: flex;
  gap: 8px;
}
.us-btn {
  height: 28px;
  padding: 0 12px;
  border: 1px solid var(--border-color);
  border-radius: 6px;
  background: var(--bg-primary);
  color: var(--text-secondary);
  cursor: pointer;
  font-size: 12px;
}
.us-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.us-btn.primary {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
}
.us-btn.danger:hover {
  background: var(--danger);
  color: #fff;
}
.us-btn.full {
  width: 100%;
}
</style>

<script setup lang="ts">
// 密码管理弹窗
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { onMounted, onUnmounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { usePasswordsStore } from '../stores/passwords'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const passwordsStore = usePasswordsStore()

const form = ref({ domain: '', username: '', password: '', notes: '' })
const showAdd = ref(false)
const revealId = ref<number | null>(null)
const revealPassword = ref('')
const errorMsg = ref('')

onMounted(async () => {
  await passwordsStore.loadPasswords()
  await invoke('set_webview_visible', { visible: false })
})

onUnmounted(async () => {
  await invoke('set_webview_visible', { visible: true })
})

async function onAdd() {
  errorMsg.value = ''
  if (!form.value.domain || !form.value.password) {
    errorMsg.value = '域名和密码必填'
    return
  }
  await passwordsStore.savePassword(
    form.value.domain,
    form.value.username || null,
    form.value.password,
    form.value.notes || null
  )
  form.value = { domain: '', username: '', password: '', notes: '' }
  showAdd.value = false
}

async function onReveal(domain: string, id: number) {
  if (revealId.value === id) {
    revealId.value = null
    revealPassword.value = ''
    return
  }
  const entry = await passwordsStore.getPassword(domain)
  if (entry) {
    revealId.value = id
    revealPassword.value = entry.password
  }
}

async function onDelete(id: number) {
  await passwordsStore.deletePassword(id)
  if (revealId.value === id) {
    revealId.value = null
    revealPassword.value = ''
  }
}
</script>

<template>
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <span>密码管理</span>
        <button class="close-btn" @click="emit('close')">✕</button>
      </div>

      <div class="dialog-body">
        <div class="toolbar">
          <button class="add-btn" @click="showAdd = !showAdd">
            {{ showAdd ? '收起' : '+ 添加密码' }}
          </button>
        </div>

        <form v-if="showAdd" class="add-form" @submit.prevent="onAdd">
          <input v-model="form.domain" placeholder="域名（如 example.com）" />
          <input v-model="form.username" placeholder="用户名（可选）" />
          <input v-model="form.password" type="password" placeholder="密码" />
          <input v-model="form.notes" placeholder="备注（可选）" />
          <div class="form-error" v-if="errorMsg">{{ errorMsg }}</div>
          <button type="submit" class="save-btn">保存</button>
        </form>

        <div class="list" v-if="passwordsStore.list.length > 0">
          <div v-for="p in passwordsStore.list" :key="p.id" class="item">
            <div class="item-info">
              <span class="item-domain">{{ p.domain }}</span>
              <span class="item-username">{{ p.username || '无用户名' }}</span>
            </div>
            <div class="item-actions">
              <span v-if="revealId === p.id" class="revealed">{{ revealPassword }}</span>
              <button class="mini-btn" @click="onReveal(p.domain, p.id)">
                {{ revealId === p.id ? '隐藏' : '查看' }}
              </button>
              <button class="mini-btn danger" @click="onDelete(p.id)">删除</button>
            </div>
          </div>
        </div>
        <div v-else class="empty">还没有保存的密码</div>
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
  width: 480px;
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
.add-form input {
  border: 1px solid #444;
  background: #1e1e1e;
  color: #e0e0e0;
  border-radius: 6px;
  padding: 8px 10px;
  font-size: 13px;
  outline: none;
}
.add-form input:focus {
  border-color: #4a9eff;
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
}
.item-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.item-domain {
  color: #e0e0e0;
  font-size: 13px;
  font-weight: 500;
}
.item-username {
  color: #888;
  font-size: 12px;
}
.item-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.revealed {
  color: #4a9eff;
  font-size: 12px;
  font-family: monospace;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini-btn {
  background: #3d3d3d;
  color: #ccc;
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
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
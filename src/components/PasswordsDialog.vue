<script setup lang="ts">
// 密码管理弹窗
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { ref, watch } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { usePasswordsStore } from '../stores/passwords'
import { useToastStore } from '../stores/toast'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const passwordsStore = usePasswordsStore()
const toastStore = useToastStore()

// 弹窗显示时隐藏所有标签 webview（child 盖在 Vue UI 上），关闭时恢复
// 注意：不能用 onMounted——组件在 App.vue 里始终挂载，onMounted 只在应用启动时执行一次
// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview，
// 否则弹窗被 child 盖住且遮罩拦截点击，形成死锁
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      if (v) {
        await passwordsStore.loadPasswords()
      } else {
        // 关闭时清空已查看的明文，避免再次打开时密码还留在 UI 上
        revealId.value = null
        revealPassword.value = ''
      }
    } catch (e) {
      console.error('[passwords] 加载失败:', e)
      toastStore.error('加载密码失败')
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'passwords-dialog')
      }
    }
  },
)

const form = ref({ domain: '', username: '', password: '', notes: '' })
const showAdd = ref(false)
const revealId = ref<number | null>(null)
const revealPassword = ref('')
const errorMsg = ref('')

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
    form.value.notes || null,
  )
  form.value = { domain: '', username: '', password: '', notes: '' }
  showAdd.value = false
}

async function onReveal(id: number) {
  if (revealId.value === id) {
    revealId.value = null
    revealPassword.value = ''
    return
  }
  // 按 id 查（同域名多账号时精确定位），不再按域名 LIMIT 1
  try {
    const entry = await passwordsStore.getPasswordById(id)
    if (entry) {
      revealId.value = id
      revealPassword.value = entry.password
    }
  } catch (e) {
    console.error('[passwords] 查看密码失败:', e)
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
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog">
      <div class="dialog-header">
        <span>密码管理</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
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
              <button class="mini-btn" @click="onReveal(p.id)">
                {{ revealId === p.id ? '隐藏' : '查看' }}
              </button>
              <button class="mini-btn danger" @click="onDelete(p.id)">删除</button>
            </div>
          </div>
        </div>
        <div v-else-if="!passwordsStore.loaded" class="empty loading-hint">加载中…</div>
        <div v-else class="empty">还没有保存的密码</div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
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
.add-form input {
  border: 1px solid var(--border-color);
  background: var(--bg-secondary);
  color: var(--text-primary);
  border-radius: 6px;
  padding: 8px 10px;
  font-size: 13px;
  outline: none;
}
.add-form input:focus {
  border-color: var(--accent);
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
}
.item-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.item-domain {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
}
.item-username {
  color: var(--text-muted);
  font-size: 12px;
}
.item-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}
.revealed {
  color: var(--accent);
  font-size: 12px;
  font-family: monospace;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mini-btn {
  background: var(--bg-active);
  color: var(--text-secondary);
  border: none;
  border-radius: 4px;
  padding: 4px 8px;
  font-size: 12px;
  cursor: pointer;
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
</style>

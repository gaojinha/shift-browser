<script setup lang="ts">
// 密码管理弹窗
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗会被页面盖住。
// 因此挂载时隐藏所有标签 webview，卸载时恢复。
import { computed, ref, watch } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { usePasswordsStore } from '../stores/passwords'
import { useToastStore } from '../stores/toast'
import { useConfirmDelete } from '../composables/useConfirmDelete'
import UiDialog from './ui/UiDialog.vue'

const props = defineProps<{ show: boolean }>()
const emit = defineEmits<{ (e: 'close'): void }>()

/** 外层显隐（UiDialog 双向绑定：关即 emit close） */
const showModel = computed({
  get: () => props.show,
  set: (v: boolean) => {
    if (!v) emit('close')
  },
})

const passwordsStore = usePasswordsStore()
const toastStore = useToastStore()
const confirmDelete = useConfirmDelete()

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
        query.value = ''
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

// C-2 搜索框：客户端实时过滤域名/用户名/备注，不新增后端调用
const query = ref('')
const filteredList = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return passwordsStore.list
  return passwordsStore.list.filter((p) =>
    [p.domain, p.username ?? '', p.notes ?? ''].some((f) => f.toLowerCase().includes(q)),
  )
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
  // 安全：后端仅允许查看当前网站的密码（防页面脚本枚举 id 捞全表），
  // 跨站查看被拒时给提示，让用户先到该站再查看
  try {
    const entry = await passwordsStore.getPasswordById(id)
    if (entry) {
      revealId.value = id
      revealPassword.value = entry.password
    } else {
      toastStore.show('仅允许查看当前网站的密码（请先打开该网站）', 'info')
    }
  } catch (e) {
    console.error('[passwords] 查看密码失败:', e)
  }
}

async function onDelete(id: number) {
  // 不可恢复：二次点击确认，防误触一点即删
  if (!confirmDelete.arm(id)) return
  await passwordsStore.deletePassword(id)
  if (revealId.value === id) {
    revealId.value = null
    revealPassword.value = ''
  }
}
</script>

<template>
  <!-- 密码管理：外层 UiDialog；添加按钮进 header-actions 槽 -->
  <UiDialog v-model:open="showModel" title="密码管理" width="min(520px, calc(100vw - 32px))">
    <template #actions>
      <button class="add-btn" @click="showAdd = !showAdd">
        {{ showAdd ? '收起' : '+ 添加密码' }}
      </button>
    </template>

        <input
          v-model="query"
          class="search-input"
          type="search"
          placeholder="搜索域名 / 用户名 / 备注"
          aria-label="搜索密码"
          spellcheck="false"
        />

        <form v-if="showAdd" class="add-form" @submit.prevent="onAdd">
          <input v-model="form.domain" placeholder="域名（如 example.com）" />
          <input v-model="form.username" placeholder="用户名（可选）" />
          <input v-model="form.password" type="password" placeholder="密码" />
          <input v-model="form.notes" placeholder="备注（可选）" />
          <div class="form-error" v-if="errorMsg">{{ errorMsg }}</div>
          <button type="submit" class="save-btn">保存</button>
        </form>

        <div class="list" v-if="filteredList.length > 0">
          <div v-for="p in filteredList" :key="p.id" class="item">
            <div class="item-info">
              <span class="item-domain">{{ p.domain }}</span>
              <span class="item-username">{{ p.username || '无用户名' }}</span>
            </div>
            <div class="item-actions">
              <span v-if="revealId === p.id" class="revealed">{{ revealPassword }}</span>
              <button class="mini-btn" @click="onReveal(p.id)">
                {{ revealId === p.id ? '隐藏' : '查看' }}
              </button>
              <button
                class="mini-btn danger"
                :class="{ armed: confirmDelete.isPending(p.id) }"
                @click="onDelete(p.id)"
              >
                {{ confirmDelete.isPending(p.id) ? '确认删除' : '删除' }}
              </button>
            </div>
          </div>
        </div>
        <div v-else-if="!passwordsStore.loaded" class="empty loading-hint">加载中…</div>
        <div v-else-if="passwordsStore.list.length === 0" class="empty">还没有保存的密码</div>
        <div v-else class="empty">无匹配</div>
  </UiDialog>
</template>

<style scoped>
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留密码业务样式 */
/* 通用 .add-btn/.add-form/.form-error/.save-btn 已收口至 styles/components.css */
/* 通用 .list/.item/.item-info/.item-actions/.mini-btn/.empty 已收口至 styles/components.css */
.item-domain {
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 500;
}
.item-username {
  color: var(--text-muted);
  font-size: 12px;
}
.revealed {
  color: var(--accent);
  font-size: 12px;
  font-family: monospace;
  max-width: 140px;
  overflow: hidden;
  text-overflow: ellipsis;
}
.search-input {
  display: block;
  width: 100%;
  box-sizing: border-box;
  height: 30px;
  margin-bottom: 12px;
  padding: 0 10px;
  background: var(--bg-primary);
  color: var(--text-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-xs);
  font-size: 13px;
  outline: none;
}
.search-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-ring);
}
</style>

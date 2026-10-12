<script setup lang="ts">
// 权限请求提示（安全 #2 · 2026-09-22）
// 由 App.vue 监听 permission-request 事件后挂载；回答后调 respond_permission。
// 与 CertWarning 同套机制：child webview 是 GTK 部件盖在 Vue UI 上，弹出时隐藏 webview。
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
import { adviceFor } from '../utils/advice'
import { useToastStore } from '../stores/toast'

interface PermReq {
  id: number
  tabId: string
  host: string
  kind: string
  kindZh: string
}

const props = defineProps<{ info: PermReq | null }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const toastStore = useToastStore()
const busy = ref(false)
const remember = ref(false)

/** 各权限一句话用途+建议（单源见 utils/advice，组件只展示） */

watch(
  () => props.info,
  async (v) => {
    remember.value = false
    try {
      await setWebviewVisible(!v, 'permission-prompt')
    } catch (e) {
      console.warn('[perm] webview 显隐失败:', e)
    }
  },
)

async function answer(allow: boolean) {
  if (!props.info || busy.value) return
  busy.value = true
  try {
    await invoke('respond_permission', {
      id: props.info.id,
      allow,
      remember: remember.value,
      kind: props.info.kind,
      host: props.info.host,
    })
    toastStore.show(
      allow ? `已允许 ${props.info.kindZh}${remember.value ? '（已记住）' : ''}` : `已拒绝 ${props.info.kindZh}`,
      'info',
    )
    emit('close')
  } catch (e) {
    toastStore.show('权限处理失败：' + String(e), 'error')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div v-if="info" class="perm-mask">
    <div class="perm-box">
      <div class="perm-head">
        <span class="perm-ico">🛡️</span>
        <h3>网站请求权限</h3>
      </div>
      <p class="perm-lead">
        <b class="mono">{{ info.host }}</b> 请求使用 <b class="hl">{{ info.kindZh }}</b>。
      </p>
      <p class="perm-use">{{ adviceFor(info.kind).use }}</p>
      <p class="perm-tip">
        {{ adviceFor(info.kind).advice }}默认拒绝，避免被静默授权。
      </p>
      <label class="perm-remember">
        <input v-model="remember" type="checkbox" />
        记住我的选择（可在证书/权限设置中撤销）
      </label>
      <div class="perm-actions">
        <button class="btn-deny" :disabled="busy" @click="answer(false)">拒绝</button>
        <button class="btn-allow" :disabled="busy" @click="answer(true)">允许</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.perm-mask {
  position: fixed;
  inset: 0;
  z-index: var(--z-security);
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(10, 14, 24, 0.9);
  backdrop-filter: blur(6px);
}
.perm-box {
  /* 安全类特例：440px 宽 + 大内边距，显眼压住一切，不跟普通弹窗统一 */
  width: 440px;
  max-width: 92vw;
  background: rgba(28, 40, 64, 0.98);
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: var(--radius-xl);
  padding: 24px 26px;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.5);
}
.perm-head {
  display: flex;
  align-items: center;
  gap: 10px;
}
.perm-ico {
  font-size: 24px;
}
.perm-head h3 {
  margin: 0;
  font-size: var(--text-md);
  color: #fff;
}
.perm-lead {
  margin: 14px 0 6px;
  font-size: var(--text-base);
  color: var(--text-secondary);
  line-height: var(--leading-relaxed);
}
.mono {
  font-family: var(--font-mono);
}
.hl {
  color: var(--warning);
}
.perm-tip {
  margin: 0 0 var(--space-md);
  font-size: var(--text-sm);
  color: var(--text-muted);
  line-height: var(--leading-base);
}
.perm-use {
  margin: 0 0 6px;
  font-size: var(--text-sm);
  color: var(--text-secondary);
  line-height: var(--leading-base);
}
.perm-remember {
  display: flex;
  align-items: center;
  gap: var(--space-sm);
  font-size: var(--text-sm);
  color: var(--text-muted);
  margin-bottom: var(--space-lg);
  cursor: pointer;
}
.perm-actions {
  display: flex;
  gap: 10px;
}
.btn-deny,
.btn-allow {
  flex: 1;
  padding: 11px;
  border-radius: var(--radius-md);
  font-size: var(--text-md);
  cursor: pointer;
}
/* 以下按钮色为安全深蓝底专用，深浅主题下底色不变 */
.btn-deny {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
  border: 1px solid rgba(255, 255, 255, 0.2);
}
.btn-deny:hover {
  background: rgba(255, 255, 255, 0.18);
}
.btn-allow {
  background: rgba(74, 158, 255, 0.3);
  color: #cfe4ff;
  border: 1px solid rgba(74, 158, 255, 0.5);
}
.btn-allow:hover {
  background: rgba(74, 158, 255, 0.45);
}
.btn-deny:disabled,
.btn-allow:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>

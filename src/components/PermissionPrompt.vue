<script setup lang="ts">
// 权限请求提示（安全 #2 · 2026-09-22）
// 由 App.vue 监听 permission-request 事件后挂载；回答后调 respond_permission。
// 与 CertWarning 同套机制：child webview 是 GTK 部件盖在 Vue UI 上，弹出时隐藏 webview。
import { ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { setWebviewVisible } from '../utils/webview'
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

/** 各权限一句话用途+建议：用户看不懂英文 kind，必须翻成人话 */
const KIND_ADVICE: Record<string, { use: string; advice: string }> = {
  media: {
    use: '网站能打开你的摄像头、麦克风，看到听到你这边。',
    advice: '建议拒绝；视频会议等确实需要时再允许，用完在设置里删掉。',
  },
  geolocation: {
    use: '网站能知道你的大概位置（城市级）。',
    advice: '建议拒绝；地图导航确实需要时再允许。',
  },
  notification: {
    use: '网站能给你推送消息通知。',
    advice: '按需允许；不认识的站建议拒绝，免得广告轰炸。',
  },
  'pointer-lock': {
    use: '网站能锁定鼠标指针（常见于网页游戏、全屏应用）。',
    advice: '玩游戏时允许，平时拒绝。',
  },
  'device-info': {
    use: '网站能读取设备信息，可能被用来追踪识别你。',
    advice: '建议拒绝。',
  },
  other: {
    use: '网站请求了一项不常见的权限。',
    advice: '不认识就拒绝。',
  },
}

function adviceFor(kind: string) {
  return KIND_ADVICE[kind] ?? KIND_ADVICE.other
}

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
  width: 440px;
  max-width: 92vw;
  background: rgba(28, 40, 64, 0.98);
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 16px;
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
  font-size: 16px;
  color: #fff;
}
.perm-lead {
  margin: 14px 0 6px;
  font-size: 13px;
  color: #ddd;
  line-height: 1.7;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
}
.hl {
  color: #ffb14e;
}
.perm-tip {
  margin: 0 0 12px;
  font-size: 12px;
  color: #93a4bd;
  line-height: 1.6;
}
.perm-use {
  margin: 0 0 6px;
  font-size: 12px;
  color: #c9d6e8;
  line-height: 1.6;
}
.perm-remember {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: #bbb;
  margin-bottom: 16px;
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
  border-radius: 12px;
  font-size: 14px;
  cursor: pointer;
}
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

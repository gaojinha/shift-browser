<script setup lang="ts">
// 证书错误拦截页（P2）：TLS 校验失败时弹"危险站点"警告
// 由 App.vue 监听 tls-error 事件后挂载；"继续前往"= 调 accept_cert_exception 放行并重载
import { ref, watch } from 'vue'
import { invokeOk, errText } from '../utils/invoke'
import { setWebviewVisible } from '../utils/webview'
import { verdictFor } from '../utils/advice'
import { useToastStore } from '../stores/toast'

interface TlsErrorPayload {
  tabId: string
  host: string
  port: number
  flags: number
  reasons: string[]
  fingerprint: string
  subject: string
  issuer: string
  notAfter: string | null
}

const props = defineProps<{ info: TlsErrorPayload | null }>()
const emit = defineEmits<{ (e: 'close'): void }>()

const toastStore = useToastStore()
const busy = ref(false)
const advanced = ref(false) // 是否已展开"高级"细节

// child webview 是 GTK 部件盖在 Vue UI 上：警告出现时隐藏之，关闭时恢复
// 安全警告必须压住所有业务浮层，用统一来源标识参与全局计数
watch(
  () => props.info,
  async (v) => {
    try {
      await setWebviewVisible(!v, 'cert-warning')
    } catch (e) {
      console.warn('[cert] webview 显隐失败:', e)
    }
  },
)

async function viewCert() {
  // 打开证书中心看详情（复用统一入口）
  advanced.value = true
}

async function proceed() {
  if (!props.info || busy.value) return
  busy.value = true
  try {
    await invokeOk('accept_cert_exception', {
      id: props.info.tabId,
      host: props.info.host,
      port: props.info.port,
      fingerprint: props.info.fingerprint,
      reason: '用户在警告页选择继续前往',
    })
    toastStore.show('已放行该证书（可在证书中心撤销）', 'info')
    emit('close')
  } catch (e) {
    toastStore.show('放行失败：' + errText(e), 'error')
  } finally {
    busy.value = false
  }
}

async function goBack() {
  // 关闭警告并复位到启动页（不加载失败站点）
  try {
    await setWebviewVisible(true, 'cert-warning')
  } catch {
    /* ignore */
  }
  emit('close')
}

/** 各错误原因一句话研判（单源见 utils/advice，组件只展示） */
</script>

<template>
  <div v-if="info" class="warn-mask">
    <div class="warn-box">
      <div class="warn-head">
        <span class="warn-ico">⚠️</span>
        <h2>您的连接不是私密连接</h2>
      </div>

      <p class="warn-lead">
        攻击者可能会试图从 <b class="mono">{{ info.host }}</b> 窃取您的信息（例如密码、消息或信用卡）。
      </p>

      <div class="warn-reasons">
        <div v-for="(r, i) in info.reasons" :key="i">· {{ r }}</div>
      </div>

      <div class="warn-verdicts">
        <div v-for="(r, i) in info.reasons" :key="'v' + i">→ {{ verdictFor(r) }}</div>
        <div class="warn-consequence">继续前往后，中间人能看到你的明文；放行记录进证书中心，可随时撤销。</div>
      </div>

      <div class="warn-actions">
        <button class="btn-primary" @click="goBack">返回（推荐）</button>
        <button class="btn-danger" :disabled="busy" @click="proceed">
          {{ busy ? '处理中…' : '继续前往（不安全）' }}
        </button>
      </div>

      <button class="btn-link" @click="viewCert">查看证书详情 ▾</button>

      <div v-if="advanced" class="warn-detail">
        <div class="d-line"><span class="k">主机</span><span class="v mono">{{ info.host }}:{{ info.port }}</span></div>
        <div class="d-line"><span class="k">主题</span><span class="v mono">{{ info.subject || '—' }}</span></div>
        <div class="d-line"><span class="k">签发者</span><span class="v mono">{{ info.issuer || '—' }}</span></div>
        <div class="d-line"><span class="k">到期</span><span class="v mono">{{ info.notAfter || '—' }}</span></div>
        <div class="d-line"><span class="k">指纹</span><span class="v mono">{{ info.fingerprint || '—' }}</span></div>
        <p class="warn-hint">若这是您自己的内网/自签服务，可"继续前往"；放行记录会存入证书中心，可随时撤销。</p>
      </div>
    </div>
  </div>
</template>

<style scoped>
.warn-mask {
  position: fixed;
  inset: 0;
  z-index: var(--z-security);
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(10, 14, 24, 0.92);
  backdrop-filter: blur(6px);
}
.warn-box {
  /* 安全类特例：620px 宽 + 大内边距，显眼压住一切，不跟普通弹窗统一 */
  width: 620px;
  max-width: 92vw;
  max-height: 84vh;
  overflow-y: auto;
  background: rgba(38, 22, 26, 0.98);
  border: 1px solid rgba(255, 90, 90, 0.4);
  border-radius: var(--radius-xl);
  padding: 28px 32px;
  box-shadow: 0 24px 70px rgba(0, 0, 0, 0.5);
}
.warn-head {
  display: flex;
  align-items: center;
  gap: 12px;
}
.warn-ico {
  font-size: 30px;
}
.warn-head h2 {
  /* 安全类特例：20px 大标题，警告一眼可见，不跟 14px 普通标题统一 */
  margin: 0;
  font-size: var(--text-xl);
  color: #ff8f8f;
}
.warn-lead {
  margin: 16px 0;
  font-size: var(--text-base);
  color: var(--text-secondary);
  line-height: var(--leading-relaxed);
}
.warn-reasons {
  padding: var(--space-md) var(--space-lg);
  border-radius: var(--radius-md);
  /* 深红底专用浅粉文字，对比度高于 danger 原色 */
  background: var(--danger-soft);
  border: 1px solid rgba(255, 90, 90, 0.28);
  color: #ffb3b3;
  font-size: var(--text-base);
  line-height: var(--leading-relaxed);
}
.warn-verdicts {
  margin-top: 10px;
  padding: var(--space-md) var(--space-lg);
  border-radius: var(--radius-md);
  background: var(--warning-soft);
  border: 1px solid rgba(255, 177, 78, 0.3);
  color: var(--warning);
  font-size: var(--text-sm);
  line-height: var(--leading-relaxed);
}
.warn-consequence {
  margin-top: 6px;
  color: var(--danger);
}
.warn-actions {
  display: flex;
  gap: var(--space-md);
  margin-top: 22px;
}
.btn-primary,
.btn-danger {
  flex: 1;
  padding: var(--space-md);
  border-radius: var(--radius-md);
  font-size: var(--text-md);
  cursor: pointer;
  border: none;
}
/* 以下按钮/链接色为安全深红底专用，深浅主题下底色不变，文字保持浅色可读 */
.btn-primary {
  background: rgba(255, 255, 255, 0.12);
  color: #fff;
  border: 1px solid rgba(255, 255, 255, 0.2);
}
.btn-primary:hover {
  background: rgba(255, 255, 255, 0.2);
}
.btn-danger {
  background: rgba(200, 60, 60, 0.35);
  color: #ffd5d5;
  border: 1px solid rgba(255, 90, 90, 0.5);
}
.btn-danger:hover {
  background: rgba(200, 60, 60, 0.5);
}
.btn-danger:disabled {
  opacity: 0.6;
  cursor: default;
}
.btn-link {
  margin-top: var(--space-lg);
  background: none;
  border: none;
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  padding: 0;
}
.btn-link:hover {
  /* hover 显 accent（深浅底均可见）；--accent-contrast 深色主题为深、浅色主题为白，透明底上两主题都不可见 */
  color: var(--accent);
}
.warn-detail {
  margin-top: var(--space-md);
  padding: var(--space-md) 14px;
  border-radius: var(--radius-md);
  background: rgba(255, 255, 255, 0.04);
}
.d-line {
  display: flex;
  gap: 10px;
  font-size: var(--text-sm);
  padding: 4px 0;
}
.d-line .k {
  width: 56px;
  flex-shrink: 0;
  color: var(--text-muted);
}
.d-line .v {
  color: var(--text-secondary);
  word-break: break-all;
}
.mono {
  font-family: var(--font-mono);
}
.warn-hint {
  margin: 10px 0 0;
  font-size: var(--text-sm);
  color: var(--text-muted);
  line-height: var(--leading-relaxed);
}
</style>

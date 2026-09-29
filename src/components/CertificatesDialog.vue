<script setup lang="ts">
// 证书中心：当前网页证书（P1）+ 监测列表/例外/事件（P3）
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗必须隐藏 webview，否则被页面盖住。
import { computed, ref, watch } from 'vue'
import { invokeOk, errText } from '../utils/invoke'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
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

const tabsStore = useTabsStore()
const toastStore = useToastStore()

interface CertInfo {
  host: string
  port: number
  fingerprint_sha256: string
  subject: string
  issuer: string
  not_before: string | null
  not_after: string | null
  dns_names: string[]
  ip_addresses: string[]
  errors: number
  error_flags: string[]
  chain: string[]
  valid: boolean
  is_exception: boolean
}
interface CertRecord {
  host: string
  port: number
  subject: string | null
  issuer: string | null
  not_before: string | null
  not_after: string | null
  fingerprint_sha256: string
  change_count: number
  last_seen: string
  days_left: number | null
}
interface CertException {
  host: string
  port: number
  fingerprint_sha256: string
  added_at: string
  reason: string | null
}
interface CertEvent {
  id: number
  host: string
  port: number
  kind: string
  severity: string
  detail: string | null
  created_at: string
}

const tab = ref<'current' | 'monitor' | 'exceptions' | 'events'>('current')
const current = ref<CertInfo | null>(null)
const records = ref<CertRecord[]>([])
const exceptions = ref<CertException[]>([])
const events = ref<CertEvent[]>([])
const loading = ref(false)

const KIND_ZH: Record<string, string> = {
  first_seen: '首次记录',
  cert_changed: '证书变更',
  verify_error: '校验失败',
  expiry_warn: '临期告警',
  expired: '已过期',
  accepted: '用户放行',
  exception_removed: '取消例外',
}

async function loadCurrent() {
  const id = tabsStore.activeTabId
  if (!id) {
    current.value = null
    return
  }
  try {
    current.value = await invokeOk<CertInfo | null>('get_tls_info', { id })
  } catch (e) {
    current.value = null
    console.warn('[cert] 读取当前证书失败:', e)
  }
}

async function loadAll() {
  loading.value = true
  try {
    const [r, e, v] = await Promise.all([
      invokeOk<CertRecord[]>('list_certificates'),
      invokeOk<CertException[]>('list_cert_exceptions'),
      invokeOk<CertEvent[]>('list_cert_events', { limit: 200 }),
    ])
    records.value = r
    exceptions.value = e
    events.value = v
  } catch (err) {
    console.error('[cert] 加载失败:', err)
  } finally {
    loading.value = false
  }
}

async function scanExpiry() {
  try {
    const list = await invokeOk<CertRecord[]>('scan_cert_expiry', { days: 30 })
    toastStore.show(`扫描完成：${list.length} 个证书临期/过期`, 'info')
    await loadAll()
  } catch (e) {
    toastStore.show('扫描失败：' + errText(e), 'error')
  }
}

async function removeException(x: CertException) {
  try {
    await invokeOk('delete_cert_exception', {
      host: x.host,
      port: x.port,
      fingerprint: x.fingerprint_sha256,
    })
    await loadAll()
    toastStore.show('已取消证书例外', 'info')
  } catch (e) {
    toastStore.show('取消失败：' + errText(e), 'error')
  }
}

async function removeRecord(r: CertRecord) {
  try {
    await invokeOk('delete_certificate', { host: r.host, port: r.port })
    await loadAll()
  } catch (e) {
    toastStore.show('删除失败：' + errText(e), 'error')
  }
}

function severityClass(s: string) {
  return s === 'danger' ? 'sev-danger' : s === 'warn' ? 'sev-warn' : 'sev-info'
}

// 弹窗显示时隐藏所有标签 webview，关闭时恢复（参与全局计数）
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      await setWebviewVisible(!v, 'certs-dialog')
    } catch (e) {
      console.warn('[cert] webview 显隐失败:', e)
    }
    if (my !== seq) return
    if (v) {
      tab.value = 'current'
      await Promise.all([loadCurrent(), loadAll()])
    }
  },
)
</script>

<template>
  <!-- 证书中心：外层 UiDialog（720px 宽）；页签+内容原样搬进默认槽 -->
  <UiDialog v-model:open="showModel" title="🔐 证书中心" width="min(720px, calc(100vw - 32px))">

      <div class="cert-tabs">
        <button :class="{ on: tab === 'current' }" @click="tab = 'current'">当前网页</button>
        <button :class="{ on: tab === 'monitor' }" @click="tab = 'monitor'">
          已监测 ({{ records.length }})
        </button>
        <button :class="{ on: tab === 'exceptions' }" @click="tab = 'exceptions'">
          例外 ({{ exceptions.length }})
        </button>
        <button :class="{ on: tab === 'events' }" @click="tab = 'events'">事件 ({{ events.length }})</button>
      </div>

      <div class="cert-body">
        <!-- 当前网页 -->
        <template v-if="tab === 'current'">
          <div v-if="!current" class="cert-empty">当前标签没有 TLS 证书信息（非 HTTPS 页或尚未加载）</div>
          <div v-else class="cert-info">
            <div class="cert-line">
              <span class="k">连接</span>
              <span class="v">
                <b :class="current.valid ? 'ok' : 'bad'">{{ current.valid ? '🔒 安全' : '⚠️ 不安全' }}</b>
                {{ current.host }}:{{ current.port }}
                <span v-if="current.is_exception" class="tag">已放行</span>
              </span>
            </div>
            <div v-if="current.error_flags.length" class="cert-errs">
              <div v-for="(f, i) in current.error_flags" :key="i">⚠️ {{ f }}</div>
            </div>
            <div class="cert-line"><span class="k">主题</span><span class="v">{{ current.subject || '—' }}</span></div>
            <div class="cert-line"><span class="k">签发者</span><span class="v">{{ current.issuer || '—' }}</span></div>
            <div class="cert-line">
              <span class="k">有效期</span>
              <span class="v">{{ current.not_before || '—' }} → {{ current.not_after || '—' }}</span>
            </div>
            <div class="cert-line">
              <span class="k">指纹</span>
              <span class="v mono">{{ current.fingerprint_sha256 || '—' }}</span>
            </div>
            <div v-if="current.dns_names.length" class="cert-line">
              <span class="k">域名</span>
              <span class="v mono">{{ current.dns_names.join(', ') }}</span>
            </div>
            <div v-if="current.ip_addresses.length" class="cert-line">
              <span class="k">IP</span>
              <span class="v mono">{{ current.ip_addresses.join(', ') }}</span>
            </div>
            <div v-if="current.chain.length" class="cert-line">
              <span class="k">证书链</span>
              <span class="v mono">{{ current.chain.join(' → ') }}</span>
            </div>
          </div>
        </template>

        <!-- 已监测 -->
        <template v-else-if="tab === 'monitor'">
          <div class="cert-bar">
            <button class="btn-mini" @click="scanExpiry">扫描临期</button>
            <button class="btn-mini" @click="loadAll">刷新</button>
          </div>
          <div v-if="loading" class="cert-empty">加载中…</div>
          <div v-else-if="!records.length" class="cert-empty">还没有监测记录，浏览 HTTPS 网站后自动记录</div>
          <table v-else class="cert-table">
            <thead>
              <tr><th>主机</th><th>签发者</th><th>到期</th><th>剩余</th><th>变更</th><th></th></tr>
            </thead>
            <tbody>
              <tr v-for="r in records" :key="r.host + ':' + r.port">
                <td class="mono">{{ r.host }}:{{ r.port }}</td>
                <td class="ellip">{{ r.issuer || '—' }}</td>
                <td class="mono">{{ (r.not_after || '—').slice(0, 10) }}</td>
                <td>
                  <span
                    v-if="r.days_left !== null"
                    :class="r.days_left < 0 ? 'bad' : r.days_left <= 7 ? 'warn' : 'ok'"
                  >
                    {{ r.days_left < 0 ? '已过期' : r.days_left + ' 天' }}
                  </span>
                  <span v-else>—</span>
                </td>
                <td>{{ r.change_count }}</td>
                <td><button class="btn-x" @click="removeRecord(r)">✕</button></td>
              </tr>
            </tbody>
          </table>
        </template>

        <!-- 例外 -->
        <template v-else-if="tab === 'exceptions'">
          <div v-if="!exceptions.length" class="cert-empty">没有放行的证书例外</div>
          <table v-else class="cert-table">
            <thead>
              <tr><th>主机</th><th>指纹</th><th>加入时间</th><th>理由</th><th></th></tr>
            </thead>
            <tbody>
              <tr v-for="x in exceptions" :key="x.host + x.fingerprint_sha256">
                <td class="mono">{{ x.host }}:{{ x.port }}</td>
                <td class="mono ellip">{{ x.fingerprint_sha256.slice(0, 16) }}…</td>
                <td class="mono">{{ x.added_at }}</td>
                <td class="ellip">{{ x.reason || '—' }}</td>
                <td><button class="btn-x" @click="removeException(x)">撤销</button></td>
              </tr>
            </tbody>
          </table>
        </template>

        <!-- 事件 -->
        <template v-else>
          <div v-if="!events.length" class="cert-empty">暂无证书事件</div>
          <div v-else class="cert-events">
            <div v-for="ev in events" :key="ev.id" class="ev-row" :class="severityClass(ev.severity)">
              <span class="ev-kind">{{ KIND_ZH[ev.kind] || ev.kind }}</span>
              <span class="ev-host mono">{{ ev.host }}</span>
              <span class="ev-detail">{{ ev.detail }}</span>
              <span class="ev-time">{{ ev.created_at }}</span>
            </div>
          </div>
        </template>
      </div>
  </UiDialog>
</template>

<style scoped>
/* 弹窗基础样式走 UiDialog（Reka 底座），这里只保留证书业务样式 */
.cert-tabs {
  display: flex;
  gap: 6px;
  padding: 12px 18px 0;
}
.cert-tabs button {
  padding: 6px 14px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-full);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease),
    border-color var(--dur-fast) var(--ease);
}
.cert-tabs button:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.cert-tabs button.on {
  background: var(--accent-soft);
  border-color: transparent;
  color: var(--accent);
  font-weight: 600;
}
.cert-body {
  padding: 14px 18px 18px;
  overflow-y: auto;
  flex: 1;
}
.cert-empty {
  color: var(--text-muted);
  font-size: var(--text-base);
  padding: 28px 0;
  text-align: center;
}
.cert-line {
  display: flex;
  gap: 12px;
  padding: 7px 0;
  font-size: var(--text-base);
  border-bottom: 1px solid var(--hairline);
}
.cert-line .k {
  width: 72px;
  flex-shrink: 0;
  color: var(--text-muted);
}
.cert-line .v {
  color: var(--text-primary);
  word-break: break-all;
}
.mono {
  font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  font-size: var(--text-sm);
}
.ok {
  color: var(--success);
}
.bad {
  color: var(--danger);
}
.warn {
  color: var(--warning);
}
.tag {
  margin-left: 8px;
  padding: 1px 9px;
  border-radius: var(--radius-full);
  background: var(--warning-soft);
  color: var(--warning);
  font-size: var(--text-xs);
}
.cert-errs {
  margin: 8px 0;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  background: var(--danger-soft);
  color: var(--danger);
  font-size: var(--text-sm);
}
.cert-bar {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.btn-mini {
  padding: 6px 14px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-full);
  color: var(--text-secondary);
  font-size: var(--text-sm);
  cursor: pointer;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.btn-mini:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.btn-x {
  background: var(--danger-soft);
  border: 1px solid transparent;
  color: var(--danger);
  border-radius: var(--radius-full);
  font-size: var(--text-xs);
  padding: 3px 10px;
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
.btn-x:hover {
  background: var(--danger);
  color: #fff;
}
.cert-table {
  width: 100%;
  border-collapse: collapse;
  font-size: var(--text-sm);
}
.cert-table th {
  text-align: left;
  color: var(--text-muted);
  font-weight: 500;
  padding: 7px 8px;
  border-bottom: 1px solid var(--hairline);
}
.cert-table td {
  padding: 7px 8px;
  color: var(--text-secondary);
  border-bottom: 1px solid var(--hairline);
}
.ellip {
  max-width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.cert-events {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.ev-row {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 8px 11px;
  border-radius: var(--radius-sm);
  background: var(--bg-secondary);
  font-size: var(--text-sm);
}
.ev-row.sev-danger {
  background: var(--danger-soft);
}
.ev-row.sev-warn {
  background: var(--warning-soft);
}
.ev-kind {
  flex-shrink: 0;
  color: var(--text-primary);
  font-weight: 500;
  width: 72px;
}
.ev-host {
  width: 180px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}
.ev-detail {
  flex: 1;
  color: var(--text-secondary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.ev-time {
  flex-shrink: 0;
  color: var(--text-muted);
  font-variant-numeric: tabular-nums;
}
</style>

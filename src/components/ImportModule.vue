<script setup lang="ts">
// 从浏览器导入（F-94）：Chrome / Brave 书签 + 密码导入面板
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，弹窗必须隐藏 webview，否则被页面盖住。
import { computed, ref, watch } from 'vue'
import { invokeOk, errText } from '../utils/invoke'
import { setWebviewVisible } from '../utils/webview'
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

const toastStore = useToastStore()

/** F-89 扫描回执（全是计数/标记，无明文字段） */
interface SourceReport {
  browser: 'chrome' | 'brave'
  profile: string
  kind: 'bookmarks' | 'passwords'
  file: string
  count: number | null
  encrypted: boolean
  decryptable: boolean | null
}
interface ImportBookmarksResult {
  imported: number
  merged_away: number
  rejected_unsafe: number
  skipped_sources: string[]
  used_sources: string[]
}
interface ImportPasswordsResult {
  imported: number
  skipped_empty: number
  failed: number
  skipped_sources: string[]
  used_sources: string[]
  degraded_reason: string | null
}

const tab = ref<'bookmarks' | 'passwords'>('bookmarks')
const sources = ref<SourceReport[]>([])
const loading = ref(false)

const bmResult = ref<ImportBookmarksResult | null>(null)
const pwResult = ref<ImportPasswordsResult | null>(null)
const importing = ref(false)

/** 打开时：隐藏 webview + 扫描源 */
watch(
  () => props.show,
  async (show) => {
    if (!show) {
      sources.value = []
      bmResult.value = null
      pwResult.value = null
      return
    }
    await setWebviewVisible(false, 'import-dialog')
    await loadSources()
  },
)

async function loadSources() {
  loading.value = true
  try {
    sources.value = await invokeOk<SourceReport[]>('scan_browser_import')
  } catch (e) {
    toastStore.error(`扫描失败：${errText(e)}`)
    sources.value = []
  } finally {
    loading.value = false
  }
}

/** 某浏览器的书签源摘要（有记录 profile 数与可读条数） */
function srcSummary(browser: 'chrome' | 'brave', kind: 'bookmarks' | 'passwords'): string {
  const rows = sources.value.filter((s) => s.browser === browser && s.kind === kind)
  if (!rows.length) return '未发现'
  const total = rows.reduce((n, r) => n + (r.count ?? 0), 0)
  return `${total} 条`
}

async function doImportBookmarks() {
  importing.value = true
  try {
    bmResult.value = await invokeOk<ImportBookmarksResult>('import_bookmarks_from_browser')
    toastStore.show(`书签导入完成：写入 ${bmResult.value.imported} 条`, 'success')
  } catch (e) {
    toastStore.error(`书签导入失败：${errText(e)}`)
  } finally {
    importing.value = false
  }
}

async function doImportPasswords() {
  importing.value = true
  try {
    pwResult.value = await invokeOk<ImportPasswordsResult>('import_passwords_from_browser')
    toastStore.show(`密码导入完成：写入 ${pwResult.value.imported} 条`, 'success')
  } catch (e) {
    toastStore.error(`密码导入失败：${errText(e)}`)
  } finally {
    importing.value = false
  }
}
</script>

<template>
  <UiDialog v-model:open="showModel" title="从浏览器导入" width="min(560px, calc(100vw - 48px))">
    <div class="import-body" data-testid="import-module">
      <p class="import-hint">从本机 Chrome / Brave 导入书签与密码。源文件只读，不会被修改。</p>

      <div class="import-tabs" role="tablist">
        <button
          class="import-tab"
          :class="{ active: tab === 'bookmarks' }"
          role="tab"
          @click="tab = 'bookmarks'"
        >
          书签
        </button>
        <button
          class="import-tab"
          :class="{ active: tab === 'passwords' }"
          role="tab"
          @click="tab = 'passwords'"
        >
          密码
        </button>
      </div>

      <div v-if="loading" class="import-loading">正在扫描本机浏览器…</div>

      <template v-else>
        <!-- 书签块 -->
        <div v-if="tab === 'bookmarks'" class="import-section">
          <div class="src-summary">
            <span>Chrome：{{ srcSummary('chrome', 'bookmarks') }}</span>
            <span>Brave：{{ srcSummary('brave', 'bookmarks') }}</span>
          </div>
          <button class="import-run" :disabled="importing" @click="doImportBookmarks">
            {{ importing ? '导入中…' : '导入书签' }}
          </button>
          <div v-if="bmResult" class="import-result" data-testid="bm-result">
            <div>写入 {{ bmResult.imported }} 条</div>
            <div v-if="bmResult.merged_away">合并去重 {{ bmResult.merged_away }} 条</div>
            <div v-if="bmResult.rejected_unsafe">拒收不安全链接 {{ bmResult.rejected_unsafe }} 条</div>
            <div v-if="bmResult.skipped_sources.length" class="warn">
              跳过源：{{ bmResult.skipped_sources.join('、') }}
            </div>
          </div>
        </div>

        <!-- 密码块 -->
        <div v-else class="import-section">
          <div class="src-summary">
            <span>Chrome：{{ srcSummary('chrome', 'passwords') }}</span>
            <span>Brave：{{ srcSummary('brave', 'passwords') }}</span>
          </div>
          <p class="import-warn">密码经系统钥匙串解密后写入本机密码库，明文不落盘。</p>
          <button class="import-run" :disabled="importing" @click="doImportPasswords">
            {{ importing ? '导入中…' : '导入密码' }}
          </button>
          <div v-if="pwResult" class="import-result" data-testid="pw-result">
            <div>写入 {{ pwResult.imported }} 条</div>
            <div v-if="pwResult.skipped_empty">空密码跳过 {{ pwResult.skipped_empty }} 条</div>
            <div v-if="pwResult.failed">失败 {{ pwResult.failed }} 条</div>
            <div v-if="pwResult.degraded_reason" class="warn">{{ pwResult.degraded_reason }}</div>
          </div>
        </div>
      </template>
    </div>
  </UiDialog>
</template>

<style scoped>
.import-body { display: flex; flex-direction: column; gap: 12px; }
.import-hint { margin: 0; color: var(--text-dim, #888); font-size: 13px; }
.import-tabs { display: flex; gap: 8px; }
.import-tab {
  padding: 6px 16px; border-radius: 8px; border: 1px solid var(--border, #333);
  background: transparent; color: var(--text, #ddd); cursor: pointer;
}
.import-tab.active { background: var(--accent, #4a7dff); color: #fff; border-color: transparent; }
.import-loading { padding: 24px 0; text-align: center; color: var(--text-dim, #888); }
.import-section { display: flex; flex-direction: column; gap: 10px; }
.src-summary { display: flex; gap: 16px; font-size: 13px; color: var(--text-dim, #888); }
.import-warn { margin: 0; font-size: 12px; color: var(--warn, #c8a04a); }
.import-run {
  align-self: flex-start; padding: 8px 20px; border-radius: 8px; border: none;
  background: var(--accent, #4a7dff); color: #fff; cursor: pointer;
}
.import-run:disabled { opacity: 0.5; cursor: default; }
.import-result {
  padding: 10px 12px; border-radius: 8px; background: var(--bg-soft, #1c1c1e);
  font-size: 13px; display: flex; flex-direction: column; gap: 4px;
}
.import-result .warn { color: var(--warn, #c8a04a); }
</style>

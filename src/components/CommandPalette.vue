<script setup lang="ts">
// 命令栏（Ctrl+K 万能入口）——阶段 3.1 交互形态 + shadcn 试点 2（Reka Combobox 底座）
// 一个输入框统一入口：执行浏览器命令 / 切换标签 / 搜索并打开历史 / 直接打开网址 / 问 AI。
// 注意：child webview 是 GTK 部件盖在 Vue UI 上，打开时必须隐藏所有标签 webview，否则被页面盖住。
//
// Reka 接管的部分：上下键高亮、回车选中、悬停高亮、高亮项自动滚入视野、无障碍 listbox 语义。
// 自己保留的部分：多源候选计算（query 驱动，ignoreFilter）、选中派发 execute()、
//   webview 显隐、输入框聚焦、中文输入法组字保护（isComposing 时按键不下沉到 Reka）。
import { computed, defineComponent, nextTick, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import {
  ComboboxContent,
  ComboboxEmpty,
  ComboboxInput,
  ComboboxItem,
  ComboboxRoot,
  ComboboxViewport,
  FocusScope,
  injectComboboxRootContext,
} from 'reka-ui'
import { setWebviewVisible } from '../utils/webview'
import { useTabsStore } from '../stores/tabs'
import type { HistoryEntry } from '../types/history'

const props = defineProps<{
  show: boolean
  /** 初始候选模式（tab-search 快捷键直达标签模式；缺省 all） */
  initialMode?: 'all' | 'tab'
}>()
const { t } = useI18n()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'command', action: string): void
  (e: 'ask-ai', query: string): void
}>()

const tabsStore = useTabsStore()

type Kind = 'command' | 'tab' | 'history' | 'url' | 'ai'
interface Item {
  key: string
  kind: Kind
  icon: string
  title: string
  subtitle?: string
  /** command=动作名；tab=标签 id；history/url=网址；ai=提问文本 */
  value: string
}

const query = ref('')
/** 候选范围子模式（C-3 标签搜索）：all=全部混合 / tab=只看标签 */
const mode = ref<'all' | 'tab'>('all')
const MODES: { id: 'all' | 'tab'; label: string }[] = [
  { id: 'all', label: '全部' },
  { id: 'tab', label: '标签' },
]
const historyEntries = ref<HistoryEntry[]>([])
const inputRef = ref<InstanceType<typeof ComboboxInput> | null>(null)

/** 内置命令（kind=command，value 交给 App 派发） */
const COMMANDS: Item[] = [
  { key: 'cmd:new-tab', kind: 'command', icon: '＋', title: '新建标签页', value: 'new-tab' },
  { key: 'cmd:close-tab', kind: 'command', icon: '✕', title: '关闭当前标签页', value: 'close-tab' },
  { key: 'cmd:reload', kind: 'command', icon: '↻', title: '刷新当前页面', value: 'reload' },
  { key: 'cmd:focus-address', kind: 'command', icon: '⌖', title: '聚焦地址栏', value: 'focus-address' },
  { key: 'cmd:ai', kind: 'command', icon: '✦', title: '打开 AI 助手', value: 'ai' },
  { key: 'cmd:history', kind: 'command', icon: '🕘', title: '打开历史记录', value: 'history' },
  { key: 'cmd:bookmarks', kind: 'command', icon: '🔖', title: '打开书签管理', value: 'bookmarks' },
  { key: 'cmd:downloads', kind: 'command', icon: '⤓', title: '打开下载管理', value: 'downloads' },
  { key: 'cmd:passwords', kind: 'command', icon: '🔑', title: '打开密码管理', value: 'passwords' },
  { key: 'cmd:sessions', kind: 'command', icon: '🗂', title: '打开会话管理', value: 'sessions' },
  { key: 'cmd:userscripts', kind: 'command', icon: '📜', title: '打开用户脚本', value: 'userscripts' },
  { key: 'cmd:translate', kind: 'command', icon: '🌐', title: '翻译当前页面', value: 'translate' },
  { key: 'cmd:settings', kind: 'command', icon: '⚙', title: '打开设置', value: 'settings' },
  { key: 'cmd:close-same-domain', kind: 'command', icon: '🧹', title: '关闭同域名标签', value: 'close-same-domain' },
  { key: 'cmd:close-duplicates', kind: 'command', icon: '🧹', title: '关闭重复标签', value: 'close-duplicates' },
  { key: 'cmd:mute-all', kind: 'command', icon: '🔇', title: '全部静音/取消', value: 'mute-all' },
  { key: 'cmd:devtools', kind: 'command', icon: '🛠', title: '开发者工具', value: 'devtools' },
]

/** 输入是否像网址（含协议，或像域名） */
function isUrlLike(s: string): boolean {
  if (/^[a-z][a-z0-9+.-]*:\/\//i.test(s)) return true
  return !/\s/.test(s) && /^[^\s/]+\.[^\s/]{2,}/.test(s)
}
function normalizeUrl(s: string): string {
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(s) ? s : 'https://' + s
}

/** 标签候选（切换）：匹配标题/URL；全部模式与标签子模式共用（C-3） */
function tabItems(q: string): Item[] {
  const out: Item[] = []
  for (const t of tabsStore.tabs) {
    const title = t.title || t.url || '新标签页'
    if (!q || title.toLowerCase().includes(q) || t.url.toLowerCase().includes(q)) {
      out.push({ key: 'tab:' + t.id, kind: 'tab', icon: '🗔', title, subtitle: t.url, value: t.id })
    }
  }
  return out
}

const items = computed<Item[]>(() => {
  const raw = query.value.trim()
  const q = raw.toLowerCase()
  const out: Item[] = []

  // 标签子模式（C-3）：只出标签候选
  if (mode.value === 'tab') {
    return tabItems(q).slice(0, 50)
  }

  // ① 问 AI（有输入即置顶候选）
  if (raw) {
    out.push({
      key: 'ai:' + raw,
      kind: 'ai',
      icon: '✦',
      title: `问 AI：${raw}`,
      value: raw,
    })
  }

  // ② 直接打开网址（输入像 URL/域名）
  if (raw && isUrlLike(raw)) {
    out.push({
      key: 'url:' + raw,
      kind: 'url',
      icon: '↗',
      title: `打开 ${raw}`,
      value: normalizeUrl(raw),
    })
  }

  // ③ 标签页（切换）
  out.push(...tabItems(q))

  // ④ 历史（打开；无输入时只给最近 6 条，避免刷屏）
  const hist = historyEntries.value.filter((h) => {
    if (!q) return true
    const title = (h.title || h.url).toLowerCase()
    return title.includes(q) || h.url.toLowerCase().includes(q)
  })
  for (const h of hist.slice(0, q ? 20 : 6)) {
    out.push({
      key: 'his:' + h.id,
      kind: 'history',
      icon: '🕘',
      title: h.title || h.url,
      subtitle: h.url,
      value: h.url,
    })
  }

  // ⑤ 内置命令
  for (const c of COMMANDS) {
    if (!q || c.title.toLowerCase().includes(q)) out.push(c)
  }

  return out.slice(0, 50)
})

function execute(it: Item) {
  if (it.kind === 'command') emit('command', it.value)
  else if (it.kind === 'tab') tabsStore.switchTab(it.value)
  else if (it.kind === 'history' || it.kind === 'url') tabsStore.createTab(it.value)
  else if (it.kind === 'ai') emit('ask-ai', it.value)
  emit('close')
}

/** 中文输入法组字中：所有按键都不下沉到 Reka，交给候选框（捕获阶段拦） */
function guardComposing(e: KeyboardEvent) {
  if (e.isComposing) e.stopPropagation()
}

/** 空结果回车 = 关面板（与旧版 execute() 无候选即关一致；有候选时 Reka 选中走 @select） */
function onEnterKey() {
  if (items.value.length === 0) emit('close')
}

async function loadHistory() {
  try {
    historyEntries.value = await invoke<HistoryEntry[]>('get_recent_history', { limit: 40 })
  } catch {
    historyEntries.value = []
  }
}

// 修 critical（aria-controls 空串）：
// Reka 2.10.5 ComboboxRoot 的 context 是裸对象（createContext 无 reactive 包装），
// contentId 初始为 ''；ComboboxContent 的 contentId ||= useId() 只在 Portal 分支
// 赋值（非 Portal 内联场景不生效），ComboboxInput 写死 :aria-controls="rootContext.contentId"
// → 永远渲染空串（axe critical）。
//
// 修法：在 ComboboxRoot 的槽内用包装组件 CpComboboxIdFix，通过 Reka 主包公开的
// injectComboboxRootContext 拿到同一 context 对象，源头改写 contentId 为静态值。
// 因 context 是裸对象（赋值不触发响应式反 patch），Reka 自身渲染的 input aria-controls
// 与 content 元素 id 天然同步为该静态值，无需 DOM 补丁 / MutationObserver。
const COMMAND_LISTBOX_ID = 'cp-command-listbox'

/** 包裹在 ComboboxRoot 槽内：inject 同一 context，源头改写 contentId 为静态值 */
const CpComboboxIdFix = defineComponent({
  name: 'CpComboboxIdFix',
  setup(_, { slots }) {
    const rootContext = injectComboboxRootContext()
    if (rootContext && !rootContext.contentId)
      rootContext.contentId = COMMAND_LISTBOX_ID
    return () => slots.default?.()
  },
})

// 打开：隐藏 webview（否则被页面盖住）+ 载入历史 + 聚焦输入框；关闭：恢复 webview
watch(
  () => props.show,
  async (v) => {
    if (v) {
      query.value = ''
      mode.value = props.initialMode ?? 'all'
      historyEntries.value = []
      await setWebviewVisible(false, 'command-palette')
      loadHistory()
      nextTick(() => {
        ;(inputRef.value?.$el as HTMLElement | undefined)?.focus?.()
      })
    } else {
      await setWebviewVisible(true, 'command-palette')
    }
  },
)
</script>

<template>
  <!-- 焦点陷阱：FocusScope(trapped+loop) 圈定 overlay 内 Tab；Reka 在 unmount 时自动把焦点还给打开前的元素 -->
  <FocusScope v-if="show" class="cp-overlay" :trapped="true" :loop="true" as="div" @mousedown.self="emit('close')">
    <!-- 命令菜单官方形态：常开 + 自过滤关闭 + 选中默认行为拦截走 execute() -->
    <ComboboxRoot
      :open="true"
      :ignore-filter="true"
      model-value=""
      class="cp-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="t('commandPalette.title')"
    >
      <!-- 源头修：inject ComboboxRoot context 并改写 contentId 为静态值，
           Reka 自身渲染的 input aria-controls 与 content id 天然同步，无 DOM 补丁 -->
      <CpComboboxIdFix>
        <div class="cp-input-row" @keydown.capture="guardComposing">
          <span class="cp-search">⌕</span>
          <ComboboxInput
            ref="inputRef"
            v-model="query"
            class="cp-input"
            :placeholder="t('commandPalette.placeholder')"
            :aria-label="t('commandPalette.searchLabel')"
            spellcheck="false"
            @keydown.enter="onEnterKey"
          />
          <span class="cp-esc">Esc</span>
        </div>
        <!-- 候选范围子模式（C-3 标签搜索）：全部 / 标签 -->
        <div class="cp-mode-row" role="radiogroup" aria-label="候选范围">
          <button
            v-for="m in MODES"
            :key="m.id"
            class="cp-mode-tab"
            :class="{ active: mode === m.id }"
            role="radio"
            :aria-checked="mode === m.id"
            @click="mode = m.id"
          >
            {{ m.label }}
          </button>
        </div>
        <ComboboxContent class="cp-list-wrap" :aria-label="t('commandPalette.listLabel')" @escape-key-down="emit('close')">
          <ComboboxViewport class="cp-list">
            <ComboboxEmpty class="cp-empty">{{ t('commandPalette.empty') }}</ComboboxEmpty>
            <ComboboxItem
              v-for="it in items"
              :key="it.key"
              :value="it.key"
              class="cp-item"
              @select.prevent="execute(it)"
            >
              <span class="cp-icon">{{ it.icon }}</span>
              <span class="cp-text">
                <span class="cp-title">{{ it.title }}</span>
                <span v-if="it.subtitle" class="cp-sub">{{ it.subtitle }}</span>
              </span>
            </ComboboxItem>
          </ComboboxViewport>
        </ComboboxContent>
      </CpComboboxIdFix>
    </ComboboxRoot>
  </FocusScope>
</template>

<style scoped>
.cp-overlay {
  position: fixed;
  inset: 0;
  z-index: var(--z-overlay);
  background: var(--scrim);
  backdrop-filter: blur(6px);
  -webkit-backdrop-filter: blur(6px);
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding-top: 12vh;
}
.cp-panel {
  width: min(var(--dialog-lg), 92vw);
  max-height: 60vh;
  display: flex;
  flex-direction: column;
  background: var(--bg-elevated);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-lg);
  box-shadow: var(--shadow-dialog);
  overflow: hidden;
  animation: cp-pop var(--dur) var(--ease-out);
}
.cp-panel:focus-within {
  border-color: var(--accent);
}
@keyframes cp-pop {
  from {
    opacity: 0;
    transform: translateY(-8px) scale(0.985);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
.cp-input-row {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 13px 16px;
  border-bottom: 1px solid var(--hairline);
}
.cp-search {
  font-size: 16px;
  color: var(--accent);
}
.cp-input {
  flex: 1;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text-primary);
  font-size: var(--text-md);
}
.cp-input::placeholder {
  color: var(--text-muted);
}
.cp-esc {
  font-size: var(--text-xs);
  color: var(--text-muted);
  background: var(--bg-elevated-2);
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-xs);
  padding: 2px 7px;
  line-height: var(--leading-tight);
}
.cp-mode-row {
  display: flex;
  gap: 6px;
  padding: 0 16px 8px;
}
.cp-mode-tab {
  height: 24px;
  padding: 0 12px;
  border: 1px solid var(--border-subtle);
  border-radius: var(--radius-full);
  background: transparent;
  color: var(--text-secondary);
  font-size: var(--text-xs);
  cursor: pointer;
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
}
.cp-mode-tab:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.cp-mode-tab:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.cp-mode-tab.active {
  background: var(--accent-soft);
  border-color: var(--accent);
  color: var(--accent);
}
/* Reka Content 内联渲染：穿透布局，视觉与旧版一致 */
.cp-list-wrap {
  display: contents;
}
.cp-list {
  overflow-y: auto;
  padding: 6px;
}
.cp-empty {
  padding: 24px;
  margin: 6px;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--text-base);
  border: 1px dashed var(--border-color);
  border-radius: var(--radius-sm);
}
.cp-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 7px 10px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease);
}
/* 高亮态由 Reka data-highlighted 驱动（替代旧 .active 类） */
.cp-item[data-highlighted] {
  background: var(--accent-soft);
  box-shadow: inset 0 0 0 1px var(--accent);
  outline: none;
}
.cp-icon {
  width: 28px;
  height: 28px;
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-size: 14px;
  color: var(--text-secondary);
  background: var(--bg-elevated-2);
  border: 1px solid var(--hairline);
  border-radius: var(--radius-xs);
}
.cp-item[data-highlighted] .cp-icon {
  background: var(--accent);
  border-color: var(--accent);
  color: var(--accent-contrast);
}
.cp-text {
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.cp-title {
  color: var(--text-primary);
  font-size: var(--text-base);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.cp-item[data-highlighted] .cp-title {
  font-weight: 600;
}
.cp-sub {
  color: var(--text-muted);
  font-size: var(--text-xs);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>

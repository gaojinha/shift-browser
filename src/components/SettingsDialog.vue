<script setup lang="ts">
// 设置弹窗 · IA 重组（2026-09）：10 栏平铺 → 8 栏 4 分组
// 基础：常规 / 外观；安全与主权：隐私与安全 / 数据与同步；
// 智能与平台：AI 助手（模型/MCP/试验 子页签）/ 扩展与自动化；系统：高级 / 关于
// 设置项存 settings 表（key-value），保存即生效
import { computed, nextTick, ref, watch } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useToastStore } from '../stores/toast'
import GeneralSection from './settings/GeneralSection.vue'
import SelfCheckSection from './settings/SelfCheckSection.vue'
import AboutSection from './settings/AboutSection.vue'
import AiSection from './settings/AiSection.vue'
import ExtensionsSection from './settings/ExtensionsSection.vue'
import ApiSection from './settings/ApiSection.vue'
import AppearanceSection from './settings/AppearanceSection.vue'
import PrivacySection from './settings/PrivacySection.vue'
import DataSection from './settings/DataSection.vue'
import AutomationCard from './settings/AutomationCard.vue'
import CompatCard from './settings/CompatCard.vue'
import UserscriptsEntryCard from './settings/UserscriptsEntryCard.vue'
import {
  SECTION_GROUPS,
  resolveTarget,
  searchSettings,
  type SearchEntry,
  type SectionId,
  type AiTabId,
} from './settings/settingsNavigation'

const props = defineProps<{ show: boolean; initialSection?: string }>()
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'openCerts'): void
  (e: 'open-userscripts'): void
}>()

const settingsStore = useSettingsStore()

// 当前激活栏目；AI 栏记住子页签（深链 ai#mcp 打开时初始落到 MCP）
const activeSection = ref<SectionId>('general')
const aiTab = ref<AiTabId>('model')

// 右侧内容根：搜索/深链命中卡片锚点后滚动定位
const contentRef = ref<HTMLElement | null>(null)
function scrollToAnchor(anchor?: string) {
  if (!anchor) return
  void nextTick(() => {
    requestAnimationFrame(() => {
      const el = contentRef.value?.querySelector(`[data-anchor="${anchor}"]`) as HTMLElement | null
      el?.scrollIntoView({ behavior: 'smooth', block: 'start' })
    })
  })
}

// 设置搜索（静态注册表，见 settingsNavigation.ts）
const searchQuery = ref('')
const searchResults = computed(() => searchSettings(searchQuery.value))
function sectionLabel(id: SectionId): string {
  return SECTION_GROUPS.flatMap((g) => g.items).find((i) => i.id === id)?.label ?? ''
}
function goEntry(e: SearchEntry) {
  activeSection.value = e.section
  searchQuery.value = ''
  if (e.section === 'ai') {
    // 页签切换由 AiSection watch initialTab 处理
    aiTab.value = e.aiTab ?? 'model'
  } else {
    scrollToAnchor(e.anchor)
  }
}

// 外部指定初始栏目（状态栏隐私战绩 → privacy#shield；旧 id 经映射表全兼容）
watch(
  () => props.show,
  (v) => {
    if (v && props.initialSection) {
      const t = resolveTarget(props.initialSection)
      activeSection.value = t.section
      aiTab.value = t.aiTab
      if (t.section !== 'ai') scrollToAnchor(t.anchor)
    }
    if (!v) searchQuery.value = ''
  },
)

// 竞态防护：seq 令牌丢弃过期回调；加载失败也必须在 finally 里恢复/隐藏 webview
let seq = 0
watch(
  () => props.show,
  async (v) => {
    const my = ++seq
    try {
      // 各分区数据由对应 *Section.vue 的 onMounted 自取（切到该分区时加载）
      if (v && !settingsStore.loaded) {
        settingsStore.loadSettings().catch((e) => {
          console.error('[settings] 加载设置失败:', e)
          useToastStore().error('加载设置失败')
        })
      }
    } finally {
      if (my === seq) {
        await setWebviewVisible(!v, 'settings-dialog')
      }
    }
  },
)
</script>

<template>
  <Transition name="fade">
  <div class="dialog-overlay" v-if="props.show" @click.self="emit('close')">
    <div class="dialog settings-dialog">
      <div class="dialog-header">
        <span class="dialog-title"><AppIcon name="gear" :size="15" /> 设置</span>
        <button class="close-btn" aria-label="关闭" @click="emit('close')">✕</button>
      </div>
      <div class="dialog-body settings-body">
        <!-- 左侧导航 -->
        <div class="settings-nav">
          <div class="settings-search">
            <input v-model="searchQuery" type="text" placeholder="搜索设置…" spellcheck="false" />
          </div>
          <!-- 搜索结果（有查询时替换分组导航） -->
          <div v-if="searchQuery.trim()" class="search-results">
            <button
              v-for="r in searchResults"
              :key="r.section + '#' + (r.anchor ?? '') + '#' + r.label"
              class="search-item"
              @click="goEntry(r)"
            >
              <span class="search-item-label">{{ r.label }}</span>
              <span class="search-item-section">{{ sectionLabel(r.section) }}</span>
            </button>
            <div v-if="searchResults.length === 0" class="search-empty">无匹配设置</div>
          </div>
          <template v-else>
            <template v-for="g in SECTION_GROUPS" :key="g.label">
              <div class="nav-group-label">{{ g.label }}</div>
              <div
                v-for="s in g.items"
                :key="s.id"
                class="nav-item"
                :class="{ active: activeSection === s.id }"
                @click="activeSection = s.id"
              >
                <span class="nav-icon"><AppIcon :name="s.icon" :size="16" /></span>
                <span class="nav-label">{{ s.label }}</span>
              </div>
            </template>
          </template>
        </div>

        <!-- 右侧内容（普通栏统一滚动容器；AI 栏自带子页签与滚动） -->
        <div ref="contentRef" class="settings-content">
          <div v-if="activeSection === 'general'" class="settings-scroll">
            <GeneralSection />
          </div>
          <div v-else-if="activeSection === 'appearance'" class="settings-scroll">
            <AppearanceSection />
          </div>
          <div v-else-if="activeSection === 'privacy'" class="settings-scroll">
            <PrivacySection @open-certs="emit('openCerts')" />
          </div>
          <div v-else-if="activeSection === 'data'" class="settings-scroll">
            <DataSection />
          </div>
          <AiSection v-else-if="activeSection === 'ai'" :initial-tab="aiTab" />
          <div v-else-if="activeSection === 'extensions'" class="settings-scroll">
            <ExtensionsSection />
            <UserscriptsEntryCard @open="emit('open-userscripts')" />
            <AutomationCard />
          </div>
          <div v-else-if="activeSection === 'advanced'" class="settings-scroll">
            <ApiSection />
            <CompatCard />
            <SelfCheckSection />
          </div>
          <div v-else-if="activeSection === 'about'" class="settings-scroll">
            <AboutSection />
          </div>
        </div>
      </div>
    </div>
  </div>
  </Transition>
</template>

<style scoped>
.settings-dialog {
  width: min(860px, calc(100vw - 48px));
  height: min(660px, calc(100vh - 96px));
  display: flex;
  flex-direction: column;
  border-radius: var(--radius-lg);
}
.settings-body {
  display: flex;
  gap: 0;
  padding: 0;
  overflow: hidden;
  flex: 1;
  min-height: 0;
}

/* 左侧导航 */
.settings-nav {
  width: 184px;
  flex-shrink: 0;
  border-right: 1px solid var(--hairline);
  padding: 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  background: var(--bg-elevated-2);
  overflow-y: auto;
}
.settings-search {
  padding: 2px 2px 8px;
  flex-shrink: 0;
}
.settings-search input {
  width: 100%;
  box-sizing: border-box;
  padding: 7px 10px;
  font-size: 12.5px;
  color: var(--text-primary);
  background: var(--bg-primary);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  outline: none;
  -webkit-appearance: none;
  appearance: none;
}
.settings-search input:focus {
  border-color: var(--accent);
}
.search-results {
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow-y: auto;
}
.search-item {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: 2px;
  padding: 7px 12px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  cursor: pointer;
  text-align: left;
}
.search-item:hover {
  background: var(--bg-hover);
}
.search-item-label {
  font-size: var(--text-base);
  color: var(--text-primary);
}
.search-item-section {
  font-size: 11px;
  color: var(--text-muted);
}
.search-empty {
  padding: 12px;
  font-size: 12px;
  color: var(--text-muted);
  text-align: center;
}
.nav-group-label {
  padding: 10px 12px 4px;
  font-size: 11px;
  font-weight: 600;
  color: var(--text-muted);
  letter-spacing: 0.08em;
  user-select: none;
}
.nav-group-label:first-child {
  padding-top: 2px;
}
.nav-item {
  position: relative;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: var(--text-base);
  color: var(--text-secondary);
  transition:
    background var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease);
  user-select: none;
}
.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-item.active {
  background: var(--accent-soft);
  color: var(--accent);
  font-weight: 600;
}
.nav-item.active .nav-icon {
  color: var(--accent);
}
.nav-icon {
  width: 18px;
  height: 18px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  color: inherit;
}
.nav-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 右侧内容 */
.settings-content {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  min-height: 0;
}
.settings-scroll {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
  min-height: 0;
}
.settings-scroll::-webkit-scrollbar {
  width: 6px;
}
.settings-scroll::-webkit-scrollbar-thumb {
  background: var(--bg-active);
  border-radius: 3px;
}
.dialog-title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}
</style>

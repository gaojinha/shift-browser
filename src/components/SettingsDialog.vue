<script setup lang="ts">
// 设置弹窗（通用 / 外观 / AI 助手 / 隐私 / 扩展）
// 设置项存 settings 表（key-value），保存即生效
import { ref, watch } from 'vue'
import { setWebviewVisible } from '../utils/webview'
import { useSettingsStore } from '../stores/settings'
import { useToastStore } from '../stores/toast'
import GeneralSection from './settings/GeneralSection.vue'
import SelfCheckSection from './settings/SelfCheckSection.vue'
import AboutSection from './settings/AboutSection.vue'
import AiSection from './settings/AiSection.vue'
import ExtensionsSection from './settings/ExtensionsSection.vue'
import ApiSection from './settings/ApiSection.vue'
import McpSection from './settings/McpSection.vue'
import AppearanceSection from './settings/AppearanceSection.vue'
import PrivacySection from './settings/PrivacySection.vue'
import DataSection from './settings/DataSection.vue'

const props = defineProps<{ show: boolean; initialSection?: string }>()
const emit = defineEmits<{ (e: 'close'): void; (e: 'openCerts'): void }>()

const settingsStore = useSettingsStore()

// 侧边导航：当前激活区块
const activeSection = ref('general')
// 外部指定初始选项卡（状态栏隐私战绩点击 → 跳隐私防护；打开后消费一次并复位）
watch(
  () => props.show,
  (v) => {
    if (v && props.initialSection) {
      activeSection.value = props.initialSection
    }
  },
)
const SECTIONS = [
  { id: 'general', icon: 'house', label: '通用' },
  { id: 'appearance', icon: 'palette', label: '外观' },
  { id: 'ai', icon: 'sparkles', label: 'AI 助手' },
  { id: 'mcp', icon: 'plug', label: 'MCP 工具' },
  { id: 'extensions', icon: 'puzzle', label: '扩展' },
  { id: 'privacy', icon: 'shield', label: '隐私防护' },
  { id: 'data', icon: 'lock', label: '数据安全' },
  { id: 'api', icon: 'plug', label: '开放 API' },
  { id: 'selfcheck', icon: 'activity', label: '自检' },
  { id: 'about', icon: 'info', label: '关于' },
]

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
          <div
            v-for="s in SECTIONS"
            :key="s.id"
            class="nav-item"
            :class="{ active: activeSection === s.id }"
            @click="activeSection = s.id"
          >
            <span class="nav-icon"><AppIcon :name="s.icon" :size="16" /></span>
            <span class="nav-label">{{ s.label }}</span>
          </div>
        </div>

        <!-- 右侧内容 -->
        <div class="settings-content">
          <!-- 通用 -->
          <GeneralSection v-if="activeSection === 'general'" />
          <AppearanceSection v-if="activeSection === 'appearance'" />

          <!-- AI 助手 -->
          <AiSection v-if="activeSection === 'ai'" />

          <!-- MCP 工具（阶段 1.3：AI 面板可调用外部工具） -->
          <McpSection v-if="activeSection === 'mcp'" />

          <!-- 扩展 -->
          <ExtensionsSection v-if="activeSection === 'extensions'" />

          <!-- 隐私防护 -->
          <PrivacySection v-if="activeSection === 'privacy'" />
          <!-- 数据安全 -->
          <DataSection v-if="activeSection === 'data'" @open-certs="emit('openCerts')" />

          <!-- 开放 API -->
          <ApiSection v-if="activeSection === 'api'" />
          <!-- 自检 -->
          <SelfCheckSection v-if="activeSection === 'selfcheck'" />
          <AboutSection v-if="activeSection === 'about'" />
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
  border-right: 1px solid var(--border-subtle);
  padding: 12px 10px;
  display: flex;
  flex-direction: column;
  gap: 3px;
  background: var(--bg-primary);
}
.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 9px 12px;
  border-radius: var(--radius-sm);
  cursor: pointer;
  font-size: 13px;
  color: var(--text-secondary);
  transition:
    background var(--dur),
    color var(--dur),
    box-shadow var(--dur);
  user-select: none;
}
.nav-item:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.nav-item.active {
  background: var(--grad-accent);
  color: #fff;
  font-weight: 600;
  box-shadow: var(--shadow-sm);
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
}
.dialog-title {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}
</style>

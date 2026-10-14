<script setup lang="ts">
// 设置弹窗 · 通用分区（R2-1d 拆分：原 SettingsDialog.vue 内联节抽出）
// 默认浏览器 / 主页 / 搜索引擎 / 标签休眠 / 垂直标签栏 / 翻译
// IA 重组（2026-09）：定时任务三块已移至「扩展与自动化」AutomationCard.vue
import { computed, onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'
import { SEARCH_ENGINES } from '../../constants'
import SettingCard from './ui/SettingCard.vue'
import SettingSwitch from './ui/SettingSwitch.vue'
import SegControl from './ui/SegControl.vue'
import HintBlock from './ui/HintBlock.vue'
import './ui/settings-shared.css'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

/** 分段控件双向绑定（读 store、写 onSet，保存即生效） */
const searchEngine = computed({
  get: () => settingsStore.get('search_engine') || 'bing',
  set: (v: string) => void onSet('search_engine', v),
})
const hibernateMinutes = computed({
  get: () => settingsStore.get('tab_hibernate_minutes') || '10',
  set: (v: string) => void onSet('tab_hibernate_minutes', v),
})
const translateEngine = computed({
  get: () => settingsStore.get('translate_engine') || 'google',
  set: (v: string) => void onSet('translate_engine', v),
})
const searchOptions = SEARCH_ENGINES.map((e) => ({ value: e.id, label: e.label }))

/** 标签休眠超时选项（分钟，0=关闭；后端 hibernate_idle_tabs 读该设置） */
const HIBERNATE_OPTIONS = [
  { v: '0', label: '关闭' },
  { v: '10', label: '10 分钟' },
  { v: '30', label: '30 分钟' },
  { v: '60', label: '60 分钟' },
]

/** 翻译源（后端 translate.rs 按 settings.translate_engine 派发，失败自动回退另一方） */
const TRANSLATE_ENGINES = [
  { id: 'google', label: 'Google' },
  { id: 'bing', label: 'Bing' },
]

/** 翻译目标语言（Google/Bing 语言码，Bing 后端对 zh-CN 自动映射 zh-Hans） */
const TRANSLATE_TARGETS = [
  { id: 'zh-CN', label: '中文（简体）' },
  { id: 'zh-TW', label: '中文（繁体）' },
  { id: 'en', label: '英语' },
  { id: 'ja', label: '日语' },
  { id: 'ko', label: '韩语' },
  { id: 'fr', label: '法语' },
  { id: 'de', label: '德语' },
  { id: 'ru', label: '俄语' },
  { id: 'es', label: '西班牙语' },
  { id: 'pt', label: '葡萄牙语' },
  { id: 'it', label: '意大利语' },
]

// 默认浏览器（阶段 4：xdg-settings 集成）
// 状态：is_default 是否已是默认；current 当前默认 desktop-id；desktop_ready 用户级声明是否就绪
const defaultBrowser = ref<{
  is_default: boolean
  current: string
  desktop_id: string
  desktop_ready: boolean
}>({ is_default: false, current: '', desktop_id: '', desktop_ready: false })
const dbBusy = ref(false)
async function loadDefaultBrowser() {
  try {
    defaultBrowser.value = await invoke('default_browser_status')
  } catch (e) {
    console.error('[settings] 读取默认浏览器状态失败:', e)
  }
}
async function setAsDefaultBrowser() {
  if (dbBusy.value) return
  dbBusy.value = true
  try {
    defaultBrowser.value = await invoke('set_default_browser')
    useToastStore().success('已设为默认浏览器')
  } catch (e) {
    console.error('[settings] 设置默认浏览器失败:', e)
    useToastStore().error('设置失败：' + e)
  } finally {
    dbBusy.value = false
  }
}

onMounted(loadDefaultBrowser)
</script>

<template>
  <div class="section-stack">
    <SettingCard title="通用" icon="monitor">
      <div class="field">
        <label>默认浏览器</label>
        <span class="field-desc">
          {{
            defaultBrowser.is_default
              ? '✅ 当前已是默认浏览器'
              : (defaultBrowser.current
                  ? '当前默认：' + defaultBrowser.current
                  : '未检测到默认浏览器设置')
          }}
        </span>
        <button
          class="mini-btn primary"
          :disabled="defaultBrowser.is_default || dbBusy"
          @click="setAsDefaultBrowser"
        >
          {{ dbBusy ? '设置中…' : defaultBrowser.is_default ? '已是默认' : '设为默认浏览器' }}
        </button>
      </div>
      <HintBlock>
        设为默认后，从邮件、聊天等外部应用点开的链接会直接用 Shift Browser 打开。
        系统级 desktop 文件若缺浏览器声明，会自动写入用户级声明（无需 root）。
      </HintBlock>
      <div class="field">
        <label for="homepage-input">主页 URL</label>
        <input
          id="homepage-input"
          type="text"
          :value="settingsStore.get('homepage')"
          placeholder="newtab 或 https://..."
          spellcheck="false"
          @change="onSet('homepage', ($event.target as HTMLInputElement).value.trim())"
        />
      </div>
      <div class="field">
        <label>搜索引擎</label>
        <SegControl v-model="searchEngine" :options="searchOptions" label="搜索引擎" />
      </div>
      <div class="field">
        <label>标签休眠</label>
        <SegControl
          v-model="hibernateMinutes"
          :options="HIBERNATE_OPTIONS.map((h) => ({ value: h.v, label: h.label }))"
          label="标签休眠"
        />
      </div>
      <HintBlock>
        闲置超过该时长的标签会自动休眠：<strong>销毁它的渲染进程、立刻释放内存</strong>（一个标签
        240-500MB），点击该标签时按原网址重新加载。标签开得多、内存吃紧时特别有用。
      </HintBlock>
      <div class="field">
        <label for="hibernate-count">休眠标签数阈值</label>
        <input
          id="hibernate-count"
          class="task-num hibernate-count"
          type="number"
          min="0"
          :value="settingsStore.get('tab_hibernate_count') || '8'"
          @change="onSet('tab_hibernate_count', ($event.target as HTMLInputElement).value)"
        />
        <span class="field-desc">
          个（0=只按上方时长休眠；打开的标签超过该数量时，切走的后台标签立即休眠）
        </span>
      </div>
      <div class="field">
        <label for="vertical-tabs">垂直标签栏</label>
        <span class="field-desc">标签栏移到左侧竖排，适合多标签</span>
        <SettingSwitch
          label="垂直标签栏"
          :checked="settingsStore.get('vertical_tabs') === 'true'"
          @change="onSet('vertical_tabs', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label for="update-check">启动查版本</label>
        <span class="field-desc">启动时检查新版本并提示（只提示，不自动安装）</span>
        <SettingSwitch
          label="启动查版本"
          :checked="settingsStore.get('update_check_enabled') === 'true'"
          @change="onSet('update_check_enabled', $event ? 'true' : 'false')"
        />
      </div>
      <div class="field">
        <label>翻译源</label>
        <SegControl
          v-model="translateEngine"
          :options="TRANSLATE_ENGINES.map((t) => ({ value: t.id, label: t.label }))"
          label="翻译源"
        />
      </div>
      <HintBlock>
        「翻译当前页面」、右键划词翻译、翻译面板都用这个源。<br />
        <b>Google</b>：译文质量好，但国内网络常连不上；<b>Bing</b>（Edge 端点）国内一般可直连。
        任一源失败会自动回退到另一方，改用即时生效、无需重启。
      </HintBlock>
      <div class="field">
        <label for="translate-target">翻译目标语言</label>
        <select
          id="translate-target"
          :value="settingsStore.get('translate_target') || 'zh-CN'"
          @change="onSet('translate_target', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="t in TRANSLATE_TARGETS" :key="t.id" :value="t.id">
            {{ t.label }}
          </option>
        </select>
      </div>
      <HintBlock>
        翻译面板、划词翻译、整页翻译都按这个目标语言输出，切换即时生效。
      </HintBlock>
      <div class="field">
        <label for="translate-timeout">翻译超时（秒）</label>
        <input
          id="translate-timeout"
          type="number"
          min="5"
          max="120"
          step="5"
          :value="settingsStore.get('translate_timeout') || '15'"
          @change="onSet('translate_timeout', ($event.target as HTMLInputElement).value)"
        />
      </div>
      <HintBlock>
        翻译请求等待的最长时间（5–120 秒，默认 15）。网络慢或译文较长时调大；
        保存后新打开的页面立即生效，已打开页面刷新后生效。
      </HintBlock>
    </SettingCard>
  </div>
</template>

<style scoped>
/* 通用样式已收口至 ui/settings-shared.css，此处仅留文件特有样式（当前无） */
</style>

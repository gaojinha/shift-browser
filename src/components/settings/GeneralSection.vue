<script setup lang="ts">
// 设置弹窗 · 通用分区（R2-1d 拆分：原 SettingsDialog.vue 内联节抽出）
// 默认浏览器 / 主页 / 搜索引擎 / 标签休眠 / 垂直标签栏 / 翻译
// IA 重组（2026-09）：定时任务三块已移至「扩展与自动化」AutomationCard.vue
import { onMounted, ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useSettingsStore } from '../../stores/settings'
import { useSettingsSet } from '../../composables/useSettingsSet'
import { useToastStore } from '../../stores/toast'
import { SEARCH_ENGINES } from '../../constants'

const settingsStore = useSettingsStore()
const { onSet } = useSettingsSet()

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
    <div class="card">
      <div class="card-title"><AppIcon name="monitor" :size="14" /> 通用</div>
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
      <div class="kw-hint">
        设为默认后，从邮件、聊天等外部应用点开的链接会直接用 Shift Browser 打开。
        系统级 desktop 文件若缺浏览器声明，会自动写入用户级声明（无需 root）。
      </div>
      <div class="field">
        <label>主页 URL</label>
        <input
          type="text"
          :value="settingsStore.get('homepage')"
          placeholder="newtab 或 https://..."
          spellcheck="false"
          @change="onSet('homepage', ($event.target as HTMLInputElement).value.trim())"
        />
      </div>
      <div class="field">
        <label>搜索引擎</label>
        <div class="seg-group">
          <button
            v-for="e in SEARCH_ENGINES"
            :key="e.id"
            class="seg-btn"
            :class="{ active: (settingsStore.get('search_engine') || 'bing') === e.id }"
            @click="onSet('search_engine', e.id)"
          >
            {{ e.label }}
          </button>
        </div>
      </div>
      <div class="field">
        <label>标签休眠</label>
        <div class="seg-group">
          <button
            v-for="h in HIBERNATE_OPTIONS"
            :key="h.v"
            class="seg-btn"
            :class="{ active: (settingsStore.get('tab_hibernate_minutes') || '30') === h.v }"
            @click="onSet('tab_hibernate_minutes', h.v)"
          >
            {{ h.label }}
          </button>
        </div>
      </div>
      <div class="kw-hint">
        闲置超过该时长的标签会自动休眠：<strong>销毁它的渲染进程、立刻释放内存</strong>（一个标签
        240-500MB），点击该标签时按原网址重新加载。标签开得多、内存吃紧时特别有用。
      </div>
      <div class="field">
        <label>休眠标签数阈值</label>
        <input
          class="task-num hibernate-count"
          type="number"
          min="0"
          :value="settingsStore.get('tab_hibernate_count') || '0'"
          @change="onSet('tab_hibernate_count', ($event.target as HTMLInputElement).value)"
        />
        <span class="field-desc">
          个（0=只按上方时长休眠；打开的标签超过该数量时，切走的后台标签立即休眠）
        </span>
      </div>
      <div class="field">
        <label>垂直标签栏</label>
        <span class="field-desc">标签栏移到左侧竖排，适合多标签</span>
        <label class="switch">
          <input
            type="checkbox"
            :checked="settingsStore.get('vertical_tabs') === 'true'"
            @change="onSet('vertical_tabs', ($event.target as HTMLInputElement).checked ? 'true' : 'false')"
          />
          <span class="slider"></span>
        </label>
      </div>
      <div class="field">
        <label>翻译源</label>
        <div class="seg-group">
          <button
            v-for="t in TRANSLATE_ENGINES"
            :key="t.id"
            class="seg-btn"
            :class="{ active: (settingsStore.get('translate_engine') || 'google') === t.id }"
            @click="onSet('translate_engine', t.id)"
          >
            {{ t.label }}
          </button>
        </div>
      </div>
      <div class="kw-hint">
        「翻译当前页面」、右键划词翻译、翻译面板都用这个源。<br />
        <b>Google</b>：译文质量好，但国内网络常连不上；<b>Bing</b>（Edge 端点）国内一般可直连。
        任一源失败会自动回退到另一方，改用即时生效、无需重启。
      </div>
      <div class="field">
        <label>翻译目标语言</label>
        <select
          :value="settingsStore.get('translate_target') || 'zh-CN'"
          @change="onSet('translate_target', ($event.target as HTMLSelectElement).value)"
        >
          <option v-for="t in TRANSLATE_TARGETS" :key="t.id" :value="t.id">
            {{ t.label }}
          </option>
        </select>
      </div>
      <div class="kw-hint">
        翻译面板、划词翻译、整页翻译都按这个目标语言输出，切换即时生效。
      </div>
      <div class="field">
        <label>翻译超时（秒）</label>
        <input
          type="number"
          min="5"
          max="120"
          step="5"
          :value="settingsStore.get('translate_timeout') || '15'"
          @change="onSet('translate_timeout', ($event.target as HTMLInputElement).value)"
        />
      </div>
      <div class="kw-hint">
        翻译请求等待的最长时间（5–120 秒，默认 15）。网络慢或译文较长时调大；
        保存后新打开的页面立即生效，已打开页面刷新后生效。
      </div>
    </div>
  </div>
</template>

<style scoped>
.settings-scroll { flex: 1; overflow-y: auto; padding: 14px 16px; }.settings-scroll::-webkit-scrollbar { width: 6px; }.settings-scroll::-webkit-scrollbar-thumb { background: var(--bg-active); border-radius: 3px; }.card { margin-bottom: 12px; }.card-title { display: flex; align-items: center; gap: 7px; font-size: 14px; color: var(--text-primary); font-weight: 600; margin-bottom: 12px; padding-bottom: 8px; border-bottom: 1px solid var(--border-subtle); }.card-title .app-icon { color: var(--accent); }.field { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 10px; }.field-desc { flex: 1; font-size: 12px; color: var(--text-secondary); }.field label { font-size: 13px; color: var(--text-secondary); flex-shrink: 0; }.field input,
.field select { flex: 1; max-width: 300px; border: 1px solid var(--border-color); background: var(--bg-primary); color: var(--text-primary); border-radius: 7px; padding: 7px 11px; font-size: 13px; outline: none; transition: border-color 0.15s; -webkit-appearance: none; appearance: none; }.field select { padding-right: 26px; background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'><path fill='%23c6d0df' d='M2.5 4.5L6 8l3.5-3.5'/></svg>"); background-repeat: no-repeat; background-position: right 8px center; background-size: 12px; }.field input:focus,
.field select:focus { border-color: var(--accent); }.seg-group { display: flex; gap: 6px; flex: 1; max-width: 300px; }.seg-btn { flex: 1; padding: 6px 0; border: 1px solid var(--border-color); background: var(--bg-primary); color: var(--text-secondary); border-radius: 7px; font-size: 13px; cursor: pointer; transition: all 0.15s; }.seg-btn:hover { background: var(--bg-hover); }.seg-btn.active { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); font-weight: 600; }.switch { position: relative; display: inline-block; width: 36px; height: 20px; flex-shrink: 0; }.switch input { opacity: 0; width: 0; height: 0; }.slider { position: absolute; cursor: pointer; inset: 0; background: var(--bg-active); border-radius: 10px; transition: 0.2s; }.slider::before { content: ''; position: absolute; width: 14px; height: 14px; left: 3px; top: 3px; background: var(--text-muted); border-radius: 50%; transition: 0.2s; }.switch input:checked + .slider { background: var(--accent); }.switch input:checked + .slider::before { transform: translateX(16px); background: #fff; }.kw-block { margin: 4px 0 12px 0; padding: 10px 12px; background: var(--bg-primary); border: 1px solid var(--border-subtle); border-radius: 8px; }.kw-hint { font-size: 11px; color: var(--text-muted); margin-bottom: 8px; line-height: 1.4; }.net-rules { width: 100%; box-sizing: border-box; min-height: 64px; padding: 8px; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-secondary); color: var(--text-primary); font-size: 12px; font-family: monospace; resize: vertical; outline: none; }.net-rules:focus { border-color: var(--accent); }.task-num { width: 72px; height: 28px; padding: 0 8px; border: 1px solid var(--border-color); border-radius: 6px; background: var(--bg-primary); color: var(--text-primary); font-size: 12px; outline: none; }.task-num:focus { border-color: var(--accent); }.mini-btn { background: var(--bg-active); color: var(--text-primary); border: 1px solid var(--border-color); border-radius: 7px; padding: 5px 12px; font-size: 12px; font-weight: 500; cursor: pointer; transition: all 0.15s; white-space: nowrap; }.mini-btn:hover { background: var(--accent-soft); color: var(--accent); border-color: var(--accent); }.mini-btn:disabled { opacity: 0.45; cursor: default; }.mini-btn.primary { background: var(--accent); color: #fff; border-color: var(--accent); }.mini-btn.primary:hover { background: var(--accent-2); color: #fff; border-color: var(--accent-2); }
</style>

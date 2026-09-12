// 设置状态管理（Pinia）
// 对应 settings 表（key-value），封装 get_all_settings / set_setting / clear_history
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'

interface SettingRow {
  key: string
  value: string
}

// 默认设置（settings 表无记录时生效）
export const DEFAULT_SETTINGS: Record<string, string> = {
  homepage: 'newtab', // 'newtab' = 内置新标签页（原版左侧栏面板），也可填任意 URL
  search_engine: 'bing', // bing | baidu | google
  theme: 'dark', // dark | light
  show_bookmarks_bar: 'true',
  record_history: 'true',
  ai_model: 'agnes-2.0-flash', // AI 模型（agnes-2.0-flash 稳定 / agnes-2.5-pro / agnes-2.5-flash 可能超时）
  ai_base_url: 'https://apihub.agnes-ai.com/v1', // AI API 地址
  ai_api_key: '', // AI API Key（留空使用环境变量 AGNES_API_KEY）
}

interface SettingsState {
  settings: Record<string, string>
  loaded: boolean
}

export const useSettingsStore = defineStore('settings', {
  state: (): SettingsState => ({
    settings: {},
    loaded: false,
  }),
  getters: {
    /** 读取设置（无记录时返回默认值） */
    get: (state) => (key: string): string => {
      if (state.settings[key] !== undefined) return state.settings[key]
      return DEFAULT_SETTINGS[key] ?? ''
    },
    /** 是否为浅色主题 */
    isLight: (state) => {
      const v = state.settings['theme']
      return (v ?? DEFAULT_SETTINGS.theme) === 'light'
    },
    /** 是否显示书签栏 */
    showBookmarksBar: (state) => {
      const v = state.settings['show_bookmarks_bar']
      return (v ?? DEFAULT_SETTINGS.show_bookmarks_bar) === 'true'
    },
  },
  actions: {
    async loadSettings() {
      const rows = await invoke<SettingRow[]>('get_all_settings')
      this.settings = {}
      for (const r of rows) this.settings[r.key] = r.value
      this.loaded = true
    },
    /** 保存单个设置（内存 + 数据库） */
    async set(key: string, value: string) {
      this.settings[key] = value
      await invoke('set_setting', { key, value })
    },
    /** 清除全部历史记录 */
    async clearHistory() {
      await invoke('clear_history')
    },
  },
})
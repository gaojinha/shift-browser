// 设置状态管理（Pinia）
// 对应 settings 表（key-value），封装 get_all_settings / set_setting / clear_history
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'

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
  translate_target: 'zh-CN', // 翻译目标语言（Google/Bing 语言码）
  translate_timeout: '15', // 翻译超时（秒，5-120，默认 15）
  // 以下默认值与后端一致（2026-09-19 补齐，避免 get() 回退空串导致状态栏/设置显示失真）
  translate_engine: 'google', // 翻译源（google / bing，失败自动回退另一方）
  adblock_enabled: 'true', // 广告拦截（默认开启）
  privacy_shield_level: 'standard', // 隐私防护档位（off / standard / strict）
  privacy_shield: 'true', // 隐私防护旧布尔开关（兼容）
  tab_hibernate_minutes: '30', // 空闲标签休眠超时（分钟，0=关闭）
  lock_enabled: 'true', // 启动锁屏
  show_lunar: 'true', // 新标签页显示农历
  show_quote: 'true', // 新标签页显示每日一句
  show_weather: 'true', // 新标签页显示天气
  show_bg: 'true', // 新标签页显示壁纸
  font_size: '16', // 新标签页字号
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
    get:
      (state) =>
      (key: string): string => {
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
      // 启动路径：拉取失败不能中断启动流程（2026-09-19 修复），
      // 用空表 + 默认值兜底，后续 get() 回退 DEFAULT_SETTINGS
      try {
        const rows = await invoke<SettingRow[]>('get_all_settings')
        this.settings = {}
        for (const r of rows) this.settings[r.key] = r.value
      } catch (e) {
        console.error('[settings] 加载设置失败，使用默认值:', e)
        this.settings = {}
      }
      this.loaded = true
    },
    /** 保存单个设置（内存 + 数据库，失败回滚内存值） */
    async set(key: string, value: string) {
      const prev = this.settings[key]
      try {
        this.settings[key] = value
        await invoke('set_setting', { key, value })
      } catch (e) {
        console.error('[settings] 保存设置失败:', e)
        this.settings[key] = prev
        useToastStore().error('保存设置失败')
      }
    },
    /** 清除全部历史记录 */
    async clearHistory() {
      try {
        await invoke('clear_history')
      } catch (e) {
        console.error('[settings] 清除历史失败:', e)
        useToastStore().error('清除历史记录失败')
      }
    },
  },
})

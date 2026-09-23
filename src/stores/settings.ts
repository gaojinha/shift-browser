// 设置状态管理（Pinia）
// 对应 settings 表（key-value），封装 get_all_settings / set_setting / clear_history
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'
import { DEFAULT_SETTINGS } from '../constants'

interface SettingRow {
  key: string
  value: string
}

// 默认设置来自 contracts.rs（单一来源，build.rs 生成 src/constants.ts）——不再手写，
// 后端白名单与前端默认值同源，杜绝"注释写与后端一致"的人肉同步

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

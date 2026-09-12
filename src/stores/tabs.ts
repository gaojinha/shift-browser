// 标签页状态管理（Pinia）
// 对应 Electron 版 renderer 的 tabs 状态 + TabManager 命令调用
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import type { Tab } from '../types/tab'
import type { Session, SessionTab } from '../types/session'
import { useGroupsStore } from './groups'

interface TabsState {
  tabs: Tab[]
  activeTabId: string | null
  pollTimer: number | null
  /** 切换中标记（合并连点，防消息排队） */
  _switching: boolean
}

export const useTabsStore = defineStore('tabs', {
  state: (): TabsState => ({
    tabs: [],
    activeTabId: null,
    pollTimer: null,
    _switching: false,
  }),

  getters: {
    activeTab(state): Tab | null {
      return state.tabs.find((t) => t.id === state.activeTabId) ?? null
    },
  },

actions: {
    /** 从 Rust 拉取全部标签（启动时调用） */
    async loadTabs() {
      const tabs = await invoke<Tab[]>('get_tabs')
      // Rust 返回的 Tab 无 groupId 字段，补默认值（未分组）
      this.tabs = tabs.map((t) => ({ ...t, groupId: t.groupId ?? null }))
      const active = tabs.find((t) => t.is_active)
      this.activeTabId = active?.id ?? null
    },

    /** 分配标签到分组（groupId = null 表示取消分组） */
    assignTabToGroup(tabId: string, groupId: number | null) {
      const tab = this.tabs.find((t) => t.id === tabId)
      if (tab) tab.groupId = groupId
    },

    /** 创建新标签页 */
    async createTab(url = 'https://www.bing.com') {
      const tab = await invoke<Tab>('create_tab', { url })
      this.tabs.push(tab)
      // 取消旧标签激活
      for (const t of this.tabs) {
        if (t.id !== tab.id) t.is_active = false
      }
      this.activeTabId = tab.id
      return tab
    },

    /** 关闭标签页 */
    async closeTab(id: string) {
      const next = await invoke<Tab | null>('close_tab', { id })
      this.tabs = this.tabs.filter((t) => t.id !== id)
      if (this.activeTabId === id) {
        this.activeTabId = next?.id ?? null
        if (next) {
          for (const t of this.tabs) {
            t.is_active = t.id === next.id
          }
        }
      }
    },

    /** 切换标签页 */
    async switchTab(id: string) {
      // 合并连点：上一次切换未返回时忽略新点击（快速连点会让主线程消息排队逐个重绘，越切越卡）
      if (this._switching) return
      this._switching = true
      try {
        const tab = await invoke<Tab | null>('switch_tab', { id })
        if (!tab) return
        for (const t of this.tabs) {
          t.is_active = t.id === id
        }
        this.activeTabId = id
      } finally {
        this._switching = false
      }
    },

    /** 导航到新 URL */
    async navigateTab(id: string, url: string) {
      const tab = await invoke<Tab | null>('navigate_tab', { id, url })
      if (tab) {
        const idx = this.tabs.findIndex((t) => t.id === id)
        if (idx !== -1) this.tabs[idx] = tab
      }
    },

    /** 后退（当前活动标签） */
    async goBack() {
      if (!this.activeTabId) return
      await invoke<Tab | null>('go_back')
    },

    /** 前进（当前活动标签） */
    async goForward() {
      if (!this.activeTabId) return
      await invoke<Tab | null>('go_forward')
    },

    /** 同步标签元数据（标题/图标） */
    async updateTabMeta(id: string, title?: string, favicon?: string) {
      const tab = await invoke<Tab | null>('update_tab', { id, title, favicon })
      if (tab) {
        const idx = this.tabs.findIndex((t) => t.id === id)
        if (idx !== -1) this.tabs[idx] = { ...tab, groupId: this.tabs[idx].groupId ?? null }
      }
    },

    /** 恢复指定工作区最近会话（有会话则逐个恢复标签，返回是否恢复成功） */
    async restoreLastSession(workspaceId: number | null = null): Promise<boolean> {
      const session = await invoke<Session | null>('load_last_session', { workspaceId })
      if (!session) return false
      let tabs: SessionTab[] = []
      try {
        tabs = JSON.parse(session.data) as SessionTab[]
      } catch {
        return false
      }
      // 分组可能已被删除，恢复前确保分组列表已加载（校验 groupId 存在性）
      const groups = useGroupsStore()
      if (groups.groups.length === 0) {
        await groups.loadGroups().catch(() => {})
      }
      for (const t of tabs) {
        if (t.url) {
          const tab = await this.createTab(t.url)
          // 恢复分组归属（groupId 是数据库持久 id，按 session 顺序匹配；分组已删除则回未分组）
          if (t.groupId != null && tab) {
            const exists = groups.groups.some((g) => g.id === t.groupId)
            this.assignTabToGroup(tab.id, exists ? t.groupId : null)
          }
        }
      }
      return tabs.length > 0
    },

    /** 保存当前会话（标签页快照，含分组归属），workspaceId = 所属工作区（null = 默认） */
    async saveCurrentSession(workspaceId: number | null = null) {
      const tabs: SessionTab[] = this.tabs.map((t) => ({
        url: t.url,
        title: t.title,
        groupId: t.groupId ?? null,
      }))
      if (tabs.length === 0) return
      try {
        await invoke<number>('save_session', {
          name: '自动保存',
          workspaceId,
          tabs,
        })
      } catch (e) {
        console.error('[tabs] 保存会话失败:', e)
      }
    },

    /** 关闭全部标签（切换工作区时清空用） */
    async clearAllTabs() {
      try {
        await invoke('close_all_tabs')
      } catch (e) {
        console.error('[tabs] 清空标签失败:', e)
      }
      this.tabs = []
      this.activeTabId = null
    },

    /**
     * 启动标题/URL 轮询（Rust 侧 on_page_load 更新状态，前端定时拉取同步）
     * 说明：Tauri 2 内置无 event listen 插件（tauri-plugin-event 不存在于 crates.io），
     * 事件方案走不通，改用轮询。
     */
    startPolling(interval = 2000) {
      if (this.pollTimer) return
      this.pollTimer = window.setInterval(async () => {
        try {
          const tabs = await invoke<Tab[]>('get_tabs')
          // 保留前端运行时 groupId（Rust 侧无此字段）
          const prevGroups = new Map(this.tabs.map((t) => [t.id, t.groupId]))
          this.tabs = tabs.map((t) => ({ ...t, groupId: prevGroups.get(t.id) ?? null }))
          const active = tabs.find((t) => t.is_active)
          if (active) this.activeTabId = active.id
        } catch (e) {
          console.error('[tabs] 轮询标签失败:', e)
        }
      }, interval)
    },

    /** 停止轮询（组件卸载时调用） */
    stopPolling() {
      if (this.pollTimer) {
        clearInterval(this.pollTimer)
        this.pollTimer = null
      }
    },
  },
})
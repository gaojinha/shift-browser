// 标签页状态管理（Pinia）
// 对应 Electron 版 renderer 的 tabs 状态 + TabManager 命令调用
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { Tab } from '../types/tab'
import type { Session, SessionTab } from '../types/session'
import { useGroupsStore } from './groups'
import { useSettingsStore } from './settings'
import { useToastStore } from './toast'

interface TabsState {
  tabs: Tab[]
  activeTabId: string | null
  pollTimer: number | null
  /** tabs-updated 事件取消监听句柄 */
  _unlisten: (() => void) | null
  /** 切换中标记（合并连点，防消息排队） */
  _switching: boolean
  /** 用户操作自增令牌：close/switch 与轮询并发时，过期回调不回写 activeTabId */
  _opSeq: number
  /** 正在加载的标签 id（地址栏刷新按钮转圈/进度条用） */
  loadingTabId: string | null
  /** 当前活动标签是否可后退/前进（前端乐观追踪，webview 内链跳也会在轮询里补 back） */
  canGoBack: boolean
  canGoForward: boolean
}

export const useTabsStore = defineStore('tabs', {
  state: (): TabsState => ({
    tabs: [],
    activeTabId: null,
    pollTimer: null,
    _unlisten: null,
    _switching: false,
    _opSeq: 0,
    loadingTabId: null,
    canGoBack: false,
    canGoForward: false,
  }),

  getters: {
    activeTab(state): Tab | null {
      return state.tabs.find((t) => t.id === state.activeTabId) ?? null
    },
  },

  actions: {
    /** 从 Rust 拉取全部标签（启动时调用），失败不抛（App.vue 初始化链依赖此方法不炸） */
    async loadTabs() {
      try {
        const tabs = await invoke<Tab[]>('get_tabs')
        // Rust 返回的 Tab 无 groupId 字段，补默认值（未分组）
        this.tabs = tabs.map((t) => ({ ...t, groupId: t.groupId ?? null }))
        const active = tabs.find((t) => t.is_active)
        this.activeTabId = active?.id ?? null
      } catch (e) {
        console.error('[tabs] 加载标签失败:', e)
        useToastStore().error('加载标签页失败')
      }
    },

    /** 分配标签到分组（groupId = null 表示取消分组） */
    assignTabToGroup(tabId: string, groupId: number | null) {
      const tab = this.tabs.find((t) => t.id === tabId)
      if (tab) tab.groupId = groupId
    },

    /** 创建新标签页（无参时按主页设置解析；'newtab' = 本地新标签页）
     *  @param groupId 可选：创建后把新标签归入该分组（group 是前端运行时概念，后端 Tab 无此字段，
     *                 所以在 invoke 返回后由前端补写 groupId，不依赖 Rust 返回） */
    async createTab(url?: string, groupId?: number | null): Promise<Tab | null> {
      // 参与 _opSeq：防止 create_tab 在途时 syncTabs（轮询）用旧快照整体覆盖，把刚建的标签顶掉
      const my = ++this._opSeq
      let target = url || useSettingsStore().get('homepage') || 'newtab'
      if (target === 'newtab') {
        try {
          // 先刷新数据（书签/历史面板），再取 URL
          await invoke('refresh_newtab').catch((e) => {
            console.error('[tabs] 刷新新标签页数据失败:', e)
          })
          target = await invoke<string>('get_newtab_url')
        } catch {
          target = 'https://www.bing.com'
        }
      }
      try {
        const tab = await invoke<Tab>('create_tab', { url: target })
        // push 前去重：syncTabs 轮询可能已把该 id 带进列表（并发窗口），直接 push 会重复
        const dup = this.tabs.findIndex((t) => t.id === tab.id)
        if (dup !== -1) this.tabs[dup] = tab
        else this.tabs.push(tab)
        // 归组（可选）：createTab(url, groupId) 语义 —— 新建即归入给定分组。
        // groupId 是前端运行时概念（Rust Tab 无此字段，参见 54 行注释），
        // 所以通过现有 assignTabToGroup 幂等归组，不依赖后端回传。
        if (groupId != null) this.assignTabToGroup(tab.id, groupId)
        // 取消旧标签激活
        for (const t of this.tabs) {
          if (t.id !== tab.id) t.is_active = false
        }
        // 仅当无更新的用户操作时才接管激活，避免覆盖连点/切换结果
        if (my === this._opSeq) {
          this.activeTabId = tab.id
          this._markLoading(tab.id)
        }
        this.canGoBack = false
        this.canGoForward = false
        return tab
      } catch (e) {
        console.error('[tabs] 创建标签失败:', e)
        useToastStore().error('创建标签页失败')
        return null
      }
    },

    /** 关闭标签页 */
    async closeTab(id: string) {
      const my = ++this._opSeq
      try {
        const next = await invoke<Tab | null>('close_tab', { id })
        // 标签确实关了，列表总是同步（Rust 是权威）
        this.tabs = this.tabs.filter((t) => t.id !== id)
        // 但 activeTabId 归属只在没有更新的用户操作时回写，
        // 否则会覆盖用户刚点的 switchTab 结果
        if (my === this._opSeq && this.activeTabId === id) {
          this.activeTabId = next?.id ?? null
          if (next) {
            for (const t of this.tabs) {
              t.is_active = t.id === next.id
            }
          }
        }
      } catch (e) {
        console.error('[tabs] 关闭标签失败:', e)
        useToastStore().error('关闭标签页失败')
      }
    },

    /** 切换标签页 */
    async switchTab(id: string) {
      // 合并连点：上一次切换未返回时忽略新点击（快速连点会让主线程消息排队逐个重绘，越切越卡）
      if (this._switching) return
      this._switching = true
      const my = ++this._opSeq
      try {
        const tab = await invoke<Tab | null>('switch_tab', { id })
        // 过期回调：期间用户又关了/切了别的标签，不回写激活状态
        if (!tab || my !== this._opSeq) return
        for (const t of this.tabs) {
          t.is_active = t.id === id
        }
        this.activeTabId = id
        // 切标签时历史栈未知：乐观允许后退（用户刚从别的页切回来多半能退），前进关掉
        this.canGoBack = true
        this.canGoForward = false
      } catch (e) {
        console.error('[tabs] 切换标签失败:', e)
        useToastStore().error('切换标签页失败')
      } finally {
        this._switching = false
      }
    },

    /** 标记标签加载中，8 秒兜底关掉（同 URL 刷新时轮询可能看不出变化） */
    _markLoading(id: string) {
      this.loadingTabId = id
      window.setTimeout(() => {
        if (this.loadingTabId === id) this.loadingTabId = null
      }, 8000)
    },

    /** 导航到新 URL */
    async navigateTab(id: string, url: string) {
      this._markLoading(id)
      this.canGoBack = true
      this.canGoForward = false
      try {
        const tab = await invoke<Tab | null>('navigate_tab', { id, url })
        if (tab) {
          const idx = this.tabs.findIndex((t) => t.id === id)
          // 保留分组：Rust 返回的 Tab 无 groupId 字段，整体替换会掉组（与 pinTab/muteTab 对齐）
          if (idx !== -1) this.tabs[idx] = { ...tab, groupId: this.tabs[idx].groupId ?? null }
        }
      } catch (e) {
        console.error('[tabs] 导航失败:', e)
        useToastStore().error('页面导航失败')
        if (this.loadingTabId === id) this.loadingTabId = null
      }
    },

    /** 固定/取消固定标签（原版 tab-pin） */
    async pinTab(id: string) {
      try {
        const tab = await invoke<Tab | null>('toggle_pin', { id })
        if (tab) {
          const idx = this.tabs.findIndex((t) => t.id === id)
          if (idx !== -1) this.tabs[idx] = { ...tab, groupId: this.tabs[idx].groupId ?? null }
        }
      } catch (e) {
        console.error('[tabs] 固定标签失败:', e)
        useToastStore().error('固定标签失败')
      }
    },

    /** 静音/取消静音标签（原版 tab-mute） */
    async muteTab(id: string) {
      try {
        const tab = await invoke<Tab | null>('toggle_mute', { id })
        if (tab) {
          const idx = this.tabs.findIndex((t) => t.id === id)
          if (idx !== -1) this.tabs[idx] = { ...tab, groupId: this.tabs[idx].groupId ?? null }
        }
      } catch (e) {
        console.error('[tabs] 静音标签失败:', e)
        useToastStore().error('静音标签失败')
      }
    },

    /** 拖拽排序：把 fromId 移到 toId 的位置（前端先乐观更新，再通知 Rust） */
    async reorderTabs(fromId: string, toId: string) {
      const from = this.tabs.findIndex((t) => t.id === fromId)
      const to = this.tabs.findIndex((t) => t.id === toId)
      if (from === -1 || to === -1 || from === to) return
      const moved = this.tabs.splice(from, 1)[0]
      this.tabs.splice(to, 0, moved)
      try {
        await invoke('reorder_tabs', { ids: this.tabs.map((t) => t.id) })
      } catch (e) {
        console.error('[tabs] 排序同步失败:', e)
        // 失败回滚：重新拉取权威顺序
        const tabs = await invoke<Tab[]>('get_tabs').catch(() => [])
        if (tabs.length) this.tabs = tabs.map((t) => ({ ...t, groupId: t.groupId ?? null }))
      }
    },

    /** 刷新标签（Ctrl+R） */
    async reloadTab(id: string) {
      this._markLoading(id)
      try {
        const tab = await invoke<Tab | null>('reload_tab', { id })
        if (tab) {
          const idx = this.tabs.findIndex((t) => t.id === id)
          // 保留分组：reload 返回的 Tab 无 groupId，整体替换会掉组（对齐 navigateTab/pinTab）
          if (idx !== -1) this.tabs[idx] = { ...tab, groupId: this.tabs[idx].groupId ?? null }
        }
      } catch (e) {
        console.error('[tabs] 刷新失败:', e)
        useToastStore().error('刷新页面失败')
        if (this.loadingTabId === id) this.loadingTabId = null
      }
    },

    /** 后退（当前活动标签） */
    async goBack() {
      if (!this.activeTabId || !this.canGoBack) return
      this._markLoading(this.activeTabId)
      this.canGoForward = true
      try {
        await invoke<Tab | null>('go_back')
      } catch (e) {
        // 无历史可退属正常交互，静默忽略；其余错误记录
        console.error('[tabs] 后退失败:', e)
        if (this.loadingTabId === this.activeTabId) this.loadingTabId = null
      }
    },

    /** 前进（当前活动标签） */
    async goForward() {
      if (!this.activeTabId || !this.canGoForward) return
      this._markLoading(this.activeTabId)
      this.canGoBack = true
      try {
        await invoke<Tab | null>('go_forward')
      } catch (e) {
        console.error('[tabs] 前进失败:', e)
        if (this.loadingTabId === this.activeTabId) this.loadingTabId = null
      }
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
      let session: Session | null = null
      try {
        session = await invoke<Session | null>('load_last_session', { workspaceId })
      } catch (e) {
        // 容错：会话加载失败不向上抛（工作区切换流程依赖此方法不炸）
        console.error('[tabs] 加载会话失败:', e)
        return false
      }
      if (!session) return false
      return this.restoreSession(session)
    },

    /** 恢复指定会话快照（会话管理弹窗用，返回恢复的标签数） */
    async restoreSession(session: Session): Promise<boolean> {
      let tabs: SessionTab[] = []
      try {
        tabs = JSON.parse(session.data) as SessionTab[]
      } catch {
        return false
      }
      // 分组可能已被删除，恢复前确保分组列表已加载（校验 groupId 存在性）
      const groups = useGroupsStore()
      if (groups.groups.length === 0) {
        await groups.loadGroups().catch((e) => {
          console.error('[tabs] 恢复会话前加载分组失败:', e)
        })
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

    /** 保存为命名会话（会话管理弹窗用，返回新会话 id） */
    async saveNamedSession(name: string, workspaceId: number | null = null): Promise<number | null> {
      const tabs: SessionTab[] = this.tabs.map((t) => ({
        url: t.url,
        title: t.title,
        groupId: t.groupId ?? null,
      }))
      if (tabs.length === 0) return null
      try {
        return await invoke<number>('save_session', { name, workspaceId, tabs })
      } catch (e) {
        console.error('[tabs] 保存命名会话失败:', e)
        return null
      }
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

    /** 从 Rust 侧同步标签状态（事件驱动 + 低频兜底调用） */
    async syncTabs() {
      try {
        const my = this._opSeq
        const tabs = await invoke<Tab[]>('get_tabs')
        // 保留前端运行时 groupId（Rust 侧无此字段）
        const prevGroups = new Map(this.tabs.map((t) => [t.id, t.groupId]))
        const prevUrls = new Map(this.tabs.map((t) => [t.id, t.url]))
        const prevTitles = new Map(this.tabs.map((t) => [t.id, t.title]))
        this.tabs = tabs.map((t) => ({ ...t, groupId: prevGroups.get(t.id) ?? null }))
        // 页面加载完成（URL/标题变化）→ 关掉加载指示；webview 内链跳视为可后退
        if (this.loadingTabId) {
          const lid = this.loadingTabId
          const t = tabs.find((x) => x.id === lid)
          if (
            t &&
            (t.url !== prevUrls.get(lid) || t.title !== prevTitles.get(lid))
          ) {
            this.loadingTabId = null
            this.canGoBack = true
          }
        }
        // activeTabId 只在同步期间无用户操作时回写（_opSeq 未变），
        // 避免同步的 is_active 覆盖用户刚点的 switchTab/closeTab 结果
        const active = tabs.find((t) => t.is_active)
        if (active && my === this._opSeq) this.activeTabId = active.id
        // WebKit 原生历史栈状态（覆盖乐观追踪：内链跳转/JS history 也正确反映）
        try {
          const nav = await invoke<{ canGoBack: boolean; canGoForward: boolean }>(
            'get_nav_state'
          )
          this.canGoBack = nav.canGoBack
          this.canGoForward = nav.canGoForward
        } catch (e) {
          console.error('[tabs] 同步导航状态失败:', e)
        }
      } catch (e) {
        console.error('[tabs] 同步标签失败:', e)
      }
    },

    /**
     * 启动标签状态同步
     * Rust 侧在页面加载完成/标题变化/新窗口建标签/favicon 更新时 emit "tabs-updated"，
     * 前端监听事件即时同步（替代原 2s 轮询，查询开销约降 75%+）。
     * 兜底轮询（8s，仅窗口可见时跑）：覆盖 WebKit 无事件信号的场景（如 JS history.pushState
     * 改历史栈、SPA 内路由切换），把 get_nav_state（需 GTK 主线程）的开销从每 2s 降到每 8s。
     */
    startPolling(fallbackInterval = 8000) {
      if (this.pollTimer) return
      listen('tabs-updated', () => {
        this.syncTabs()
      })
        .then((un) => {
          this._unlisten = un
        })
        .catch((e) => {
          console.error('[tabs] 监听 tabs-updated 失败（退回兜底轮询）:', e)
        })
      this.pollTimer = window.setInterval(() => {
        // 窗口不可见时跳过（省 CPU/GTK 主线程调用；恢复可见后事件或下轮会补上）
        if (document.hidden) return
        this.syncTabs()
      }, fallbackInterval)
    },

    /** 停止同步（组件卸载时调用） */
    stopPolling() {
      if (this.pollTimer) {
        clearInterval(this.pollTimer)
        this.pollTimer = null
      }
      if (this._unlisten) {
        this._unlisten()
        this._unlisten = null
      }
    },
  },
})

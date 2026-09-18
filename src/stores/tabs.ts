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

// ---- 启动锁屏状态（避免锁屏期间反复弹"创建/加载标签页失败"）----
// Rust setup 弹 GTK 密码对话框期间 Db/TabManager 未就绪，标签命令必然失败。
// App.vue 初始化前查到 is_locked 后写入缓存，标签命令失败时据此静默。
let lockedCache: boolean | null = null
export function setLockedState(v: boolean): void {
  lockedCache = v
}
async function isLockedNow(): Promise<boolean> {
  if (lockedCache !== null) return lockedCache
  try {
    lockedCache = await invoke<boolean>('is_locked')
  } catch {
    lockedCache = false
  }
  return lockedCache
}

// ---- 自动分组（按域名归类，替代手动建分组）----
// 分组色板（循环取色）
const GROUP_COLORS = [
  '#4a9eff',
  '#f06292',
  '#ffb74d',
  '#66bb6a',
  '#ab47bc',
  '#26c6da',
  '#ec407a',
  '#7e57c2',
  '#8d6e63',
  '#9ccc65',
]
/** 取 URL 注册域名（去 www 前缀）；非 http(s) 返回 null */
function extractDomain(url: string): string | null {
  try {
    const u = new URL(url)
    if (u.protocol !== 'http:' && u.protocol !== 'https:') return null
    let host = u.hostname.toLowerCase()
    if (host.startsWith('www.')) host = host.slice(4)
    return host || null
  } catch {
    return null
  }
}
/** 用户手动取消分组的标签：会话内豁免，不再被自动归组拉回去 */
const autoGroupExempt = new Set<string>()
export function markAutoGroupExempt(tabId: string) {
  autoGroupExempt.add(tabId)
}

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
        // 锁屏期间命令必然失败，静默等待解锁（否则反复弹 toast）
        if (!(await isLockedNow())) useToastStore().error('加载标签页失败')
      }
    },

    /** 分配标签到分组（groupId = null 表示取消分组） */
    assignTabToGroup(tabId: string, groupId: number | null) {
      const tab = this.tabs.find((t) => t.id === tabId)
      if (tab) tab.groupId = groupId
    },

    /** 用户手动取消分组（右键"取消分组"）：会话内豁免，自动分组不再拉回 */
    exemptFromAutoGroup(tabId: string) {
      markAutoGroupExempt(tabId)
    },

    /**
     * 自动按域名归组：
     * - 内部页面/非 http(s) 跳过
     * - 已有该域名分组 → 直接归入
     * - 无分组时：同域名标签 ≥2 个才懒建组（避免一大堆单标签组占满标签栏）
     * - 已手动归组/被豁免的标签不动
     */
    async autoGroupTab(tabId: string, url: string) {
      if (autoGroupExempt.has(tabId)) return
      const tab = this.tabs.find((t) => t.id === tabId)
      if (!tab || tab.groupId != null) return
      const domain = extractDomain(url)
      if (!domain) return
      const groups = useGroupsStore()
      if (groups.groups.length === 0) {
        await groups.loadGroups().catch(() => {})
      }
      // 已有该域名分组 → 直接归入
      const existing = groups.groups.find((g) => g.name === domain)
      if (existing) {
        tab.groupId = existing.id
        return
      }
      // 无分组：同域名标签 ≥2 个才建组
      const same = this.tabs.filter(
        (t) => t.url && extractDomain(t.url) === domain,
      )
      if (same.length >= 2) {
        try {
          // isAuto=true：按域名自动归类产生的组**不落库**，
          // 仅存在于内存，标签全部关闭后由 pruneAutoGroups 丢弃
          const id = await groups.createGroup(
            domain,
            GROUP_COLORS[groups.groups.length % GROUP_COLORS.length],
            true,
          )
          const g = groups.groups.find((x) => x.id === id)
          if (g) {
            for (const t of same) {
              if (!autoGroupExempt.has(t.id)) t.groupId = g.id
            }
          }
        } catch {
          // groups store 已 toast 提示，静默
        }
      }
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
        // 自动按域名归组（fire-and-forget，不阻塞创建流程）
        this.autoGroupTab(tab.id, tab.url || '').catch(() => {})
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
        // 锁屏期间命令必然失败，静默等待解锁（否则反复弹 toast）
        if (!(await isLockedNow())) useToastStore().error('创建标签页失败')
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
        // 关闭后清理"已无标签使用"的自动分组：自动组不落库，标签走完即丢弃
        // （用户手动创建的分组 isAuto=false，不受影响）
        useGroupsStore().pruneAutoGroups(
          new Set(
            this.tabs
              .map((t) => t.groupId)
              .filter((g): g is number => g != null),
          ),
        )
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
        // 自动按域名归组：URL 变化/新标签时触发（fire-and-forget）
        for (const t of this.tabs) {
          if (t.url && t.url !== prevUrls.get(t.id)) {
            this.autoGroupTab(t.id, t.url).catch(() => {})
          }
        }
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
      // 空闲标签休眠检查的轮次计数（每 8 轮 ≈64s 触发一次）
      let hibernateTick = 0
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
        // 每 8 轮（≈64s）触发一次"空闲标签休眠"检查：销毁久未激活标签的 webview
        // 回收渲染进程内存（WebKitGTK 默认不主动回收，多标签内存线性增长）。
        // 阈值由 settings.tab_hibernate_minutes 控制（默认 30 分钟，0=关闭）
        if (++hibernateTick >= 8) {
          hibernateTick = 0
          invoke('hibernate_idle_tabs').catch((e) => {
            console.error('[tabs] 空闲标签休眠检查失败:', e)
          })
        }
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

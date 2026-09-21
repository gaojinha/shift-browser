// 工作区状态管理（Pinia）
// 每个工作区有独立的标签页集合；切换工作区 = 保存当前标签 → 清空 → 恢复目标工作区标签
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useTabsStore } from './tabs'
import { useToastStore } from './toast'

export interface Workspace {
  id: number
  name: string
  /** 空间主题色（阶段 3.2）；空字符串 = 跟随默认强调色 */
  color: string
  sort_order: number
  created_at: string
}

/** 新建空间时按序分配的调色板 */
export const SPACE_COLORS = [
  '#4a9eff',
  '#43a047',
  '#e53935',
  '#fb8c00',
  '#8e24aa',
  '#00acc1',
  '#f06292',
  '#5e35b1',
]

interface WorkspacesState {
  workspaces: Workspace[]
  /** 当前工作区 id（null = 默认工作区） */
  currentId: number | null
  /** 是否正在切换工作区（防重复点击） */
  switching: boolean
}

export const useWorkspacesStore = defineStore('workspaces', {
  state: (): WorkspacesState => ({
    workspaces: [],
    currentId: null,
    switching: false,
  }),

  getters: {
    current(state): Workspace | null {
      return state.workspaces.find((w) => w.id === state.currentId) ?? null
    },
    /** 当前工作区显示名（默认工作区显示"默认"） */
    currentName(state): string {
      return state.currentId === null
        ? '默认'
        : (state.workspaces.find((w) => w.id === state.currentId)?.name ?? '默认')
    },
    /** 当前空间主题色（空字符串 = 跟随默认强调色） */
    currentColor(state): string {
      return state.workspaces.find((w) => w.id === state.currentId)?.color ?? ''
    },
  },

  actions: {
    /** 启动时加载工作区列表 */
    async loadWorkspaces() {
      try {
        this.workspaces = await invoke<Workspace[]>('get_workspaces')
      } catch (e) {
        console.error('[workspaces] 加载工作区失败:', e)
      }
    },

    /** 新建空间（自动命名"空间 N" + 分配调色板主题色） */
    async createWorkspace(): Promise<number | null> {
      try {
        const n = this.workspaces.length + 1
        const id = await invoke<number>('create_workspace', { name: `空间 ${n}` })
        // 分配主题色（按序号取调色板）
        const color = SPACE_COLORS[(n - 1) % SPACE_COLORS.length]
        await invoke('update_workspace', { id, name: null, sort_order: null, color })
        await this.loadWorkspaces()
        return id
      } catch (e) {
        console.error('[workspaces] 创建空间失败:', e)
        useToastStore().error('创建空间失败')
        return null
      }
    },

    /** 重命名空间 */
    async renameWorkspace(id: number, name: string) {
      try {
        await invoke('update_workspace', { id, name, sort_order: null, color: null })
        await this.loadWorkspaces()
      } catch (e) {
        console.error('[workspaces] 重命名空间失败:', e)
        useToastStore().error('重命名空间失败')
      }
    },

    /** 设置空间主题色 */
    async setColor(id: number, color: string) {
      try {
        await invoke('update_workspace', { id, name: null, sort_order: null, color })
        await this.loadWorkspaces()
      } catch (e) {
        console.error('[workspaces] 设置空间色失败:', e)
      }
    },

    /** 删除工作区（删除后回默认工作区） */
    async deleteWorkspace(id: number) {
      try {
        await invoke('delete_workspace', { id })
        // 删除成功后，若删的是当前工作区 → 保存当前标签到默认工作区，清空标签，恢复默认会话
        if (this.currentId === id) {
          const tabs = useTabsStore()
          await tabs.saveCurrentSession(null)
          await tabs.clearAllTabs()
          this.currentId = null
          await tabs.restoreLastSession(null)
        }
        await this.loadWorkspaces()
      } catch (e) {
        console.error('[workspaces] 删除工作区失败:', e)
        useToastStore().error('删除工作区失败')
      }
    },

    /**
     * 切换到目标工作区
     * 流程：保存当前标签到当前工作区 → 清空全部标签 → 恢复目标工作区标签
     * currentId 在恢复完成后才提交（restoreLastSession 内部已容错），
     * 失败时回滚到原工作区，避免空快照覆盖目标工作区会话
     */
    async switchWorkspace(id: number | null) {
      if (this.switching || id === this.currentId) return
      this.switching = true
      const prev = this.currentId
      try {
        const tabs = useTabsStore()
        // 1. 保存当前标签到当前工作区（内部容错，失败不阻断）
        await tabs.saveCurrentSession(this.currentId)
        // 2. 清空全部标签
        await tabs.clearAllTabs()
        // 3. 恢复目标工作区标签（内部容错）
        await tabs.restoreLastSession(id)
        // 4. 恢复完成后才提交 currentId
        this.currentId = id
      } catch (e) {
        console.error('[workspaces] 切换工作区失败:', e)
        // 回滚：清空可能恢复一半的标签，回到原工作区
        const tabs = useTabsStore()
        await tabs.clearAllTabs().catch((e) => {
          console.error('[workspaces] 回滚清空标签失败:', e)
        })
        await tabs.restoreLastSession(prev).catch((e) => {
          console.error('[workspaces] 回滚恢复会话失败:', e)
        })
        this.currentId = prev
        useToastStore().error('切换工作区失败')
      } finally {
        this.switching = false
      }
    },
  },
})

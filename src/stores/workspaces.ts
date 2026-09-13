// 工作区状态管理（Pinia）
// 每个工作区有独立的标签页集合；切换工作区 = 保存当前标签 → 清空 → 恢复目标工作区标签
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useTabsStore } from './tabs'

export interface Workspace {
  id: number
  name: string
  sort_order: number
  created_at: string
}

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
      return state.currentId === null ? '默认' : (state.workspaces.find((w) => w.id === state.currentId)?.name ?? '默认')
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

    /** 新建工作区（自动命名"工作区 N"） */
    async createWorkspace(): Promise<number | null> {
      try {
        const n = this.workspaces.length + 1
        const id = await invoke<number>('create_workspace', { name: `工作区 ${n}` })
        await this.loadWorkspaces()
        return id
      } catch (e) {
        console.error('[workspaces] 创建工作区失败:', e)
        return null
      }
    },

    /** 重命名工作区 */
    async renameWorkspace(id: number, name: string) {
      try {
        await invoke('update_workspace', { id, name })
        await this.loadWorkspaces()
      } catch (e) {
        console.error('[workspaces] 重命名工作区失败:', e)
      }
    },

    /** 删除工作区（删除后回默认工作区） */
    async deleteWorkspace(id: number) {
      try {
        await invoke('delete_workspace', { id })
        if (this.currentId === id) {
          // 当前工作区被删 → 保存当前标签到默认工作区，清空标签，恢复默认工作区会话
          const tabs = useTabsStore()
          await tabs.saveCurrentSession(null)
          await tabs.clearAllTabs()
          this.currentId = null
          await tabs.restoreLastSession(null)
        }
        await this.loadWorkspaces()
      } catch (e) {
        console.error('[workspaces] 删除工作区失败:', e)
      }
    },

    /**
     * 切换到目标工作区
     * 流程：保存当前标签到当前工作区 → 清空全部标签 → 恢复目标工作区标签
     */
    async switchWorkspace(id: number | null) {
      if (this.switching || id === this.currentId) return
      this.switching = true
      try {
        const tabs = useTabsStore()
        // 1. 保存当前标签到当前工作区
        await tabs.saveCurrentSession(this.currentId)
        // 2. 清空全部标签
        await tabs.clearAllTabs()
        // 3. 恢复目标工作区标签
        this.currentId = id
        await tabs.restoreLastSession(id)
      } catch (e) {
        console.error('[workspaces] 切换工作区失败:', e)
      } finally {
        this.switching = false
      }
    },
  },
})
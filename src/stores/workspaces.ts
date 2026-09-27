// 工作区状态管理（Pinia）
// 每个工作区有独立的标签页集合；切换工作区 = 保存当前标签 → 清空 → 恢复目标工作区标签
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useTabsStore } from './tabs'
import { useToastStore } from './toast'
// 空间配色池（单一来源）
import { UI } from '../constants'

export interface Workspace {
  id: number
  name: string
  /** 空间主题色（阶段 3.2）；空字符串 = 跟随默认强调色 */
  color: string
  sort_order: number
  created_at: string
}

/** 新建空间时按序分配的调色板——来自 constants.ts（唯一来源） */

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
        const color = UI.spaceColors[(n - 1) % UI.spaceColors.length]
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
          const saved = await tabs.saveCurrentSession(null)
          if (!saved) {
            useToastStore().error('保存标签失败，已中止删除后的切换')
            await this.loadWorkspaces()
            return
          }
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
     * 安全顺序：保存失败（有标签但写库失败）直接中止，不清空；清空后恢复失败用内存
     * 快照重建，再不行回滚原工作区。currentId 在恢复完成后才提交。
     */
    async switchWorkspace(id: number | null) {
      if (this.switching || id === this.currentId) return
      this.switching = true
      const prev = this.currentId
      try {
        const tabs = useTabsStore()
        // 1. 保存当前标签到当前工作区：失败直接中止（旧标签还在内存，不丢）
        const saved = await tabs.saveCurrentSession(this.currentId)
        if (!saved) {
          useToastStore().error('保存当前工作区失败，已中止切换')
          return
        }
        // 2. 清空全部标签（返回内存快照供回滚）
        const snapshot = await tabs.clearAllTabs()
        // 3. 恢复目标工作区标签
        const ok = await tabs.restoreLastSession(id)
        if (!ok) {
          // 目标无会话或恢复失败：用快照重建当前页，不算切换失败（新工作区本就空的常见）
          // 快照为空（原工作区本无标签）则无需重建
          if (snapshot.length > 0) {
            let rebuilt = 0
            for (const t of snapshot) {
              if (t.url) {
                const tab = await tabs.createTab(t.url, undefined, true).catch(() => null)
                if (tab) {
                  if (t.groupId != null) tabs.assignTabToGroup(tab.id, t.groupId)
                  rebuilt++
                }
              }
            }
            // 快照一个没重建出来且目标也空：回滚原工作区
            if (rebuilt === 0) {
              await tabs.restoreLastSession(prev).catch((e) => {
                console.error('[workspaces] 回滚恢复会话失败:', e)
              })
              this.currentId = prev
              useToastStore().error('切换工作区失败，已回滚')
              return
            }
            // 快照重建成功 = 目标工作区是空的，留在目标（标签是原样的，不丢）
          }
        }
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

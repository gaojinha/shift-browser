// 标签分组状态管理（Pinia）
// 对应 Rust db/tab_groups.rs 的命令；标签归组是前端运行时概念（tab.groupId）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from './toast'

export interface TabGroup {
  id: number
  name: string
  color: string
  collapsed: boolean
  created_at: string
}

export const useGroupsStore = defineStore('groups', {
  state: () => ({
    groups: [] as TabGroup[],
  }),

  actions: {
    /** 拉取全部分组 */
    async loadGroups() {
      this.groups = await invoke<TabGroup[]>('get_tab_groups')
    },

    /** 创建分组，返回新分组 id */
    async createGroup(name: string, color = '#4a9eff'): Promise<number> {
      try {
        const id = await invoke<number>('create_tab_group', { name, color })
        await this.loadGroups()
        return id
      } catch (e) {
        console.error('[groups] 创建分组失败:', e)
        useToastStore().error('创建分组失败')
        throw e
      }
    },

    /** 更新分组（名字/颜色/折叠） */
    async updateGroup(id: number, patch: { name?: string; color?: string; collapsed?: boolean }) {
      try {
        await invoke('update_tab_group', { id, ...patch })
        await this.loadGroups()
      } catch (e) {
        console.error('[groups] 更新分组失败:', e)
        useToastStore().error('更新分组失败')
      }
    },

    /** 删除分组 */
    async deleteGroup(id: number) {
      try {
        await invoke('delete_tab_group', { id })
        await this.loadGroups()
      } catch (e) {
        console.error('[groups] 删除分组失败:', e)
        useToastStore().error('删除分组失败')
      }
    },
  },
})

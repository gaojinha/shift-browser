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
  /** 自动按域名生成的临时组：不落库，标签全部关闭后丢弃（用户手动建的不受影响） */
  isAuto?: boolean
}

export const useGroupsStore = defineStore('groups', {
  state: () => ({
    groups: [] as TabGroup[],
    loading: false,
  }),

  actions: {
    /** 拉取全部分组（保留内存中的自动分组——它们不来自数据库） */
    async loadGroups() {
      this.loading = true
      try {
        const dbGroups = await invoke<TabGroup[]>('get_tab_groups')
        // 自动分组只存在于前端内存，不能被数据库结果冲掉
        const autoGroups = this.groups.filter((g) => g.isAuto)
        this.groups = [...dbGroups, ...autoGroups]
      } finally {
        this.loading = false
      }
    },

    /** 创建分组，返回新分组 id。
     *  isAuto=true 时**不落库**：按域名自动归类产生的组是临时组，
     *  只在内存里存在，其标签全部关闭后由 pruneAutoGroups 丢弃；
     *  用户手动创建的分组仍走数据库持久化。 */
    async createGroup(name: string, color = '#4a9eff', isAuto = false): Promise<number> {
      if (isAuto) {
        // 负 id 避免与数据库自增 id 冲突
        const id = -Date.now()
        this.groups.push({
          id,
          name,
          color,
          collapsed: false,
          created_at: new Date().toISOString(),
          isAuto: true,
        })
        return id
      }
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

    /** 丢弃已无标签使用的自动分组（标签关闭后调用）。手动分组不受影响。 */
    pruneAutoGroups(usedGroupIds: Set<number>) {
      const before = this.groups.length
      this.groups = this.groups.filter((g) => !g.isAuto || usedGroupIds.has(g.id))
      const removed = before - this.groups.length
      if (removed > 0) {
        console.info('[groups] 已丢弃', removed, '个空的自动分组')
      }
    },

    /** 更新分组（名字/颜色/折叠），失败向上抛（调用方乐观更新需回滚） */
    async updateGroup(id: number, patch: { name?: string; color?: string; collapsed?: boolean }) {
      try {
        await invoke('update_tab_group', { id, ...patch })
        await this.loadGroups()
      } catch (e) {
        console.error('[groups] 更新分组失败:', e)
        useToastStore().error('更新分组失败')
        throw e
      }
    },

    /** 删除分组（失败向上抛，调用方决定是否继续清标签归属） */
    async deleteGroup(id: number) {
      try {
        await invoke('delete_tab_group', { id })
        await this.loadGroups()
      } catch (e) {
        console.error('[groups] 删除分组失败:', e)
        useToastStore().error('删除分组失败')
        throw e
      }
    },
  },
})

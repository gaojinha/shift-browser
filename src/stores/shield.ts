// 护盾状态（B-04 MVP）
// 职责：① 隐私统计"本页基线"归集（近似口径）② 站点降盾（兼容白名单）读写封装
// 说明：计数数据源为 Rust 内存统计（get_privacy_stats，重启归零）；
//       "本页"值 = 当前统计 - 进入本页时的基线快照（近似：并发场景下拦截归属不逐一区分，UI 标注）
import { defineStore } from 'pinia'
import { invoke } from '@tauri-apps/api/core'

export interface PrivacyStats {
  cleaned_params: number
  privacy_injections: number
  adblock_injections: number
}

interface ShieldState {
  stats: PrivacyStats | null
  /** 本页基线（进入当前页时的统计快照） */
  baseline: PrivacyStats | null
  /** 基线对应的页面 URL（用于检测页面切换） */
  baselineUrl: string
}

const zero: PrivacyStats = { cleaned_params: 0, privacy_injections: 0, adblock_injections: 0 }

export const useShieldStore = defineStore('shield', {
  state: (): ShieldState => ({ stats: null, baseline: null, baselineUrl: '' }),
  getters: {
    /** 本页增量（近似）：当前统计 - 基线；统计被重置时钳制非负 */
    pageDelta(state): PrivacyStats {
      if (!state.stats || !state.baseline) return zero
      const d = (a: number, b: number) => Math.max(0, a - b)
      return {
        adblock_injections: d(state.stats.adblock_injections, state.baseline.adblock_injections),
        cleaned_params: d(state.stats.cleaned_params, state.baseline.cleaned_params),
        privacy_injections: d(state.stats.privacy_injections, state.baseline.privacy_injections),
      }
    },
  },
  actions: {
    /** 拉取当前统计（失败返回 null，由 UI 显示"统计不可用"） */
    async loadStats(): Promise<PrivacyStats | null> {
      try {
        this.stats = await invoke<PrivacyStats>('get_privacy_stats')
      } catch {
        this.stats = null
      }
      return this.stats
    },
    /** 导航发生时调用（AddressBar watch）：立即拉一次统计并重建基线（新页面计数口径起点） */
    async noteNavigation(url: string) {
      await this.loadStats()
      this.baselineUrl = url
      this.baseline = this.stats ? { ...this.stats } : null
    },
    /** 兜底（面板打开时调用）：基线未建立或与当前 URL 不符时重置 */
    ensureBaseline(url: string) {
      if (this.baselineUrl !== url) {
        this.baselineUrl = url
        this.baseline = this.stats ? { ...this.stats } : null
      }
    },
    async getWhitelist(): Promise<string[]> {
      try {
        return await invoke<string[]>('get_privacy_whitelist')
      } catch {
        return []
      }
    },
    async setWhitelist(domains: string[]) {
      await invoke('set_privacy_whitelist', { domains })
    },
    /** 站点是否处于"降盾"（兼容白名单）状态 */
    async isSiteShieldOff(host: string): Promise<boolean> {
      if (!host) return false
      const list = await this.getWhitelist()
      return list.includes(host)
    },
    /** 降盾/恢复（host 精准匹配；恢复时保留其它白名单项） */
    async toggleSiteShield(host: string): Promise<boolean> {
      const list = await this.getWhitelist()
      const exists = list.includes(host)
      const next = exists ? list.filter((d) => d !== host) : [...list, host]
      await this.setWhitelist(next)
      // 返回操作后的"防护中"状态：操作前处于降盾（exists）则为本次恢复 → true
      return exists
    },
  },
})

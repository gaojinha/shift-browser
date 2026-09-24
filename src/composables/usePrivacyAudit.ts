import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useToastStore } from '../stores/toast'

/** 隐私统计 / 自测 / GPC 检测 / 外联审计（R2-1d 从 PrivacySection 抽出） */
export function usePrivacyAudit() {
  const privacyStats = ref<{
    cleaned_params: number
    privacy_injections: number
    adblock_injections: number
  } | null>(null)
  async function loadPrivacyStats() {
    try {
      privacyStats.value = await invoke<{
        cleaned_params: number
        privacy_injections: number
        adblock_injections: number
      }>('get_privacy_stats')
    } catch (e) {
      console.error('[settings] 读取隐私统计失败:', e)
    }
  }
  async function resetPrivacyStats() {
    try {
      await invoke('reset_privacy_stats')
      await loadPrivacyStats()
      useToastStore().success('统计已重置')
    } catch (e) {
      console.error('[settings] 重置统计失败:', e)
      useToastStore().error('重置统计失败')
    }
  }

  /** 隐私防护自测（阶段 2：信任需要证明） */
  interface PrivacySelfCheck {
    id: string
    label: string
    status: string
    detail: string
  }
  interface PrivacySelfTestReport {
    shield_level: string
    fp_mode: string
    hostname: string
    whitelisted: boolean
    total: number
    passed: number
    checks: PrivacySelfCheck[]
  }
  const privacySelfTest = ref<PrivacySelfTestReport | null>(null)
  const privacySelfTestRunning = ref(false)
  async function runPrivacySelfTest() {
    privacySelfTestRunning.value = true
    try {
      const r = await invoke<PrivacySelfTestReport>('privacy_selftest')
      privacySelfTest.value = r
      if (r.passed === r.total) {
        useToastStore().success(`自测通过：${r.passed}/${r.total} 项防护已生效`)
      } else {
        useToastStore().show(`自测完成：${r.passed}/${r.total} 项通过`)
      }
    } catch (e) {
      console.error('[settings] 隐私自测失败:', e)
      useToastStore().error('自测失败：请先打开一个网页')
    } finally {
      privacySelfTestRunning.value = false
    }
  }

  /** GPC 站点声明检测（阶段 2：GPC 完整化） */
  interface GpcSiteStatus {
    origin: string
    url: string
    reachable: boolean
    declared: boolean | null
    last_update: string | null
    summary: string
    error: string | null
  }
  const gpcSite = ref<GpcSiteStatus | null>(null)
  const gpcChecking = ref(false)
  async function checkGpcSite() {
    gpcChecking.value = true
    try {
      gpcSite.value = await invoke<GpcSiteStatus>('gpc_site_check')
    } catch (e) {
      console.error('[settings] GPC 站点检测失败:', e)
      useToastStore().error('检测失败：请先打开一个网页')
    } finally {
      gpcChecking.value = false
    }
  }

  /** 零遥测审计（阶段 2：全量网络出口清单） */
  interface EgressConn {
    pid: number
    proto: string
    remote_ip: string
    remote_port: number
    state: string
  }
  interface EgressReport {
    processes: number
    connections: EgressConn[]
    distinct_remotes: number
    loopback: number
    external: number
    checked_at: number
    note: string
  }
  const egressReport = ref<EgressReport | null>(null)
  const egressRunning = ref(false)
  async function runEgressAudit() {
    egressRunning.value = true
    try {
      egressReport.value = await invoke<EgressReport>('egress_audit')
    } catch (e) {
      console.error('[settings] 外联审计失败:', e)
      useToastStore().error('外联审计失败')
    } finally {
      egressRunning.value = false
    }
  }

  /** 广告拦截开关：settings 表是唯一状态源，set_adblock_enabled 只负责把开关同步给注入器 */

  return { privacyStats, loadPrivacyStats, resetPrivacyStats, privacySelfTest, privacySelfTestRunning, runPrivacySelfTest, gpcSite, gpcChecking, checkGpcSite, egressReport, egressRunning, runEgressAudit }
}

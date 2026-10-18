// usePrivacyAudit 单测（F-74）：统计加载与重置 / 自测通过与部分通过 / GPC 与外联审计的 running 复位
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { usePrivacyAudit } from './usePrivacyAudit'
import { useToastStore } from '../stores/toast'

const STATS = { cleaned_params: 1, privacy_injections: 2, adblock_injections: 3 }
const report = (passed: number, total: number) => ({
  shield_level: 'standard',
  fp_mode: 'noise',
  hostname: 'a.com',
  whitelisted: false,
  total,
  passed,
  checks: [],
})
const toasts = () => useToastStore().items.map((t) => `${t.type}:${t.message}`)

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

describe('usePrivacyAudit', () => {
  it('loadPrivacyStats: 成功填充；失败保持 null', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await a.loadPrivacyStats()
    expect(a.privacyStats.value).toBeNull()
    invokeMock.mockResolvedValueOnce(STATS)
    await a.loadPrivacyStats()
    expect(a.privacyStats.value).toEqual(STATS)
  })

  it('resetPrivacyStats: 先重置再重载；失败 toast 且不重载', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockResolvedValueOnce(undefined).mockResolvedValueOnce(STATS)
    await a.resetPrivacyStats()
    expect(invokeMock).toHaveBeenNthCalledWith(1, 'reset_privacy_stats')
    expect(invokeMock).toHaveBeenNthCalledWith(2, 'get_privacy_stats')
    expect(toasts()).toContain('success:统计已重置')
    invokeMock.mockReset()
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await a.resetPrivacyStats()
    expect(invokeMock).toHaveBeenCalledTimes(1)
    expect(toasts()).toContain('error:重置统计失败')
  })

  it('runPrivacySelfTest: 全通过为 success，部分通过为 info，running 复位', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockResolvedValueOnce(report(5, 5))
    await a.runPrivacySelfTest()
    expect(toasts()).toContain('success:自测通过：5/5 项防护已生效')
    invokeMock.mockResolvedValueOnce(report(3, 5))
    await a.runPrivacySelfTest()
    expect(toasts()).toContain('info:自测完成：3/5 项通过')
    expect(a.privacySelfTest.value?.passed).toBe(3)
    expect(a.privacySelfTestRunning.value).toBe(false)
  })

  it('runPrivacySelfTest 失败: 提示先打开网页，保留上次报告，running 复位', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockResolvedValueOnce(report(1, 1))
    await a.runPrivacySelfTest()
    invokeMock.mockRejectedValueOnce(new Error('no tab'))
    await a.runPrivacySelfTest()
    expect(toasts()).toContain('error:自测失败：请先打开一个网页')
    expect(a.privacySelfTest.value).not.toBeNull()
    expect(a.privacySelfTestRunning.value).toBe(false)
  })

  it('checkGpcSite: 成功写入结果；失败 toast，checking 复位', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockResolvedValueOnce({ origin: 'https://a.com', reachable: true, declared: true })
    await a.checkGpcSite()
    expect(a.gpcSite.value?.declared).toBe(true)
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await a.checkGpcSite()
    expect(toasts()).toContain('error:检测失败：请先打开一个网页')
    expect(a.gpcChecking.value).toBe(false)
  })

  it('runEgressAudit: 成功写入报告；失败 toast，running 复位', async () => {
    const a = usePrivacyAudit()
    invokeMock.mockResolvedValueOnce({ processes: 2, connections: [], external: 0 })
    await a.runEgressAudit()
    expect(a.egressReport.value?.processes).toBe(2)
    invokeMock.mockRejectedValueOnce(new Error('x'))
    await a.runEgressAudit()
    expect(toasts()).toContain('error:外联审计失败')
    expect(a.egressRunning.value).toBe(false)
  })
})

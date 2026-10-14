// i18n 防漂移测试（R4-5 基础框架 · 2026-10-14）
//
// 锁两件事：
// 1. en 与 zh-CN 键集完全一致（加键忘翻译立即可见，fallback 不成摆设）
// 2. 样板组件（TranslateProgress）文案确实走 t()：locale 切换后文案跟着变
//
// 跑法：npx vitest run src/i18n/i18n.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest'
import { nextTick } from 'vue'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

import zhCN from './locales/zh-CN'
import en from './locales/en'
import { i18n, setLocale } from './index'

type Node = Record<string, unknown>

/** 收集叶子键路径（a.b.c），数组忽略 */
function leafPaths(obj: Node, prefix = ''): string[] {
  return Object.entries(obj).flatMap(([k, v]) => {
    const p = prefix ? `${prefix}.${k}` : k
    if (v && typeof v === 'object') return leafPaths(v as Node, p)
    return [p]
  })
}

describe('i18n 语言包键集一致', () => {
  it('en 与 zh-CN 键路径完全一致（双向无差集）', () => {
    const zh = leafPaths(zhCN).sort()
    const enKeys = leafPaths(en).sort()
    expect(enKeys).toEqual(zh)
  })

  it('每个叶子值都是非空字符串', () => {
    for (const [locale, pack] of [
      ['zh-CN', zhCN],
      ['en', en],
    ] as const) {
      for (const p of leafPaths(pack as Node)) {
        const v = p
          .split('.')
          .reduce<unknown>((o, k) => (o as Node)[k], pack as Node)
        expect(typeof v, `${locale}:${p}`).toBe('string')
        expect(v as string, `${locale}:${p}`).not.toBe('')
      }
    }
  })
})

describe('i18n 样板：翻译进度条文案', () => {
  it('默认 zh-CN：插值渲染与原文案逐字一致', () => {
    expect(i18n.global.t('progress.preparing')).toBe('准备中…')
    expect(i18n.global.t('progress.done', { s: 12 })).toBe('完成 · 12s')
    expect(i18n.global.t('progress.runningEta', { s: 5, eta: 20 })).toBe('5s · 剩20s')
    // update 节（T-009 toast 文案并入 i18n · 收尾批）
    expect(
      i18n.global.t('update.available', { latest: '5.9.0', current: '5.8.3' }),
    ).toBe('发现新版本 v5.9.0（当前 v5.8.3），到「关于」页或 Release 页获取')
  })

  it('切 en 后插值文案跟着变；回切 zh-CN 无残留', async () => {
    setLocale('en')
    await nextTick()
    expect(i18n.global.t('progress.done', { s: 12 })).toBe('Done · 12s')
    setLocale('zh-CN')
    await nextTick()
    expect(i18n.global.t('progress.done', { s: 12 })).toBe('完成 · 12s')
  })
})

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

// ─── 多文件/多节校验（R4-5 拆包 · 022 扩展）──────────────────────────────
// 兼容 D-5 任意拆法：
//   - 单文件模式（现状）：locales/zh-CN.ts + locales/en.ts
//   - 多文件平铺：locales/zh-CN.navigation.ts + locales/en.navigation.ts
//   - 子目录模式：locales/zh-CN/navigation.ts + locales/en/navigation.ts
// 规则：同名节文件（去 locale 前缀后）在 zh-CN 和 en 两侧必须同时存在，
//       且键路径完全一致。
//
// 当前 locales/ 只有 zh-CN.ts / en.ts，此测试块自动发现 0 个节文件，
// 不报错；拆包后新增文件自动纳入校验。

describe('i18n 多文件节键集一致（多节扩展）', () => {
  // import.meta.glob 在 vitest 中静态分析，编译期发现所有 .ts 文件
  const sectionFiles = import.meta.glob<{ default: Node }>(
    './locales/!(zh-CN|en).ts',
    { eager: true },
  )

  it('节文件数量为零时跳过（单文件模式）', () => {
    const count = Object.keys(sectionFiles).length
    if (count === 0) return // 单文件模式，无节文件，通过
  })

  it('每对 zh-CN/en 节文件键路径一致', () => {
    // 按节名分组：zh-CN.navigation.ts → navigation, en.navigation.ts → navigation
    const bySection: Record<string, { zh?: Node; en?: Node; zhName?: string; enName?: string }> = {}

    for (const [path, mod] of Object.entries(sectionFiles)) {
      // path 形如 './locales/zh-CN.navigation.ts' 或 './locales/zh-CN/navigation.ts'
      const fileName = path.split('/').pop()!.replace(/\.ts$/, '')
      const [locale, section] = fileName.split('.', 2)
      if (!locale || !section) continue
      const loc = locale === 'zh-CN' ? 'zh' : locale === 'en' ? 'en' : null
      if (!loc) continue
      if (!bySection[section]) bySection[section] = {}
      bySection[section][loc] = (mod as { default: Node }).default
      bySection[section][`${loc}Name`] = fileName
    }

    for (const [section, pair] of Object.entries(bySection)) {
      if (pair.zh && pair.en) {
        const zhKeys = leafPaths(pair.zh).sort()
        const enKeys = leafPaths(pair.en).sort()
        expect(enKeys, `节 ${section}（${pair.zhName} vs ${pair.enName}）`).toEqual(zhKeys)
      } else {
        // 一侧有、另一侧没有 → 不匹配
        expect(
          pair.zh || pair.en,
          `节 ${section}：${pair.zhName ? 'zh-CN 存在' : ''} ${pair.enName ? 'en 存在' : ''}（必须成对出现）`,
        ).toBeUndefined()
      }
    }
  })

  it('节文件叶子值均为非空字符串', () => {
    for (const [path, mod] of Object.entries(sectionFiles)) {
      const pack = (mod as { default: Node }).default
      const label = path.split('/').pop()
      for (const p of leafPaths(pack)) {
        const v = p.split('.').reduce<unknown>((o, k) => (o as Node)[k], pack)
        expect(typeof v, `${label}:${p}`).toBe('string')
        expect(v as string, `${label}:${p}`).not.toBe('')
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

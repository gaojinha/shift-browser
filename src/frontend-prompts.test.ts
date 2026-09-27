// frontend-prompts 单测：快捷句口径 / 八码话术 / 人话兜底
import { describe, it, expect } from 'vitest'
import {
  shortcutSummarize,
  shortcutExplain,
  ERROR_COPY,
  humanizeError,
  EMPTY_GUIDE,
  TRANSLATE_PANEL_HINT,
  CONTEXT_BUDGET_CHARS,
  HISTORY_KEPT,
} from './frontend-prompts'

describe('快捷句口径（与后端模板配对，改文案需同步改 ai.test.ts 断言）', () => {
  it('总结句含 5 条以内', () => {
    expect(shortcutSummarize('页', 'https://x')).toContain('5 条以内')
  })
  it('解释句含 通俗易懂', () => {
    expect(shortcutExplain('页', 'https://x')).toContain('通俗易懂')
  })
  it('预算常量与后端同口径', () => {
    expect(CONTEXT_BUDGET_CHARS).toBe(8000)
    expect(HISTORY_KEPT).toBe(20)
  })
})

describe('八码人话', () => {
  it('八码全覆盖且非空', () => {
    const codes = [
      'E_DB',
      'E_PERM',
      'E_TIMEOUT',
      'E_AUTH',
      'E_RATELIMIT',
      'E_NET',
      'E_VALIDATION',
      'E_INTERNAL',
    ] as const
    for (const c of codes) expect(ERROR_COPY[c].length).toBeGreaterThan(0)
  })
  it('已知码：话术打底+原文附注', () => {
    const s = humanizeError({ code: 'E_NET', message: 'connect failed' })
    expect(s).toContain(ERROR_COPY.E_NET)
    expect(s).toContain('connect failed')
  })
  it('裸字符串：兜底内部话术+原文', () => {
    const s = humanizeError('炸了')
    expect(s).toContain(ERROR_COPY.E_INTERNAL)
    expect(s).toContain('炸了')
  })
  it('空原文：只有话术无括号', () => {
    expect(humanizeError({ code: 'E_AUTH', message: '' })).toBe(ERROR_COPY.E_AUTH)
  })
})

describe('话术表', () => {
  it('空状态四条齐', () => {
    expect(Object.keys(EMPTY_GUIDE).sort()).toEqual(['noResult', 'noTabs', 'notLogged', 'offline'])
  })
  it('翻译分工一句说清两条路', () => {
    expect(TRANSLATE_PANEL_HINT).toContain('侧栏')
  })
})

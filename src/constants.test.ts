// constants.ts 自洽性单测（F-75 前端测试补课）
// 本文件由 src-tauri/build.rs 从 constants.rs + contracts.rs 生成，
// 与后端的逐值一致性由生成器保证，这里测**自洽**：形状、格式、
// 唯一性、派生表（SEARCH_URLS/DEFAULT_SETTINGS）与源表 1:1。
import { describe, it, expect } from 'vitest'
import {
  LAYOUT,
  SEARCH_ENGINES,
  SEARCH_URLS,
  UI,
  SETTINGS_SCHEMA,
  DEFAULT_SETTINGS,
  EVENTS,
  URLS,
  TIMEOUT,
  SHORTCUTS,
  SKILLS,
  AI_BEHAVIOR_RULES,
} from './constants'

const HEX_COLOR = /^#[0-9a-f]{6}$/i

describe('LAYOUT', () => {
  it('所有几何常量是正数', () => {
    for (const [k, v] of Object.entries(LAYOUT)) {
      expect(Number.isFinite(v) && v > 0, `LAYOUT.${k} 应为正数`).toBe(true)
    }
  })
})

describe('SEARCH_ENGINES / SEARCH_URLS', () => {
  it('引擎 id 唯一、url 为 https 且以查询占位符结尾', () => {
    const ids = SEARCH_ENGINES.map((e) => e.id)
    expect(new Set(ids).size).toBe(ids.length)
    for (const e of SEARCH_ENGINES) {
      expect(e.label.length).toBeGreaterThan(0)
      expect(e.url.startsWith('https://')).toBe(true)
      expect(e.url.endsWith('='), `${e.id} url 应以 = 结尾（拼接查询串）`).toBe(true)
    }
  })

  it('SEARCH_URLS 与引擎表 1:1（无缺、无多）', () => {
    expect(Object.keys(SEARCH_URLS).sort()).toEqual(SEARCH_ENGINES.map((e) => e.id).sort())
    for (const e of SEARCH_ENGINES) expect(SEARCH_URLS[e.id]).toBe(e.url)
  })
})

describe('UI 调色板', () => {
  it('spaceColors 恰好 8 色、合法 hex、无重复（工作区按 (n-1)%8 取色依赖）', () => {
    expect(UI.spaceColors).toHaveLength(8)
    for (const c of UI.spaceColors) expect(HEX_COLOR.test(c), `非法色值: ${c}`).toBe(true)
    expect(new Set(UI.spaceColors).size).toBe(UI.spaceColors.length)
  })

  it('groupColors / groupPalette 全为合法 hex 或 var(--accent)，各自无重复', () => {
    for (const c of UI.groupColors) expect(HEX_COLOR.test(c), `非法色值: ${c}`).toBe(true)
    expect(new Set(UI.groupColors).size).toBe(UI.groupColors.length)
    for (const c of UI.groupPalette) {
      expect(c === 'var(--accent)' || HEX_COLOR.test(c), `非法色值: ${c}`).toBe(true)
    }
    expect(new Set(UI.groupPalette).size).toBe(UI.groupPalette.length)
  })

  it('defaultFavicon 是内联 SVG data URL', () => {
    expect(UI.defaultFavicon.startsWith('data:image/svg+xml')).toBe(true)
  })
})

describe('SETTINGS_SCHEMA / DEFAULT_SETTINGS', () => {
  it('kind 合法，bool 默认 true/false，int 默认可解析为整数', () => {
    for (const [k, v] of Object.entries(SETTINGS_SCHEMA)) {
      expect(['str', 'bool', 'int'], `SETTINGS_SCHEMA.${k} kind 非法: ${v.kind}`).toContain(v.kind)
      expect(typeof v.default, `SETTINGS_SCHEMA.${k} default 应为字符串`).toBe('string')
      if (v.kind === 'bool') {
        expect(['true', 'false'], `SETTINGS_SCHEMA.${k} bool 默认值非法: ${v.default}`).toContain(v.default)
      }
      if (v.kind === 'int') {
        const n = Number(v.default)
        expect(Number.isInteger(n), `SETTINGS_SCHEMA.${k} int 默认值非整数: ${v.default}`).toBe(true)
      }
    }
  })

  it('DEFAULT_SETTINGS 与 schema 1:1（同键同默认值，防生成漂移）', () => {
    expect(Object.keys(DEFAULT_SETTINGS).sort()).toEqual(Object.keys(SETTINGS_SCHEMA).sort())
    for (const [k, v] of Object.entries(SETTINGS_SCHEMA)) {
      expect(DEFAULT_SETTINGS[k], `DEFAULT_SETTINGS.${k} 与 schema 默认值不一致`).toBe(v.default)
    }
  })
})

describe('EVENTS', () => {
  it('事件名唯一、非空、小写短横线风格', () => {
    const vals = Object.values(EVENTS)
    expect(new Set(vals).size).toBe(vals.length)
    for (const v of vals) {
      expect(v.length).toBeGreaterThan(0)
      expect(/^[a-z][a-z0-9-]*$/.test(v), `事件名格式异常: ${v}`).toBe(true)
    }
  })
})

describe('URLS / TIMEOUT', () => {
  it('URLS 内部页协议为 shift://，标记串以斜杠包裹', () => {
    expect(URLS.NEWTAB).toBe('shift://newtab')
    expect(URLS.WORLD_NAV).toBe('shift://worldnav')
    expect(URLS.NEWTAB_MARKER.startsWith('/') && URLS.NEWTAB_MARKER.endsWith('/')).toBe(true)
    expect(URLS.READER_MARKER.startsWith('/') && URLS.READER_MARKER.endsWith('/')).toBe(true)
  })

  it('TIMEOUT 所有超时是正数', () => {
    for (const [k, v] of Object.entries(TIMEOUT)) {
      expect(Number.isFinite(v) && v > 0, `TIMEOUT.${k} 应为正数`).toBe(true)
    }
  })
})

describe('SHORTCUTS', () => {
  it('快捷键组合唯一、格式为 键(修饰符)+键（小写，与 matchShortcut 约定一致）', () => {
    const vals = Object.values(SHORTCUTS)
    expect(new Set(vals).size).toBe(vals.length)
    for (const v of vals) {
      expect(/^[a-z0-9]+(\+[a-z0-9]+)*$/.test(v), `快捷键格式异常: ${v}`).toBe(true)
    }
  })
})

describe('SKILLS / AI_BEHAVIOR_RULES', () => {
  it('技能 id 唯一、名称与描述非空；AI 行为规则非空', () => {
    const ids = SKILLS.map((s) => s.id)
    expect(new Set(ids).size).toBe(ids.length)
    for (const s of SKILLS) {
      expect(s.name.length).toBeGreaterThan(0)
      expect(s.desc.length).toBeGreaterThan(0)
    }
    expect(AI_BEHAVIOR_RULES.length).toBeGreaterThan(0)
  })
})

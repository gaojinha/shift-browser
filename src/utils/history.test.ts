// groupHistoryByDate 单测（C-4 历史按日期分组）
import { describe, it, expect } from 'vitest'
import { groupHistoryByDate } from './history'
import type { HistoryEntry } from '../types/history'

function entry(id: number, iso: string): HistoryEntry {
  return { id, url: `https://a.com/${id}`, title: `T${id}`, visit_count: 1, last_visit_at: iso }
}

/** 相对今天偏移 dayOffset 天的本地时刻（默认中午 12:00，避开 0 点日期边界） */
function at(dayOffset: number, h = 12, m = 0): string {
  const d = new Date()
  d.setDate(d.getDate() + dayOffset)
  d.setHours(h, m, 0, 0)
  return d.toISOString()
}

describe('groupHistoryByDate', () => {
  it('按今天/昨天/更早分组，顺序固定', () => {
    const groups = groupHistoryByDate([entry(1, at(0)), entry(2, at(-1)), entry(3, at(-3))])
    expect(groups.map((g) => g.key)).toEqual(['today', 'yesterday', 'older'])
    expect(groups[0].entries.map((e) => e.id)).toEqual([1])
    expect(groups[1].entries.map((e) => e.id)).toEqual([2])
    expect(groups[2].entries.map((e) => e.id)).toEqual([3])
  })

  it('只有今天 → 仅产出今天分组（空档不出现）', () => {
    const groups = groupHistoryByDate([entry(1, at(0)), entry(2, at(0))])
    expect(groups.map((g) => g.key)).toEqual(['today'])
    expect(groups[0].entries).toHaveLength(2)
  })

  it('空输入 → 空数组', () => {
    expect(groupHistoryByDate([])).toEqual([])
  })

  it('无效时间戳归入更早', () => {
    const groups = groupHistoryByDate([entry(1, 'not-a-date')])
    expect(groups.map((g) => g.key)).toEqual(['older'])
  })

  it('分组头中文文案', () => {
    const groups = groupHistoryByDate([entry(1, at(0)), entry(2, at(-1)), entry(3, at(-9))])
    expect(groups.map((g) => g.label)).toEqual(['今天', '昨天', '更早'])
  })
})

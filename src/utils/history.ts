// 浏览历史日期分组（C-4 借鉴 Brave：按日期分组展示）
// 纯函数：输入 HistoryEntry[]（last_visit_at 为 ISO 时间戳），按本地日期输出「今天/昨天/更早」三档。
// 不新增后端调用——分组完全在前端从已有 last_visit_at 派生。
import type { HistoryEntry } from '../types/history'

export interface HistoryDateGroup {
  /** 分组键：今天 / 昨天 / 更早 */
  key: 'today' | 'yesterday' | 'older'
  /** 分组头文案 */
  label: string
  entries: HistoryEntry[]
}

/** 本地日期 key（YYYY-MM-DD） */
function dayKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
}

/**
 * 按日期分组：今天 / 昨天 / 更早。
 * 无效时间戳归入「更早」；空档不出现（该档无条目则不产出对应分组）。
 * 输出顺序固定：今天 → 昨天 → 更早。
 */
export function groupHistoryByDate(entries: HistoryEntry[]): HistoryDateGroup[] {
  const now = new Date()
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  const todayKey = dayKey(now)
  const yesterdayKey = dayKey(yesterday)

  const today: HistoryEntry[] = []
  const yester: HistoryEntry[] = []
  const older: HistoryEntry[] = []

  for (const e of entries) {
    const d = new Date(e.last_visit_at)
    if (isNaN(d.getTime())) {
      older.push(e)
      continue
    }
    const k = dayKey(d)
    if (k === todayKey) today.push(e)
    else if (k === yesterdayKey) yester.push(e)
    else older.push(e)
  }

  const out: HistoryDateGroup[] = []
  if (today.length) out.push({ key: 'today', label: '今天', entries: today })
  if (yester.length) out.push({ key: 'yesterday', label: '昨天', entries: yester })
  if (older.length) out.push({ key: 'older', label: '更早', entries: older })
  return out
}

// useConfirmDelete 单测：二次点击语义 + 超时撤防 + 切换撤防
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { useConfirmDelete } from './useConfirmDelete'

describe('useConfirmDelete', () => {
  beforeEach(() => {
    vi.useFakeTimers()
  })
  afterEach(() => {
    vi.useRealTimers()
  })

  it('第一次返回 false（待确认），第二次返回 true（执行）', () => {
    const c = useConfirmDelete()
    expect(c.arm(1)).toBe(false)
    expect(c.isPending(1)).toBe(true)
    expect(c.arm(1)).toBe(true)
    expect(c.isPending(1)).toBe(false)
  })

  it('超时自动撤防', () => {
    const c = useConfirmDelete(3000)
    expect(c.arm(7)).toBe(false)
    vi.advanceTimersByTime(3000)
    expect(c.isPending(7)).toBe(false)
    // 撤防后重新进入待确认（不是直接执行）
    expect(c.arm(7)).toBe(false)
  })

  it('切换 id 时旧的自动撤防', () => {
    const c = useConfirmDelete()
    expect(c.arm(1)).toBe(false)
    expect(c.arm(2)).toBe(false)
    expect(c.isPending(1)).toBe(false)
    expect(c.isPending(2)).toBe(true)
  })
})

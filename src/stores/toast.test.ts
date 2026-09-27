// toast 单测：上限 3 条，同文合并（连点不堆积）
import { describe, it, expect, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { useToastStore } from './toast'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('toast 上限与合并', () => {
  it('最多保留 3 条，超了挤掉最早的', () => {
    const t = useToastStore()
    t.show('a')
    t.show('b')
    t.show('c')
    t.show('d')
    expect(t.items.map((i) => i.message)).toEqual(['b', 'c', 'd'])
  })
  it('同文重复不堆积', () => {
    const t = useToastStore()
    t.show('同文')
    t.show('同文')
    expect(t.items).toHaveLength(1)
  })
  it('error/success 快捷仍可用', () => {
    const t = useToastStore()
    t.error('错')
    t.success('好')
    expect(t.items.map((i) => i.type)).toEqual(['error', 'success'])
  })
})

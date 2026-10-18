// lib/utils.ts 单测（F-81，王二狗）
// 契约：cn() = twMerge(clsx(inputs)) —— 先 clsx 组装（falsy 过滤、数组/对象展开），
//       再 tailwind-merge 去掉后覆盖前的同类 Tailwind 冲突类。
import { describe, it, expect } from 'vitest'

import { cn } from './utils'

describe('cn：clsx 组装', () => {
  it('纯字符串原样拼接（空格分隔）', () => {
    expect(cn('a', 'b', 'c')).toBe('a b c')
  })

  it('falsy 值被过滤（false/null/undefined/空串）', () => {
    expect(cn('a', false, null, undefined, '', 'b')).toBe('a b')
  })

  it('三元/短路条件表达式常见用法', () => {
    const on = true
    expect(cn('base', on && 'active', !on && 'idle')).toBe('base active')
  })

  it('对象形式：值为真的键保留，为假的键丢弃', () => {
    expect(cn({ a: true, b: false, c: true })).toBe('a c')
  })

  it('数组形式：展开后过滤 falsy', () => {
    expect(cn(['a', ['b', false, 'c']], null, 'd')).toBe('a b c d')
  })

  it('无参数 / 全 falsy：返回空串', () => {
    expect(cn()).toBe('')
    expect(cn(false, null, undefined)).toBe('')
  })
})

describe('cn：tailwind-merge 冲突合并', () => {
  it('同组冲突类后者胜（padding）', () => {
    expect(cn('p-2', 'p-4')).toBe('p-4')
  })

  it('同组冲突类后者胜（文本色）', () => {
    expect(cn('text-red-500', 'text-blue-600')).toBe('text-blue-600')
  })

  it('非冲突类保留顺序（不同组并存）', () => {
    expect(cn('flex', 'items-center', 'gap-2')).toBe('flex items-center gap-2')
  })

  it('条件拼出的冲突类被正确合并（常见组件写法）', () => {
    const active = true
    expect(cn('px-2 py-1', active && 'px-4')).toBe('py-1 px-4')
  })
})

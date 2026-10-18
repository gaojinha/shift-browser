// backdrop store 单测（F-80，王二狗）
// 契约：初始无底垫（null）/ set(url) 落盘快照 / clear() 撤下快照。
// 无 IPC 依赖，纯状态机（setup 式 store，ref image）。
import { describe, it, expect, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'

import { useBackdropStore } from './backdrop'

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('backdrop store', () => {
  it('初始 image 为 null（无底垫）', () => {
    const s = useBackdropStore()
    expect(s.image).toBeNull()
  })

  it('set：写入快照 data URL', () => {
    const s = useBackdropStore()
    s.set('data:image/png;base64,AAAA')
    expect(s.image).toBe('data:image/png;base64,AAAA')
  })

  it('clear：撤下快照回 null', () => {
    const s = useBackdropStore()
    s.set('data:image/png;base64,AAAA')
    s.clear()
    expect(s.image).toBeNull()
  })

  it('set 可覆盖既有快照；clear 幂等（重复清仍为 null）', () => {
    const s = useBackdropStore()
    s.set('first')
    s.set('second')
    expect(s.image).toBe('second')
    s.clear()
    s.clear()
    expect(s.image).toBeNull()
  })
})

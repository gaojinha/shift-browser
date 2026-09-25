// useManagedListeners 防漂移测试（R1-5）：
// 挂载→卸载必须全量解绑；重挂载后旧实例不再收到事件（无重复触发）；
// 注册 Promise 晚于卸载 resolve 时也要立即解绑，不留泄漏。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { effectScope } from 'vue'

// 极简 Tauri 事件总线模拟：listen 注册进 registry，unlisten 移除——
// 与真实 Tauri 语义一致，于是"旧 handler 不再触发"可以直接断言
type Handler = (payload: unknown) => void
const registry = new Map<string, Set<Handler>>()
const listenMock = vi.fn(
  (event: string, handler: (ev: { payload: unknown }) => void) => {
    const h: Handler = (payload) => handler({ payload })
    if (!registry.has(event)) registry.set(event, new Set())
    registry.get(event)!.add(h)
    return Promise.resolve(() => {
      registry.get(event)?.delete(h)
    })
  },
)

vi.mock('@tauri-apps/api/event', () => ({
  listen: (...args: unknown[]) => listenMock(...args),
}))

import { useManagedListeners } from './useManagedListeners'

function emit(event: string, payload: unknown) {
  registry.get(event)?.forEach((h) => h(payload))
}

beforeEach(() => {
  registry.clear()
  listenMock.mockClear()
})

describe('useManagedListeners', () => {
  it('卸载时全量解绑，重挂载后旧实例不重复触发', async () => {
    const h1 = vi.fn()
    const scope1 = effectScope()
    await scope1.run(async () => {
      const { add } = useManagedListeners()
      await add('ev-x', h1)
    })
    expect(listenMock).toHaveBeenCalledTimes(1)

    // 卸载前：正常收到
    emit('ev-x', 1)
    expect(h1).toHaveBeenCalledTimes(1)

    scope1.stop()

    // 卸载后：旧 handler 已从总线移除，不再触发（无重复监听）
    emit('ev-x', 2)
    expect(h1).toHaveBeenCalledTimes(1)

    // 重挂载：新实例独立注册
    const h2 = vi.fn()
    const scope2 = effectScope()
    await scope2.run(async () => {
      const { add } = useManagedListeners()
      await add('ev-x', h2)
    })
    expect(listenMock).toHaveBeenCalledTimes(2)

    emit('ev-x', 3)
    expect(h1).toHaveBeenCalledTimes(1) // 旧实例不触发
    expect(h2).toHaveBeenCalledTimes(1) // 新实例触发一次
    scope2.stop()
  })

  it('多个监听一次性全部解绑', async () => {
    const scope = effectScope()
    await scope.run(async () => {
      const { add } = useManagedListeners()
      await add('a', vi.fn())
      await add('b', vi.fn())
      await add('c', vi.fn())
    })
    expect(registry.get('a')?.size).toBe(1)
    scope.stop()
    expect(registry.get('a')?.size ?? 0).toBe(0)
    expect(registry.get('b')?.size ?? 0).toBe(0)
    expect(registry.get('c')?.size ?? 0).toBe(0)
  })

  it('注册 Promise 晚于卸载 resolve 时立即解绑，不留残留', async () => {
    let resolveLate: () => void = () => {}
    listenMock.mockImplementationOnce((event: string, handler: (ev: { payload: unknown }) => void) => {
      return new Promise((unresolve) => {
        resolveLate = () => {
          const h = () => handler({ payload: null })
          if (!registry.has(event)) registry.set(event, new Set())
          registry.get(event)!.add(h)
          unresolve(() => {
            registry.get(event)?.delete(h)
          })
        }
      })
    })

    const scope = effectScope()
    const pending = scope.run(() => {
      const { add } = useManagedListeners()
      return add('late', vi.fn())
    })!
    scope.stop() // 注册尚未完成就卸载
    expect(registry.get('late')?.size ?? 0).toBe(0)

    resolveLate() // 此时注册才落地——应被立即解绑
    await pending
    expect(registry.get('late')?.size ?? 0).toBe(0)
  })
})

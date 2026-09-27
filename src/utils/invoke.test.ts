// invokeOk / toAppError / errText 单测（R1 错误类型化）
// 契约：Rust CommandError = {code ∈ 八码, message}；旧命令裸字符串兜底 E_INTERNAL
import { describe, it, expect, vi, beforeEach } from 'vitest'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))

import { invokeOk, toAppError, errText } from './invoke'

beforeEach(() => {
  invokeMock.mockReset()
})

describe('toAppError', () => {
  it('合法 CommandError 对象原样通过', () => {
    expect(toAppError({ code: 'E_AUTH', message: 'bad key' })).toEqual({
      code: 'E_AUTH',
      message: 'bad key',
    })
    expect(toAppError({ code: 'E_NET', message: 'AI 请求失败: timeout' })).toEqual({
      code: 'E_NET',
      message: 'AI 请求失败: timeout',
    })
  })

  it('裸字符串（未迁移旧命令）兜底 E_INTERNAL，消息原样保留', () => {
    expect(toAppError('主窗口不存在')).toEqual({
      code: 'E_INTERNAL',
      message: '主窗口不存在',
    })
  })

  it('Error 实例取 message，兜底 E_INTERNAL', () => {
    expect(toAppError(new Error('db locked'))).toEqual({
      code: 'E_INTERNAL',
      message: 'db locked',
    })
  })

  it('七码之外的 code 不信任：code 兜底 E_INTERNAL，保留 message', () => {
    expect(toAppError({ code: 'E_WHATEVER', message: 'm' })).toEqual({
      code: 'E_INTERNAL',
      message: 'm',
    })
  })

  it('第八码 E_RATELIMIT 原样通过（前端可按码提示稍后重试）', () => {
    expect(toAppError({ code: 'E_RATELIMIT', message: '限流' })).toEqual({
      code: 'E_RATELIMIT',
      message: '限流',
    })
  })

  it('null/undefined/数字等异型值不炸，String() 兜底', () => {
    expect(toAppError(null).code).toBe('E_INTERNAL')
    expect(toAppError(undefined).code).toBe('E_INTERNAL')
    expect(toAppError(42)).toEqual({ code: 'E_INTERNAL', message: '42' })
  })
})

describe('errText', () => {
  it('新旧两种形态都取到人读消息', () => {
    expect(errText('裸字符串')).toBe('裸字符串')
    expect(errText({ code: 'E_DB', message: '启停失败: disk full' })).toBe('启停失败: disk full')
  })
})

describe('invokeOk', () => {
  it('成功时透传返回值与参数', async () => {
    invokeMock.mockResolvedValueOnce({ ok: 1 })
    await expect(invokeOk('get_tabs', { a: 1 })).resolves.toEqual({ ok: 1 })
    expect(invokeMock).toHaveBeenCalledWith('get_tabs', { a: 1 })
  })

  it('CommandError 对象 reject → 抛同形 AppErrorShape（调用方可按 code 分流）', async () => {
    invokeMock.mockRejectedValueOnce({ code: 'E_PERM', message: '只读模式' })
    const err = await invokeOk('save_session').catch((e) => e)
    expect(err.code).toBe('E_PERM')
    expect(err.message).toBe('只读模式')
  })

  it('裸字符串 reject → 归一为 E_INTERNAL', async () => {
    invokeMock.mockRejectedValueOnce('timeout')
    const err = await invokeOk('ai_chat').catch((e) => e)
    expect(err).toEqual({ code: 'E_INTERNAL', message: 'timeout' })
  })
})

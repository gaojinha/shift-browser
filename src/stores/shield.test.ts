// 护盾 store 单测：白名单匹配口径必须与 Rust is_whitelisted（extensions/privacy.rs）同规则——
// 后缀匹配 + 条目归一化（*./协议/路径/端口），父域条目覆盖子站。改口径必须两边同步（防漂移）。
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { createPinia, setActivePinia } from 'pinia'
import { hostMatches, useShieldStore } from './shield'

const invokeMock = vi.hoisted(() => vi.fn())
vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }))

describe('hostMatches（与 Rust is_whitelisted 同口径）', () => {
  it('精准命中', () => {
    expect(hostMatches('example.com', 'example.com')).toBe(true)
  })
  it('父域条目覆盖子站（后缀匹配）', () => {
    expect(hostMatches('a.example.com', 'example.com')).toBe(true)
    expect(hostMatches('a.b.example.com', 'example.com')).toBe(true)
  })
  it('不做前缀误命中', () => {
    expect(hostMatches('notexample.com', 'example.com')).toBe(false)
  })
  it('条目归一化：*. 前缀 / 协议 / 路径 / 端口', () => {
    expect(hostMatches('a.example.com', '*.example.com')).toBe(true)
    expect(hostMatches('example.com', 'https://example.com/x')).toBe(true)
    expect(hostMatches('example.com', 'example.com:8443')).toBe(true)
    expect(hostMatches('Example.COM', 'EXAMPLE.com')).toBe(true)
  })
  it('空 host / 空条目不命中', () => {
    expect(hostMatches('', 'example.com')).toBe(false)
    expect(hostMatches('example.com', '')).toBe(false)
    expect(hostMatches('example.com', '*.')).toBe(false)
  })
})

describe('shield store 白名单读写', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    invokeMock.mockReset()
  })

  it('isSiteShieldOff：父域条目覆盖子站（与 Rust 判定一致）', async () => {
    invokeMock.mockResolvedValue(['example.com'])
    const s = useShieldStore()
    await expect(s.isSiteShieldOff('sub.example.com')).resolves.toBe(true)
    await expect(s.isSiteShieldOff('other.com')).resolves.toBe(false)
  })

  it('toggleSiteShield 降盾：返回 false（操作后未防护），白名单追加', async () => {
    invokeMock.mockResolvedValue(['other.com'])
    const s = useShieldStore()
    await expect(s.toggleSiteShield('a.com')).resolves.toBe(false)
    expect(invokeMock).toHaveBeenCalledWith('set_privacy_whitelist', {
      domains: ['other.com', 'a.com'],
    })
  })

  it('toggleSiteShield 恢复：精准条目移除，返回 true（操作后防护中）', async () => {
    invokeMock.mockResolvedValue(['a.com', 'other.com'])
    const s = useShieldStore()
    await expect(s.toggleSiteShield('a.com')).resolves.toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('set_privacy_whitelist', { domains: ['other.com'] })
  })

  it('toggleSiteShield 恢复：父域/通配写法的覆盖条目一并移除（不留死条目）', async () => {
    invokeMock.mockResolvedValue(['*.a.com', 'a.com/', 'other.com'])
    const s = useShieldStore()
    await expect(s.toggleSiteShield('x.a.com')).resolves.toBe(true)
    expect(invokeMock).toHaveBeenCalledWith('set_privacy_whitelist', { domains: ['other.com'] })
  })
})

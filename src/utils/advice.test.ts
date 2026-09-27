// advice 单测：权限/证书/自检一句话助手（与组件内旧实现同口径迁移）
import { describe, it, expect } from 'vitest'
import { adviceFor, verdictFor, fixFor } from './advice'

describe('adviceFor 权限', () => {
  it('五类全有用途+建议', () => {
    for (const k of ['media', 'geolocation', 'notification', 'pointer-lock', 'device-info']) {
      const a = adviceFor(k)
      expect(a.use.length).toBeGreaterThan(0)
      expect(a.advice.length).toBeGreaterThan(0)
    }
  })
  it('未知种类走兜底：不认识就拒绝', () => {
    expect(adviceFor('xxx').advice).toContain('拒绝')
  })
})

describe('verdictFor 证书', () => {
  it.each([
    ['证书已过期', '过期'],
    ['未知的 CA（自签', '自签'],
    ['证书与主机名不匹配', '钓鱼'],
    ['证书已被吊销', '不要继续'],
    ['证书尚未生效', '系统时间'],
    ['不安全的算法', '强度不够'],
    ['啥也没说', '谨慎'],
  ])('%s → %s', (reason, want) => {
    expect(verdictFor(reason)).toContain(want)
  })
})

describe('fixFor 自检', () => {
  it.each([
    ['内容过滤器', '隐私防护'],
    ['密码主密钥', '主密钥'],
    ['数据库', '磁盘'],
    ['数据目录', '挂载'],
    ['主窗口', '重启'],
    ['隐私防护', '隐私防护'],
    ['启用扩展', '扩展管理'],
    ['没见过的项', '报上来'],
  ])('%s → %s', (name, want) => {
    expect(fixFor(name)).toContain(want)
  })
})

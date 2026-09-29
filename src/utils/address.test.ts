// 地址栏认址纯函数单测（URL 识别 / 默认协议 / 搜索分流）
import { describe, it, expect } from 'vitest'
import { looksLikeUrl, defaultScheme, resolveInputTarget } from './address'

describe('looksLikeUrl', () => {
  it('公网域名/含点输入是 URL', () => {
    expect(looksLikeUrl('example.com')).toBe(true)
    expect(looksLikeUrl('www.baidu.com/s?wd=1')).toBe(true)
  })
  it('单标签内网主机名是 URL，纯数字走搜索', () => {
    expect(looksLikeUrl('nas')).toBe(true)
    expect(looksLikeUrl('router')).toBe(true)
    expect(looksLikeUrl('12345')).toBe(false)
  })
  it('IP/localhost/host:port 是 URL', () => {
    expect(looksLikeUrl('192.168.1.1')).toBe(true)
    expect(looksLikeUrl('localhost:3000')).toBe(true)
    expect(looksLikeUrl('nas:5000')).toBe(true)
  })
  it('普通中文词不是 URL', () => {
    expect(looksLikeUrl('今天天气')).toBe(false)
  })
})

describe('defaultScheme', () => {
  it('公网域名走 https', () => {
    expect(defaultScheme('example.com')).toBe('https')
    expect(defaultScheme('example.com/path')).toBe('https')
  })
  it('内网字面量走 http', () => {
    expect(defaultScheme('192.168.1.1')).toBe('http')
    expect(defaultScheme('localhost')).toBe('http')
    expect(defaultScheme('nas')).toBe('http')
    expect(defaultScheme('[::1]')).toBe('http')
  })
  it('非443端口走 http，443 走 https', () => {
    expect(defaultScheme('example.com:8080')).toBe('http')
    expect(defaultScheme('example.com:443')).toBe('https')
  })
})

describe('resolveInputTarget', () => {
  it('显式协议原样放行', () => {
    expect(resolveInputTarget('http://192.168.1.1/')).toEqual({
      url: 'http://192.168.1.1/',
      isSearch: false,
    })
  })
  it('公网域名补 https', () => {
    const r = resolveInputTarget('example.com')
    expect(r).toEqual({ url: 'https://example.com', isSearch: false })
  })
  it('内网 IP/单标签主机名补 http', () => {
    expect(resolveInputTarget('192.168.1.1').url).toBe('http://192.168.1.1')
    expect(resolveInputTarget('nas').url).toBe('http://nas')
    expect(resolveInputTarget('nas:5000').url).toBe('http://nas:5000')
  })
  it('带空格走搜索', () => {
    const r = resolveInputTarget('今天 天气怎么样')
    expect(r.isSearch).toBe(true)
    expect(r.url).toContain(encodeURIComponent('今天 天气怎么样'))
  })
  it('纯数字走搜索', () => {
    expect(resolveInputTarget('12345').isSearch).toBe(true)
  })
  it('空输入返回空', () => {
    expect(resolveInputTarget('   ')).toEqual({ url: '', isSearch: false })
  })
})

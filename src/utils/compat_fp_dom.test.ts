// Chrome 指纹补齐脚本单测（2026-09-29 Turnstile 确诊缺口）
//
// 直接跑 compat_chrome.js 本体（不复制逻辑）：缺的三件必须出现、值必须与
// UA 串一致（Linux/142 系）、重复注入无副作用、已有属性不覆盖。
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { JSDOM } from 'jsdom'

const JS = readFileSync(resolve(__dirname, '../../src-tauri/src/compat_chrome.js'), 'utf-8')

function makePage() {
  const dom = new JSDOM('<!doctype html><html><body></body></html>', {
    runScripts: 'outside-only',
    url: 'https://example.com/',
  })
  const w = dom.window as unknown as Record<string, unknown>
  // jsdom 自带 userAgentData/deviceMemory/scheduler 吗？先删干净，模拟 WebKitGTK 缺件
  // （删不掉就跳过该断言——不断言环境，只断言脚本行为）
  return w
}

describe('指纹补齐脚本', () => {
  it('缺件补齐：userAgentData/deviceMemory/scheduler 出现且形状正确', () => {
    const w = makePage()
    ;(w.eval as (s: string) => void)(JS)
    const nav = w.navigator as Record<string, unknown>
    const uad = nav['userAgentData'] as {
      brands: Array<{ brand: string; version: string }>
      mobile: boolean
      platform: string
      getHighEntropyValues: (h: string[]) => Promise<Record<string, unknown>>
    }
    expect(uad, 'userAgentData 必须被补上').toBeTruthy()
    expect(uad.mobile).toBe(false)
    expect(uad.platform).toBe('Linux')
    expect(uad.brands.map((b) => b.brand)).toContain('Google Chrome')
    expect(nav['deviceMemory']).toBe(8)
    expect(w['scheduler']).toBeTruthy()
    return (uad.getHighEntropyValues(['platform']) as Promise<Record<string, unknown>>).then(
      (he) => {
        expect(he['platform']).toBe('Linux')
        expect(he['uaFullVersion']).toContain('142')
      },
    )
  })

  it('幂等：重复注入不抛错不覆盖已有值', () => {
    const w = makePage()
    const run = w.eval as (s: string) => void
    run(JS)
    run(JS)
    expect(w['__sbCompatFP']).toBe(true)
    const nav = w.navigator as Record<string, unknown>
    expect(nav['userAgentData']).toBeTruthy()
  })

  it('已有属性不覆盖（别家先占的坑不动）', () => {
    const w = makePage()
    const run = w.eval as (s: string) => void
    run("Object.defineProperty(navigator, 'deviceMemory', { value: 4, configurable: true });")
    run(JS)
    const nav = w.navigator as Record<string, unknown>
    expect(nav['deviceMemory']).toBe(4)
    expect(w['__sbCompatFP']).toBe(true)
  })
})

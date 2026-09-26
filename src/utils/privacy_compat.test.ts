// 风控站点兼容性 · 隐私/广告注入脚本的帧策略测试（2026-09-26 Upwork/Cloudflare 事故）
//
// 背景：Upwork 全站挂 Cloudflare managed challenge，挑战代码跑在跨域 iframe
// （challenges.cloudflare.com）里，专门检测原生 API 是否被钩。我们的隐私脚本
// 通过 AllFrames 注入，连挑战 iframe 也被打了 canvas/RTC/plugins 等补丁，
// 导致安全检查永远失败。修复约定（改脚本必须同时改这里）：
//   1. 跨域子框架：privacy.js / strict_privacy.js / adblock.js 一律零修改；
//   2. 白名单 = 完全兼容模式：整帧不装任何指纹防护（不再只是跳过加噪）；
//   3. 顶层 / 同源子框架：防护照常生效。
//
// 跨域判定靠"读 top.location 抛 SecurityError"。jsdom 模拟不了跨域，
// 这里用 node:vm 手工搭最小沙盒（跨域 top 的 location 是抛错 getter）。
//
// 跑法：npx -y node@22 node_modules/vitest/vitest.mjs run src/utils/privacy_compat.test.ts
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import vm from 'node:vm'
import { JSDOM } from 'jsdom'

const PRIVACY_JS = readFileSync(
  resolve(__dirname, '../../src-tauri/resources/js/privacy.js'),
  'utf-8',
)
const STRICT_JS = readFileSync(
  resolve(__dirname, '../../src-tauri/resources/js/strict_privacy.js'),
  'utf-8',
)
const ADBLOCK_TEMPLATE = readFileSync(
  resolve(__dirname, '../../src-tauri/resources/js/adblock.js'),
  'utf-8',
)
const ADBLOCK_JS = ADBLOCK_TEMPLATE.replace('__SB_KEYWORDS__', '[]')

/** 构造一个"跨域子框架"的最小 window：top !== self 且读 top.location 抛错 */
function crossFrameSandbox(): Record<string, unknown> {
  const crossTop = {}
  Object.defineProperty(crossTop, 'location', {
    get() {
      throw new Error('SecurityError: blocked cross-origin access')
    },
  })
  const sandbox: Record<string, unknown> = {
    location: { hostname: 'challenges.cloudflare.com', protocol: 'https:' },
  }
  sandbox.window = sandbox
  sandbox.self = sandbox
  sandbox.top = crossTop
  vm.createContext(sandbox)
  return sandbox
}

/** 同源子框架沙盒：top 是另一个可读 location 的窗口 */
function sameOriginFrameSandbox(): Record<string, unknown> {
  const top = { location: { href: 'https://example.com/', origin: 'https://example.com' } }
  const sandbox: Record<string, unknown> = {
    location: { hostname: 'sub.example.com', protocol: 'https:', origin: 'https://example.com', href: 'https://sub.example.com/' },
    navigator: {},
  }
  sandbox.window = sandbox
  sandbox.self = sandbox
  sandbox.top = top
  vm.createContext(sandbox)
  return sandbox
}

function jsdomWindow(url?: string) {
  return new JSDOM('<!doctype html><html><body></body></html>', {
    url,
    runScripts: 'outside-only',
  }).window
}

describe('privacy.js · 跨域风控 iframe 零修改', () => {
  it('跨域子框架：只留幂等标记，不装任何防护', () => {
    const sb = crossFrameSandbox()
    vm.runInNewContext(PRIVACY_JS, sb)
    expect(sb.__shiftPrivacyInstalled).toBe(true)
    // 下面这些一律不应出现（出现即说明防护装进了风控 iframe）
    expect(sb.__shiftPrivacyFeatures).toBeUndefined()
    expect(sb.__shiftNoiseOn).toBeUndefined()
    expect(sb.__shiftPrivacyBypass).toBeUndefined()
    // navigator 根本没被碰过
    expect('navigator' in sb).toBe(false)
  })

  it('同源子框架：防护照常安装（跨域豁免不能误伤同源 iframe）', () => {
    const sb = sameOriginFrameSandbox()
    vm.runInNewContext(PRIVACY_JS, sb)
    const feat = sb.__shiftPrivacyFeatures as Record<string, unknown>
    expect(feat).toBeTruthy()
    expect(feat.gpc).toBe(true)
    expect(feat.canvas).toBe(true)
    // navigator 上真的装了 GPC getter
    const nav = sb.navigator as Record<string, unknown>
    const desc = Object.getOwnPropertyDescriptor(nav, 'globalPrivacyControl')
    expect(desc?.get?.()).toBe(true)
  })

  it('顶层框架：完整安装', () => {
    const w = jsdomWindow()
    w.eval(PRIVACY_JS)
    expect(w.eval('window.__shiftPrivacyFeatures.gpc')).toBe(true)
    expect(w.eval('window.__shiftPrivacyFeatures.canvas')).toBe(true)
    expect(w.eval('window.__shiftNoiseOn')).toBe(true)
  })
})

describe('privacy.js · 白名单 = 完全兼容模式', () => {
  it('白名单域名：整帧保持浏览器原生，连 GPC/plugins 都不改', () => {
    const w = jsdomWindow('https://www.upwork.com/nx/find-work/')
    // jsdom 自带 plugins（原型上）；记录原生形态
    expect(w.navigator.plugins).toBeTruthy()
    w.eval("window.__SHIFT_PRIVACY_WHITELIST__ = ['upwork.com']")
    w.eval(PRIVACY_JS)
    expect(w.eval('window.__shiftPrivacyBypass')).toBe(true)
    expect(w.eval('window.__shiftNoiseOn')).toBe(false)
    expect(w.eval('window.__shiftPrivacyFeatures')).toBeUndefined()
    // GPC 未安装
    expect('globalPrivacyControl' in w.navigator).toBe(false)
    // plugins 未被替换成假对象（无 own property，仍是 jsdom 原生原型链）
    expect(Object.getOwnPropertyDescriptor(w.navigator, 'plugins')).toBeUndefined()
  })

  it('白名单按子域名命中（www.upwork.com 命中 upwork.com）', () => {
    const sb = sameOriginFrameSandbox()
    ;(sb.location as Record<string, unknown>).hostname = 'www.upwork.com'
    vm.runInNewContext(
      "window.__SHIFT_PRIVACY_WHITELIST__ = ['upwork.com'];\n" + PRIVACY_JS,
      sb,
    )
    expect(sb.__shiftPrivacyBypass).toBe(true)
    expect(sb.__shiftPrivacyFeatures).toBeUndefined()
  })

  it('非白名单域名不受影响', () => {
    const sb = sameOriginFrameSandbox()
    vm.runInNewContext(
      "window.__SHIFT_PRIVACY_WHITELIST__ = ['upwork.com'];\n" + PRIVACY_JS,
      sb,
    )
    expect(sb.__shiftPrivacyBypass).toBeUndefined()
    expect(sb.__shiftPrivacyFeatures).toBeTruthy()
  })
})

describe('strict_privacy.js · 同步豁免', () => {
  it('跨域子框架：零修改（不设 strict 标记）', () => {
    const sb = crossFrameSandbox()
    vm.runInNewContext(STRICT_JS, sb)
    expect(sb.__shiftPrivacyStrict).toBeUndefined()
  })

  it('白名单 bypass 标记：严格档整体跳过', () => {
    const w = jsdomWindow('https://www.upwork.com/')
    w.eval('window.__shiftPrivacyBypass = true')
    w.eval(STRICT_JS)
    expect(w.eval('window.__shiftPrivacyStrict')).toBeUndefined()
  })

  it('普通顶层帧：严格档正常安装', () => {
    const w = jsdomWindow()
    w.eval(STRICT_JS)
    expect(w.eval('window.__shiftPrivacyStrict')).toBe(true)
  })
})

describe('adblock.js · 跨域 iframe 零介入', () => {
  it('跨域子框架：直接返回，不碰 DOM / 不挂 MutationObserver', () => {
    const sb = crossFrameSandbox()
    // 故意不给 document：守卫若失效，后续 querySelector 访问会立刻抛错
    expect(() => vm.runInNewContext(ADBLOCK_JS, sb)).not.toThrow()
    expect(sb.__shiftAdblockInstalled).toBe(true)
  })

  it('顶层帧：文本关键词照常拦截', () => {
    const w = new JSDOM(
      '<!doctype html><html><body><div id="ad">广告位招商</div></body></html>',
      { url: 'https://example.com/', runScripts: 'outside-only' },
    ).window
    const js = ADBLOCK_TEMPLATE.replace('__SB_KEYWORDS__', "['广告位']")
    // jsdom 不做布局，offsetWidth/Height 恒 0，会被脚本的"不可见元素跳过"保护漏掉
    const adEl = w.document.getElementById('ad') as HTMLElement
    Object.defineProperty(adEl, 'offsetWidth', { value: 200, configurable: true })
    Object.defineProperty(adEl, 'offsetHeight', { value: 80, configurable: true })
    w.eval(js)
    expect(adEl.style.display).toBe('none')
  })
})

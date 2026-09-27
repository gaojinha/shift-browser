// read_page 注入脚本的真实 DOM 行为测试（2026-09-27 · 开放生态 P1-1）
//
// 为什么直接 eval 源码文件，而不是另写纯函数单测：
// page_read.js 是 include_str! 嵌入 Rust 的唯一采集真身（占位符 IIFE），
// 抽纯函数反而制造"两份逻辑"。这里照 webzh_dom.test.ts 同款模式：
// 读源码 → 替占位符 → jsdom 里 eval → 断言 JSON 输出。改了脚本立刻反映。
//
// 覆盖面（对齐 docs/local-api.md 的 hints/脱敏约定）：
// - hints 三类触发（canvas/svg、密码框、验证码）+ 正常页零误报
// - 脱敏：password/hidden 的 value 一律不返回（elements/forms/html 三模式）
// - safeUrl：非 http(s) 只留 scheme+长度
// - text 模式 truncated/empty 标记、html 模式 script/style/注释剥离
// - 未知模式返回 {error}
//
// 跑法：npx vitest run src/utils/page_read_dom.test.ts
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { JSDOM } from 'jsdom'

const PAGE_READ_JS = readFileSync(
  resolve(__dirname, '../../src-tauri/src/page_read.js'),
  'utf-8',
)

/**
 * 替占位符（与 Rust 侧 read_page 的 replace 语义一致）后在 jsdom 里执行。
 * 注意：脚本里 `var MODE = '__MODE__';` 占位符**自带单引号**——Rust 侧
 * js_escape 注入的是"字符串字面量内容"（不含外层引号），两个 MAX 与
 * 三个开关是裸数字。这里照此口径，不能 JSON.stringify（会多包一层引号）。
 */
function readPage(
  html: string,
  opts: {
    mode?: string
    selector?: string
    maxChars?: number
    maxItems?: number
    expand?: boolean
    shadow?: boolean
    iframe?: boolean
  } = {},
): Record<string, unknown> {
  // 单引号字面量内容转义（防 selector 含单引号/反斜杠破坏脚本）
  const lit = (s: string) => s.replace(/\\/g, '\\\\').replace(/'/g, "\\'")
  const code = PAGE_READ_JS
    .replace(/__MODE__/g, lit(opts.mode ?? 'text'))
    .replace(/__SELECTOR__/g, lit(opts.selector ?? 'body'))
    .replace(/__MAX_CHARS__/g, String(opts.maxChars ?? 100000))
    .replace(/__MAX_ITEMS__/g, String(opts.maxItems ?? 200))
    .replace(/__EXPAND__/g, opts.expand ? '1' : '0')
    .replace(/__SHADOW__/g, opts.shadow ? '1' : '0')
    .replace(/__IFRAME__/g, opts.iframe ? '1' : '0')
  const w = new JSDOM(html, {
    runScripts: 'outside-only',
    url: 'https://example.com/page',
  }).window
  return JSON.parse(w.eval(code)) as Record<string, unknown>
}

const hints = (r: Record<string, unknown>) => (r.hints as string[]) ?? []

describe('read_page 注入脚本：hints 判定', () => {
  it('canvas/svg → 建议截图', () => {
    const r = readPage('<body><p>正文</p><canvas id="c"></canvas></body>')
    expect(hints(r).some((h) => h.includes('canvas'))).toBe(true)
  })

  it('密码输入框 → 可能需登录', () => {
    const r = readPage('<body><form><input type="password" name="pw"/></form></body>')
    expect(hints(r).some((h) => h.includes('密码'))).toBe(true)
  })

  it('验证码特征（大小写不敏感）→ 人机验证提示', () => {
    const r = readPage(
      '<body><div class="Captcha-Box">输入验证码</div></body>',
    )
    expect(hints(r).some((h) => h.includes('验证码'))).toBe(true)
  })

  it('正常页零误报：无 canvas/密码/验证码 → hints 为空', () => {
    const r = readPage(
      '<body><h1>标题</h1><p>一段普通文字</p><a href="/x">链接</a><input type="text"/></body>',
    )
    expect(hints(r)).toEqual([])
  })
})

describe('read_page 注入脚本：脱敏', () => {
  it('elements 模式：password/hidden 的 value 不返回，普通 input 返回', () => {
    const r = readPage(
      '<body><input type="text" name="u" value="alice"/><input type="password" name="p" value="secret123"/><input type="hidden" name="t" value="csrf-tok"/></body>',
      { mode: 'elements' },
    )
    const items = r.elements as Array<Record<string, unknown>>
    const u = items.find((i) => i.name === 'u')
    const p = items.find((i) => i.name === 'p')
    const t = items.find((i) => i.name === 't')
    expect(u?.value).toBe('alice')
    expect(p?.value).toBeUndefined()
    expect(t?.value).toBeUndefined()
    // 全文不得出现明文
    expect(JSON.stringify(r)).not.toContain('secret123')
    expect(JSON.stringify(r)).not.toContain('csrf-tok')
  })

  it('forms 模式：任何控件都不返回 value', () => {
    const r = readPage(
      '<body><form><input type="text" name="a" value="v1"/><input type="password" name="b" value="v2"/></form></body>',
      { mode: 'forms' },
    )
    expect(JSON.stringify(r)).not.toContain('value')
  })

  it('html 模式：剥注释/script/style 内容，剥 password input 的 value 属性', () => {
    const r = readPage(
      '<body><!-- 注释SECRET1 --><script>var k = "SECRET2";</script><style>.x{color:red}</style><input type="password" value="SECRET3"/><p>正文</p></body>',
      { mode: 'html' },
    )
    const html = r.html as string
    expect(html).not.toContain('SECRET1')
    expect(html).not.toContain('SECRET2')
    expect(html).not.toContain('SECRET3')
    expect(html).toContain('<script></script>')
    expect(html).toContain('正文')
  })

  it('images 模式：非 http(s) 的 src 只留 scheme+长度，http(s) 原样', () => {
    const r = readPage(
      '<body><img src="data:image/png;base64,AAAAABBBBB"/><img src="https://cdn.example.com/a.png"/></body>',
      { mode: 'images' },
    )
    const items = r.images as Array<Record<string, unknown>>
    const data = items.find((i) => (i.src as string).startsWith('data:'))
    const http = items.find((i) => (i.src as string).startsWith('https:'))
    expect(data?.src).toMatch(/^data:\d+$/)
    expect(data?.src).not.toContain('AAAA')
    expect(http?.src).toBe('https://cdn.example.com/a.png')
  })
})

describe('read_page 注入脚本：text 模式标记', () => {
  it('超 max_chars → truncated=true，total 记原文长度', () => {
    const long = '字'.repeat(500)
    const r = readPage(`<body><p>${long}</p></body>`, { maxChars: 100 })
    expect(r.truncated).toBe(true)
    expect((r.text as string).length).toBeLessThanOrEqual(100)
    expect(r.total).toBe(500)
  })

  it('空白页 → empty=true', () => {
    const r = readPage('<body></body>')
    expect(r.empty).toBe(true)
    expect(r.truncated).toBe(false)
  })

  it('跨源 iframe 读不到 → 计数提示', () => {
    const r = readPage(
      '<body><p>主文档</p><iframe src="https://other.example.com/x"></iframe></body>',
      { iframe: true },
    )
    expect(hints(r).some((h) => h.includes('跨源 iframe'))).toBe(true)
  })

  it('未知模式 → {error}', () => {
    const r = readPage('<body>x</body>', { mode: 'nope' })
    expect(r.error).toBeTruthy()
  })
})

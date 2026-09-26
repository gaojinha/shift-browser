// 整页汉化 · 注入脚本的真实 DOM 行为测试（2026-09-26）
//
// 为什么用 jsdom 真跑一遍注入脚本，而不是只测 Rust 的纯函数：
// page_collect.js / page_apply.js 是**唯一**决定"页面会不会被改坏"的地方。
// Rust 那边的 JSON 解析、分批、prompt 全绿，页面照样可能被写坏——
// 而"页面被写坏"是这类功能最不可接受的事故（改的是别人的页面，还不一定回得来）。
//
// 这里直接 eval 源码文件本身（不复制逻辑），改了脚本这里立刻反映。
//
// 跑法：npx vitest run src/utils/webzh_dom.test.ts
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { JSDOM } from 'jsdom'

const COLLECT_JS = readFileSync(
  resolve(__dirname, '../../src-tauri/src/page_collect.js'),
  'utf-8',
)
const APPLY_BODY = readFileSync(
  resolve(__dirname, '../../src-tauri/src/page_apply.js'),
  'utf-8',
)

/** 与 Rust 侧 page_apply_translation 同一种包法：把脚本当函数传参调 */
function applyCall(mode: string, payload: unknown): string {
  return `(() => { const f = ${APPLY_BODY.trim()}; return f(${JSON.stringify(mode)}, ${JSON.stringify(payload)}); })()`
}

function makePage(html: string) {
  const w = new JSDOM(html, { runScripts: 'outside-only' }).window
  return {
    collect: () => JSON.parse(w.eval(COLLECT_JS)),
    apply: (m: string, p: unknown) => JSON.parse(w.eval(applyCall(m, p))),
    text: () => w.document.body.textContent,
    html: () => w.document.body.innerHTML,
  }
}

const PAGE = `<!doctype html><html><body>
<h1>Hello World</h1><p>This is a sample paragraph for testing translation.</p>
<button title="Close window">Save changes</button><pre>var x = 1;</pre>
<p>2026-09-26</p><p>Already Chinese 中文段落</p>
<input placeholder="Enter your name" /></body></html>`

describe('整页汉化：注入脚本 DOM 行为', () => {
  it('收集：滤掉代码块/日期/已中文，收文本与悬停提示', () => {
    const p = makePage(PAGE)
    const c = p.collect()
    const texts = c.segments.map((s: { text: string }) => s.text)
    expect(texts).toContain('Hello World')
    expect(texts).toContain('Save changes')
    // 不该收的
    expect(texts).not.toContain('var x = 1;')
    expect(texts).not.toContain('2026-09-26')
    expect(texts.join(' ')).not.toContain('Already Chinese')
    // 悬停提示走 attrs
    const attrs = c.attrs.map((a: { attr: string; orig: string }) => `${a.attr}=${a.orig}`)
    expect(attrs).toContain('title=Close window')
    expect(attrs).toContain('placeholder=Enter your name')
  })

  it('收集：按长度升序（短段先翻，早看到效果）', () => {
    const p = makePage(PAGE)
    const chars = p.collect().segments.map((s: { chars: number }) => s.chars)
    expect([...chars].sort((a, b) => a - b)).toEqual(chars)
  })

  it('写回 + 还原：原文逐字节回来，标记清干净', () => {
    const p = makePage(PAGE)
    const orig = p.text()
    const c = p.collect()
    const pairs = c.segments.map((s: { id: string; text: string }) => ({
      id: s.id, orig: s.text, translated: '译文-' + s.text,
    }))
    const attrs = c.attrs.map((a: { id: string; attr: string; orig: string }) => ({
      attr: a.attr, orig: a.orig, translated: '译文-' + a.orig,
    }))

    const ap = p.apply('apply', { pairs, attrs })
    expect(ap.applied).toBe(pairs.length + attrs.length)
    expect(p.text()).toContain('译文-Hello')

    const rs = p.apply('restore', null)
    expect(rs.restored).toBe(pairs.length + attrs.length)
    expect(p.text()).toBe(orig)
    expect(p.html()).not.toContain('__sb')
  })

  it('还原后 title/placeholder 也回到原值', () => {
    const p = makePage(PAGE)
    p.apply('apply', {
      pairs: [], attrs: [{ attr: 'title', orig: 'Close window', translated: '关闭窗口' }],
    })
    expect(p.apply('restore', null).attrs).toBe(1)
    expect(p.html()).toContain('title="Close window"')
    expect(p.html()).toContain('placeholder="Enter your name"')
  })

  /**
   * 回归：曾因 collect 的 cleanup 里 `delete n.__sbOrig` 造成原文永久丢失——
   * 用户重复点汉化时备份被清掉，还原只能回到"上一轮译文"，回不到真原文。
   * 已在 jsdom 手工复现过，这里锁死。
   */
  it('回归：连汉化三次不还原，最终还原仍回原文', () => {
    const p = makePage(PAGE)
    const orig = p.text()
    for (let i = 1; i <= 3; i++) {
      const c = p.collect()
      p.apply('apply', {
        pairs: c.segments.map((s: { id: string; text: string }) => ({
          id: s.id, orig: s.text, translated: `【第${i}轮】` + s.text,
        })),
        attrs: c.attrs.map((a: { id: string; attr: string; orig: string }) => ({
          attr: a.attr, orig: a.orig, translated: `【第${i}轮】` + a.orig,
        })),
      })
    }
    expect(p.text()).toContain('【第3轮】')
    p.apply('restore', null)
    expect(p.text()).toBe(orig)
    expect(p.text()).not.toContain('【第')
  })

  it('页面自己改过的节点不覆盖（轮播/虚拟列表防护）', () => {
    const p = makePage('<!doctype html><html><body><p>original text</p></body></html>')
    const c = p.collect()
    // 传入的 orig 与页面当前内容不符 → 必须跳过，不能拿译文盖掉新内容
    const res = p.apply('apply', {
      pairs: [{ id: c.segments[0].id, orig: '页面早就不一样的内容', translated: '译文' }],
      attrs: [],
    })
    expect(res.applied).toBe(0)
    expect(res.mismatched).toBe(1)
  })
})

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
   * 回归：段落里只要有内联标签（<a>/<span>/<b>），整段主体文字曾被整段漏掉。
   *
   * 旧实现只收"没有元素子节点的纯文本节点"，RFC 9110 实测 1708 段里 66% 是
   * <20 字碎片、Abstract 整段一个字没收到（用户原话"汉化率不高、很多没翻"）。
   * 现在按块级段落整段收。
   *
   * 2026-09-27 起内联标签不再被抹平：收集文本带 ⟦n⟧ 结构标记，markers 结构表
   * 随段带回，写回时重建真实标签（链接翻完还能点）。
   */
  it('回归：段落含内联标签也要整段收集（标签变 ⟦n⟧ 标记）', () => {
    const p = makePage(
      '<!doctype html><html><body>' +
        '<p>The <span>HTTP</span> protocol is a stateless application-level protocol.</p>' +
        '<li><a href="#x">1. Introduction</a></li>' +
        '</body></html>',
    )
    const segs = p.collect().segments as Array<{
      text: string
      markers: Record<string, { tag: string; a: string[][] }> | null
    }>
    const para = segs.find((s) => s.text.includes('stateless'))!
    // 整段正文都在，span 被替换成成对结构标记而不是被删掉
    expect(para.text).toBe('The ⟦0⟧HTTP⟦/0⟧ protocol is a stateless application-level protocol.')
    expect(para.markers).toEqual({ 0: { tag: 'span', a: [] } })
    const li = segs.find((s) => s.text.includes('Introduction'))!
    expect(li.text).toBe('⟦0⟧1. Introduction⟦/0⟧')
    // 链接的 href 必须随结构表带走，写回时才能重建可点链接
    expect(li.markers).toEqual({ 0: { tag: 'a', a: [['href', '#x']] } })
  })

  /** 写回：带标记的译文重建真实 <a>，链接位置可随译文语序移动 */
  it('占位符写回：重建 <a href>，链接文本被翻译且仍可点', () => {
    const p = makePage(
      '<!doctype html><html><body><p>See the <a href="/x">manual</a> for details.</p></body></html>',
    )
    const origHtml = p.html()
    const seg = p.collect().segments[0] as {
      id: string; text: string
      markers: Record<string, { tag: string; a: string[][] }>
    }
    expect(seg.text).toBe('See the ⟦0⟧manual⟦/0⟧ for details.')
    const res = p.apply('apply', {
      // 链接被挪到句首——模型有权调整语序，标记跟着走
      pairs: [{ id: seg.id, orig: seg.text, translated: '⟦0⟧手册⟦/0⟧里有详情。', markers: seg.markers }],
      attrs: [],
    })
    expect(res.applied).toBe(1)
    expect(res.badMarkers ?? 0).toBe(0)
    const a = p.html()
    expect(a).toContain('href="/x"')
    expect(a).toContain('>手册</a>')
    expect(p.text()).toContain('里有详情')
    // 还原：连链接带位置逐字节回来
    p.apply('restore', null)
    expect(p.html()).toBe(origHtml)
  })

  /** 自闭合标记：<br> 这种空元素用 ⟦n/⟧ 表示，写回后仍是换行 */
  it('占位符写回：<br> 自闭合标记重建', () => {
    const p = makePage(
      '<!doctype html><html><body><p>Line one<br/>Line two</p></body></html>',
    )
    const seg = p.collect().segments[0] as { id: string; text: string; markers: unknown }
    expect(seg.text).toBe('Line one⟦0/⟧Line two')
    p.apply('apply', {
      pairs: [{ id: seg.id, orig: seg.text, translated: '第一行⟦0/⟧第二行', markers: seg.markers }],
      attrs: [],
    })
    const html = p.html()
    expect(html).toMatch(/第一行<br\s*\/?>第二行/)
  })

  /**
   * 安全回退：模型把标记弄丢/编号搞乱是常态。任何一种损坏都必须：
   * 不动页面（原文保留）、applied 不计数、badMarkers 计数；备份仍可正常还原。
   */
  // 嵌套 a>b 的段落：map 里有两个 id，交叉/缺号这类深层损坏才能造出来
  const NESTED_HTML =
    '<!doctype html><html><body><p>Click <a href="/x"><b>here</b></a> now</p></body></html>'
  it.each([
    ['标记被整体删掉', '请点击这里'],
    ['有开无闭', '⟦0⟧⟦1⟧这里⟦/1⟧现在'],
    ['幻觉出不存在的 id', '⟦0⟧⟦1⟧这里⟦/1⟧⟦/0⟧⟦2⟧多余⟦/2⟧'],
    ['嵌套交叉闭合', '⟦0⟧⟦1⟧x⟦/0⟧⟦/1⟧'],
    ['闭标记 id 对不上', '⟦0⟧⟦1⟧x⟦/1⟧⟦/9⟧'],
    ['空壳译文', '⟦0⟧⟦1⟧⟦/1⟧⟦/0⟧'],
  ])('占位符损坏安全回退（%s）', (_name, translated) => {
    const p = makePage(NESTED_HTML)
    const origText = p.text()
    const origHtml = p.html()
    const seg = p.collect().segments[0] as { id: string; text: string; markers: unknown }
    const res = p.apply('apply', {
      pairs: [{ id: seg.id, orig: seg.text, translated, markers: seg.markers }],
      attrs: [],
    })
    expect(res.applied).toBe(0)
    expect(res.badMarkers).toBe(1)
    // 页面一个字都不能被改动
    expect(p.text()).toBe(origText)
    // 即便损坏的是最后一条，也不能影响后续还原
    expect(p.apply('restore', null).restored).toBeGreaterThanOrEqual(0)
    expect(p.html()).toBe(origHtml)
  })

  /** 段落级写回：整段换成译文，还原靠 innerHTML 备份，内联标签一起回来 */
  it('段落级写回 + 还原：内联标签与原文逐字节恢复', () => {
    const p = makePage(
      '<!doctype html><html><body><p>See the <a href="/x">manual</a> for details.</p></body></html>',
    )
    const origHtml = p.html()
    const c = p.collect()
    const seg = c.segments[0]
    p.apply('apply', {
      pairs: [{ id: seg.id, orig: seg.text, translated: '详见手册。' }],
      attrs: [],
    })
    expect(p.text()).toContain('详见手册')
    const rs = p.apply('restore', null)
    expect(rs.restored).toBeGreaterThan(0)
    // 连 <a href> 一起回到原样
    expect(p.html()).toBe(origHtml)
  })
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

  it('页面自己改过的节点不覆盖（轮播/虚拟列表防护）', () => {    const p = makePage('<!doctype html><html><body><p>original text</p></body></html>')
    const c = p.collect()
    // 传入的 orig 与页面当前内容不符 → 必须跳过，不能拿译文盖掉新内容
    const res = p.apply('apply', {
      pairs: [{ id: c.segments[0].id, orig: '页面早就不一样的内容', translated: '译文' }],
      attrs: [],
    })
    expect(res.applied).toBe(0)
    expect(res.mismatched).toBe(1)
  })

  /**
   * 2026-09-27：日文是"汉字+假名"混排（これは設定です）、韩文是谚文音节。
   * 旧检测只数汉字，会把它们当中文跳过 → 日韩页面整页漏翻。
   * 出现假名/谚文必须判为"需要汉化成中文"。
   */
  it('语种识别：日文假名/韩文谚文要翻，纯中文不翻', () => {
    const p = makePage(
      '<!doctype html><html><body>' +
        '<p>これは設定です</p>' +
        '<p>コードを保存する</p>' +
        '<p>설정 화면입니다</p>' +
        '<p>这是一个中文段落</p>' +
        '<p>English needs translation</p>' +
        '</body></html>',
    )
    const texts = p.collect().segments.map((s: { text: string }) => s.text)
    // 平假名混汉字：收
    expect(texts).toContain('これは設定です')
    // 片假名：收
    expect(texts).toContain('コードを保存する')
    // 韩文：收
    expect(texts).toContain('설정 화면입니다')
    // 英文：照常收
    expect(texts).toContain('English needs translation')
    // 纯中文：不浪费一轮翻译
    expect(texts).not.toContain('这是一个中文段落')
  })

  /**
   * 回归（2026-09-29 高哥实锤）：翻完链接点不开。
   *
   * 成因：兜底段（div 裸文本 + 图标链接/图片）走无标记路径，旧实现整段
   * textContent 拍平，子元素（含 <a>）一起被删。现在只换直接文本子节点，
   * 子元素原样保留。
   */
  it('无标记写回：图标链接/图片子元素必须保住，译文进直接文本', () => {
    const p = makePage(
      '<!doctype html><html><body><div>Read more <a href="/help"></a> today now</div></body></html>',
    )
    const segs = p.collect().segments as Array<{ id: string; text: string; markers: unknown }>
    expect(segs.length).toBe(1)
    expect(segs[0].markers).toBeNull()
    const res = p.apply('apply', {
      pairs: [{ id: segs[0].id, orig: segs[0].text, translated: '多读今天' }],
      attrs: [],
    })
    expect(res.applied).toBe(1)
    expect(p.text()).toContain('多读今天')
    // 空链接还在，href 没丢
    expect(p.html()).toContain('href="/help"')
    // 还原后逐字节回来
    p.apply('restore', null)
    expect(p.html()).toContain('Read more')
    expect(p.html()).toContain('href="/help"')
  })

  /** 无标记写回：文字全在嵌套块里（无直接文本）则跳过并计数，不碰结构 */
  it('无标记写回：无直接文本可换则跳过，keptStructure 计数', () => {
    const p = makePage(
      '<!doctype html><html><body><li><div>nested block words here</div></li></body></html>',
    )
    const origHtml = p.html()
    const segs = p.collect().segments as Array<{ id: string; text: string; markers: unknown }>
    expect(segs.length).toBe(1)
    const res = p.apply('apply', {
      pairs: [{ id: segs[0].id, orig: segs[0].text, translated: '嵌套文本' }],
      attrs: [],
    })
    expect(res.applied).toBe(0)
    expect(res.keptStructure).toBe(1)
    expect(p.html()).toBe(origHtml)
  })
})

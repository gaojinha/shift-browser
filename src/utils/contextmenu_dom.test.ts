// 右键翻译菜单 DOM 行为测试
//
// context_translate.js 注入所有 http(s) 页面并接管右键菜单。两条铁律：
// 1. 输入类元素（INPUT/TEXTAREA/SELECT/富文本）必须放行原生菜单——
//    自定义菜单里没有粘贴/剪切，拦了就是没收吃饭家伙（2026-09-29 高哥实锤）。
// 2. 普通页面右键弹自定义菜单，且只动自己建的 Shadow DOM，不碰页面节点。
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { JSDOM } from 'jsdom'

const JS = readFileSync(resolve(__dirname, '../../src-tauri/src/context_translate.js'), 'utf-8')

function makePage(html: string) {
  const dom = new JSDOM(html, { runScripts: 'outside-only', url: 'https://example.com/' })
  dom.window.eval(JS)
  return dom.window as unknown as {
    document: Document
    MouseEvent: typeof MouseEvent
  } & Record<string, unknown>
}

function rightClick(w: { document: Document; MouseEvent: typeof MouseEvent }, target: Element) {
  const ev = new w.MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 50, clientY: 50 })
  target.dispatchEvent(ev)
  return ev
}

describe('右键翻译菜单', () => {
  it('输入框右键放行原生菜单（不 preventDefault、不建自定义菜单）', () => {
    const w = makePage(
      '<!doctype html><html><body><input id="kw" type="text"/></body></html>',
    )
    const ev = rightClick(w, w.document.getElementById('kw')!)
    expect(ev.defaultPrevented).toBe(false)
    expect(w.document.getElementById('__sb_tr_host')).toBeNull()
  })

  it('密码框/多行文本/富文本同样放行', () => {
    const w = makePage(
      '<!doctype html><html><body><input type="password" id="p"/><textarea id="t"></textarea>' +
        '<div id="e" contenteditable="true">edit me</div></body></html>',
    )
    for (const id of ['p', 't', 'e']) {
      const ev = rightClick(w, w.document.getElementById(id)!)
      expect(ev.defaultPrevented).toBe(false)
    }
    expect(w.document.getElementById('__sb_tr_host')).toBeNull()
  })

  it('普通页面右键弹自定义菜单，且页面原有节点一个不少', () => {    const w = makePage(
      '<!doctype html><html><body><p>hello <a href="/x">link</a></p></body></html>',
    )
    const before = w.document.body.innerHTML
    const ev = rightClick(w, w.document.querySelector('p')!)
    expect(ev.defaultPrevented).toBe(true)
    // 菜单建在 Shadow DOM 里，页面 body 只多一个 host 壳
    expect(w.document.getElementById('__sb_tr_host')).not.toBeNull()
    expect(w.document.querySelector('a')!.getAttribute('href')).toBe('/x')
    expect(before).toContain('<a href="/x">link</a>')
  })

describe('备用通道地址归一（2026-09-29 双斜杠实锤）', () => {
  function sender() {
    const dom = new JSDOM('<!doctype html><html><body></body></html>', {
      runScripts: 'outside-only', url: 'https://example.com/',
    })
    const w = dom.window as unknown as Record<string, unknown>
    // 只取 __sbTrSend 定义（整文件 eval 会注册右键监听，不影响本用例）
    const src = JS.slice(0, JS.indexOf('(function () {'))
    ;(w.eval as (s: string) => void)(src)
    const urls: string[] = []
    // 桩 Image：只收 src，不真发请求；同时删掉原生桥，走备用通道
    ;(w as Record<string, unknown>).Image = class {
      set src(v: string) {
        urls.push(v)
      }
    }
    delete (w as Record<string, unknown>).webkit
    return { w, urls }
  }

  it("带前导斜杠的 path 不拼出双斜杠（'/batch'→'/batch'）", () => {
    const { w, urls } = sender()
    const send = w.__sbTrSend as (p: string, q: unknown, c: string) => void
    send('/batch', { texts: '["hi"]' }, 'cb1')
    expect(urls).toHaveLength(1)
    expect(urls[0]).toContain('shift://__translate__/batch?')
    expect(urls[0]).not.toContain('//batch')
    expect(urls[0]).toContain('callback=cb1')
  })

  it('不带斜杠的 path 保持原样（whole-page/text）', () => {
    const { w, urls } = sender()
    const send = w.__sbTrSend as (p: string, q: unknown, c: string) => void
    send('whole-page', {}, 'cb2')
    send('text', { text: 'hi' }, 'cb3')
    expect(urls[0]).toContain('shift://__translate__/whole-page?')
    expect(urls[1]).toContain('shift://__translate__/text?')
  })
})
})

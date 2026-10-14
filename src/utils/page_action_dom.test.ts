// action/* 注入脚本的真实 DOM 行为测试（2026-10-14 · 开放生态 P1-1 收尾）
//
// 为什么整脚本 eval 而不是抽纯函数：page_action.js 是 include_str! 嵌入
// api/handlers/actions.rs 的唯一交互真身（占位符 IIFE，660 行 / 9 个动作），
// 抽纯函数会制造"两份逻辑"。照 page_read_dom.test.ts 同款模式：
// 读源码 → 替占位符（口径同 actions.rs：字符串 js_escape，JSON/数字裸注入）
// → jsdom 里 eval → 断言 JSON 输出。改了脚本立刻反映。
//
// 覆盖面（对齐 docs/local-api.md 的 action_error 约定与安全红线）：
// - 预检错误码：not_found / not_visible / disabled / bad_selector / not_editable
// - type 安全红线：password_refused（绝不写密码框）、find 永不命中密码框
// - fill 落值 + input/change 事件；type 逐字符 + clear 先清空
// - key 组合键修饰符；click 导航判定；find 最内层命中与 diagnose
//
// jsdom 无布局引擎：getBoundingClientRect 恒为 0，会把一切元素判成不可见。
// 测试桩按"在文档里即有尺寸"给非 display:none/visibility:hidden 元素 100×20，
// 与脚本自身的 computed-style 可见性判定保持同口径。
//
// 跑法：npx vitest run src/utils/page_action_dom.test.ts
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { JSDOM } from 'jsdom'

const PAGE_ACTION_JS = readFileSync(
  resolve(__dirname, '../../src-tauri/src/page_action.js'),
  'utf-8',
)

// 与 Rust 侧 crate::api::js_escape 完全同口径（\\ ' \n \r 四个字符）
const lit = (s: string) =>
  s.replace(/\\/g, '\\\\').replace(/'/g, "\\'").replace(/\n/g, '\\n').replace(/\r/g, '\\r')

interface ActionOpts {
  action?: string
  selector?: string
  index?: number
  value?: string
  direction?: string
  amount?: number
  condition?: string
  key?: string
  text?: string
  role?: string
  name?: string
  nth?: number
  pierce?: boolean
  clear?: boolean
  exact?: boolean
  visibleOnly?: boolean
  frames?: string // JSON 数组字面量（Rust 侧 frames_literal 产出）
  modifiers?: string // JSON 数组字面量
  expect?: string // JSON 对象字面量或 null
  quietMs?: number
  /** 在动作脚本之前 eval 的窗口侧代码（挂监听器 / 断言埋点） */
  pre?: string
}

/** 默认值与 actions.rs 的缺省注入一致（扩展参数缺省一律"关闭/空"） */
function buildCode(o: ActionOpts): string {
  return PAGE_ACTION_JS
    .replace(/__ACTION__/g, o.action ?? 'click')
    .replace(/__SELECTOR__/g, lit(o.selector ?? ''))
    .replace(/__INDEX__/g, String(o.index ?? 0))
    .replace(/__VALUE__/g, lit(o.value ?? ''))
    .replace(/__DIRECTION__/g, o.direction ?? 'down')
    .replace(/__AMOUNT__/g, String(o.amount ?? 600))
    .replace(/__CONDITION__/g, o.condition ?? 'appear')
    .replace(/__KEY__/g, lit(o.key ?? ''))
    .replace(/__PIERCE__/g, o.pierce ? '1' : '0')
    .replace(/__FRAMES__/g, o.frames ?? '[]')
    .replace(/__MODIFIERS__/g, o.modifiers ?? '[]')
    .replace(/__TEXT__/g, lit(o.text ?? ''))
    .replace(/__ROLE__/g, lit(o.role ?? ''))
    .replace(/__NAME__/g, lit(o.name ?? ''))
    .replace(/__NTH__/g, String(o.nth ?? 0))
    .replace(/__EXACT__/g, o.exact ? '1' : '0')
    .replace(/__WITHIN__/g, lit(''))
    .replace(/__VISIBLE_ONLY__/g, o.visibleOnly === false ? '0' : '1')
    .replace(/__EXPECT__/g, o.expect ?? 'null')
    .replace(/__QUIET_MS__/g, String(o.quietMs ?? 500))
    .replace(/__CLEAR__/g, o.clear ? '1' : '0')
}

// jsdom 布局桩：非隐藏元素给 100×20，display:none/visibility:hidden 仍为 0
const RECT_STUB = `
  Element.prototype.getBoundingClientRect = function () {
    var cs = window.getComputedStyle(this);
    var hidden = cs.display === 'none' || cs.visibility === 'hidden';
    return { left: 0, top: 0, right: hidden ? 0 : 100, bottom: hidden ? 0 : 20,
             width: hidden ? 0 : 100, height: hidden ? 0 : 20, x: 0, y: 0,
             toJSON: function () { return {}; } };
  };
`

function runAction(
  html: string,
  o: ActionOpts = {},
): { result: Record<string, unknown>; win: JSDOM['window'] } {
  const dom = new JSDOM(html, {
    runScripts: 'outside-only',
    url: 'https://example.com/page',
  })
  const win = dom.window
  win.eval(RECT_STUB)
  if (o.pre) win.eval(o.pre)
  const result = JSON.parse(win.eval(buildCode(o))) as Record<string, unknown>
  return { result, win }
}

const errCode = (r: Record<string, unknown>) => r.action_error as string | undefined

describe('click：命中与预检错误码', () => {
  it('命中 button → ok_action（click 响应无 tag 字段，url/navigated 是定位口径）', () => {
    const { result } = runAction('<body><button id="b">点我</button></body>', {
      selector: '#b',
    })
    expect(result.ok_action).toBe('click')
    expect(result.url_before).toBe(result.url_after)
    expect(result.navigated).toBe(false)
  })

  it('点击致地址变化 → navigated=true，url_after 记录新地址', () => {
    // jsdom 不做锚点导航，由页面监听器改 hash 模拟真实导航后地址差
    const { result } = runAction('<body><a id="lnk" href="#next">去</a></body>', {
      selector: '#lnk',
      pre: "document.getElementById('lnk').addEventListener('click', function () { location.hash = 'next'; });",
    })
    expect(result.ok_action).toBe('click')
    expect(result.navigated).toBe(true)
    expect(String(result.url_after)).toContain('#next')
  })

  it('选择器未命中 → not_found', () => {
    const { result } = runAction('<body><p>无</p></body>', { selector: '#nope' })
    expect(errCode(result)).toBe('not_found')
  })

  it('display:none 元素 → not_visible', () => {
    const { result } = runAction(
      '<body><button id="b" style="display:none">藏</button></body>',
      { selector: '#b' },
    )
    expect(errCode(result)).toBe('not_visible')
  })

  it('disabled 按钮 → disabled', () => {
    const { result } = runAction(
      '<body><button id="b" disabled>禁</button></body>',
      { selector: '#b' },
    )
    expect(errCode(result)).toBe('disabled')
  })

  it('非法选择器 → bad_selector', () => {
    const { result } = runAction('<body><button>x</button></body>', {
      selector: '#[',
    })
    expect(errCode(result)).toBe('bad_selector')
  })
})

describe('fill：落值与事件', () => {
  it('fill input → value 落字符串，input/change 事件各一次', () => {
    let fired = ''
    const { result, win } = runAction('<body><input id="t"/></body>', {
      action: 'fill',
      selector: '#t',
      value: 'hello',
      pre: `
        window.__n = 0;
        document.getElementById('t').addEventListener('input', function () { window.__n++; });
        document.getElementById('t').addEventListener('change', function () { window.__n += 10; });
      `,
    })
    fired = String(win.eval('String(window.__n)'))
    expect(result.ok_action).toBe('fill')
    expect(result.tag).toBe('input')
    expect((win.document.querySelector('#t') as HTMLInputElement).value).toBe('hello')
    expect(fired).toBe('11')
  })

  it('index 越界 → not_found（带共数）', () => {
    const { result } = runAction('<body><button id="b">x</button></body>', {
      action: 'fill',
      selector: '#b',
      index: 5,
    })
    expect(errCode(result)).toBe('not_found')
    expect(String(result.error)).toContain('越界')
  })
})

describe('type：逐字符输入与安全红线', () => {
  it('type 进 text input → 逐字符落值，input 事件逐字符触发', () => {
    const { result, win } = runAction('<body><input id="t"/></body>', {
      action: 'type',
      selector: '#t',
      text: 'ab c',
      pre: "window.__n = 0; document.getElementById('t').addEventListener('input', function () { window.__n++; });",
    })
    expect(result.ok_action).toBe('type')
    expect(result.typed).toBe(4)
    expect((win.document.querySelector('#t') as HTMLInputElement).value).toBe('ab c')
    expect(win.eval('String(window.__n)')).toBe('4')
  })

  it('clear=1 → 先清空旧值再输入', () => {
    const { result, win } = runAction('<body><input id="t" value="old"/></body>', {
      action: 'type',
      selector: '#t',
      text: 'new',
      clear: true,
    })
    expect(result.cleared).toBe(true)
    expect((win.document.querySelector('#t') as HTMLInputElement).value).toBe('new')
  })

  it('密码框 → password_refused，值绝不写入（安全红线）', () => {
    const { result, win } = runAction('<body><input id="p" type="password"/></body>', {
      action: 'type',
      selector: '#p',
      text: 'secret',
    })
    expect(errCode(result)).toBe('password_refused')
    expect((win.document.querySelector('#p') as HTMLInputElement).value).toBe('')
  })

  it('普通 div → not_editable', () => {
    const { result } = runAction('<body><div id="d">文本</div></body>', {
      action: 'type',
      selector: '#d',
      text: 'x',
    })
    expect(errCode(result)).toBe('not_editable')
  })

  it('缺 text 且未 clear → bad_action', () => {
    const { result } = runAction('<body><input id="t"/></body>', { action: 'type' })
    expect(errCode(result)).toBe('bad_action')
  })
})

describe('find：只读定位与密码框豁免', () => {
  it('text 瞄准 → 命中最内层，返回 tag/text/selector', () => {
    const { result } = runAction(
      '<body><div>登录百度<div><button>登录百度</button></div></div></body>',
      { action: 'find', text: '登录百度' },
    )
    expect(result.found).toBe(1)
    expect(result.returned).toBe(1)
    expect(result.tag).toBe('button')
    // stablePath 返回 tag 链全路径（不含 class，避构建 hash）
    expect(result.selector).toBe('html > body > div > div > button')
  })

  it('find 永不命中/返回密码框（安全红线）', () => {
    const { result } = runAction(
      '<body><input type="password" aria-label="secret box"/></body>',
      { action: 'find', text: 'secret' },
    )
    expect(result.found).toBe(0)
    expect(JSON.stringify(result)).not.toContain('secret')
  })

  it('nth 越界 → nth_out_of_range', () => {
    const { result } = runAction('<body><button>一</button></body>', {
      action: 'find',
      text: '一',
      nth: 3,
    })
    expect(result.reason).toBe('nth_out_of_range')
  })

  it('not_found → diagnose.nearby 给出页面可交互元素样本', () => {
    const { result } = runAction('<body><button>提交</button></body>', {
      action: 'find',
      text: '不存在的按钮',
    })
    expect(result.found).toBe(0)
    const dg = result.diagnose as { nearby: Array<{ tag: string; text: string }> }
    expect(dg.nearby.length).toBe(1)
    expect(dg.nearby[0].text).toBe('提交')
  })
})

describe('key / hover / scroll', () => {
  it('key Enter+Control → keydown 到达且 ctrlKey=true', () => {
    const { result, win } = runAction('<body><input id="t"/></body>', {
      action: 'key',
      key: 'Enter',
      selector: '#t',
      modifiers: '["Control"]',
      pre: `
        window.__kd = 0; window.__ctrl = false;
        document.getElementById('t').addEventListener('keydown', function (e) {
          window.__kd++; window.__ctrl = e.ctrlKey;
        });
      `,
    })
    expect(result.ok_action).toBe('key')
    expect(result.modifiers).toEqual(['Control'])
    expect(win.eval('String(window.__kd)')).toBe('1')
    expect(win.eval('String(window.__ctrl)')).toBe('true')
  })

  it('hover → mouseover 到达', () => {
    const { result, win } = runAction('<body><button id="b">x</button></body>', {
      action: 'hover',
      selector: '#b',
      pre: "window.__ov = 0; document.getElementById('b').addEventListener('mouseover', function () { window.__ov++; });",
    })
    expect(result.ok_action).toBe('hover')
    expect(win.eval('String(window.__ov)')).toBe('1')
  })

  it('scroll down → scrollTo(0, 600)（stub 验证坐标，消 jsdom not-implemented 噪声）', () => {
    const { result, win } = runAction('<body><p>x</p></body>', {
      action: 'scroll',
      direction: 'down',
      pre: "window.scrollTo = function (x, y) { window.__scroll = [x, y]; };",
    })
    expect(result.ok_action).toBe('scroll')
    expect(win.eval('JSON.stringify(window.__scroll)')).toBe('[0,600]')
  })

  it('scroll top → scrollTo(0, 0)；up 越顶钳到 0', () => {
    const { win } = runAction('<body><p>x</p></body>', {
      action: 'scroll',
      direction: 'top',
      pre: "window.scrollTo = function (x, y) { window.__scroll = [x, y]; };",
    })
    expect(win.eval('JSON.stringify(window.__scroll)')).toBe('[0,0]')
    const { win: win2 } = runAction('<body><p>x</p></body>', {
      action: 'scroll',
      direction: 'up',
      pre: "window.scrollTo = function (x, y) { window.__scroll = [x, y]; };",
    })
    // scrollY=0 时 up 的负值被 Math.max(0,·) 钳到 0
    expect(win2.eval('JSON.stringify(window.__scroll)')).toBe('[0,0]')
  })
})

describe('wait_for（check 条件）', () => {
  it('appear：元素存在 → met=true', () => {
    const { result } = runAction('<body><div id="x">在</div></body>', {
      action: 'check',
      condition: 'appear',
      selector: '#x',
    })
    expect(result.met).toBe(true)
    expect(result.exists).toBe(true)
  })

  it('gone：元素不存在 → met=true', () => {
    const { result } = runAction('<body><p>无</p></body>', {
      action: 'check',
      condition: 'gone',
      selector: '#nope',
    })
    expect(result.met).toBe(true)
    expect(result.exists).toBe(false)
  })

  it('text+expect contains：命中与不命中两态', () => {
    const hit = runAction('<body><div id="m">你好世界</div></body>', {
      action: 'check',
      condition: 'text',
      selector: '#m',
      expect: '{"op":"contains","value":"你好"}',
    }).result
    expect(hit.met).toBe(true)
    const miss = runAction('<body><div id="m">你好世界</div></body>', {
      action: 'check',
      condition: 'text',
      selector: '#m',
      expect: '{"op":"contains","value":"再见"}',
    }).result
    expect(miss.met).toBe(false)
  })
})

describe('兜底', () => {
  it('未知动作 → bad_action（须先过预检：给一个真实存在的 selector）', () => {
    const { result } = runAction('<body><button id="b">x</button></body>', {
      action: 'teleport',
      selector: '#b',
    })
    expect(errCode(result)).toBe('bad_action')
  })
})

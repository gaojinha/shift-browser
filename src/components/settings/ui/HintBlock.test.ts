// HintBlock 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载（纯展示组件）。
// 覆盖：默认 tone=info 不额外挂类名 / warn·danger·success 各挂对应类名 / 始终带 hint-block 基类 /
// role=note（提示块语义）/ 默认插槽内容渲染。
//
// 跑法：npx vitest run src/components/settings/ui/HintBlock.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import HintBlock from './HintBlock.vue'

function render(tone?: 'info' | 'warn' | 'danger' | 'success') {
  return mount(HintBlock, {
    props: tone === undefined ? {} : { tone },
    slots: { default: '提示内容' },
  })
}

describe('HintBlock', () => {
  it('默认 tone=info：只有基类，不额外挂类名', () => {
    const el = render().find('.hint-block')
    expect(el.exists()).toBe(true)
    expect(el.classes()).toEqual(['hint-block'])
  })

  it('显式 tone=info 同样不额外挂类名', () => {
    expect(render('info').find('.hint-block').classes()).toEqual(['hint-block'])
  })

  it.each(['warn', 'danger', 'success'] as const)('tone=%s 挂对应类名', (tone) => {
    const el = render(tone).find('.hint-block')
    expect(el.classes()).toContain('hint-block')
    expect(el.classes()).toContain(tone)
  })

  it('带 role=note 提示块语义', () => {
    expect(render('warn').find('.hint-block').attributes('role')).toBe('note')
  })

  it('默认插槽内容渲染', () => {
    expect(render().text()).toContain('提示内容')
  })
})

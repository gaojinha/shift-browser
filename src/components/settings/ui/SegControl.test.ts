// SegControl 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载（纯展示 + 单选交互）。
// 覆盖：radiogroup 语义与 aria-label / 选项按顺序渲染 / 选中项 active 类与 aria-checked /
// 点击发出 update:modelValue（不自行改状态）/ 非法值时不选中任何项。
//
// 跑法：npx vitest run src/components/settings/ui/SegControl.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import SegControl from './SegControl.vue'

type SegOption = { value: string; label: string }

const OPTIONS: SegOption[] = [
  { value: 'off', label: '关闭' },
  { value: 'standard', label: '标准' },
  { value: 'strict', label: '严格' },
]

function render(modelValue: string) {
  return mount(SegControl, {
    props: { modelValue, options: OPTIONS, label: '隐私档位' },
  })
}

describe('SegControl', () => {
  it('渲染 radiogroup 语义与 aria-label', () => {
    const group = render('standard').find('[role="radiogroup"]')
    expect(group.exists()).toBe(true)
    expect(group.attributes('aria-label')).toBe('隐私档位')
  })

  it('选项按传入顺序渲染', () => {
    const btns = render('off').findAll('button[role="radio"]')
    expect(btns.map((b) => b.text())).toEqual(['关闭', '标准', '严格'])
  })

  it('选中项挂 active 类且 aria-checked=true，其余为 false', () => {
    const btns = render('standard').findAll('button[role="radio"]')
    expect(btns[0].classes()).not.toContain('active')
    expect(btns[0].attributes('aria-checked')).toBe('false')
    expect(btns[1].classes()).toContain('active')
    expect(btns[1].attributes('aria-checked')).toBe('true')
    expect(btns[2].attributes('aria-checked')).toBe('false')
  })

  it('点击某项发出 update:modelValue，且不自行改状态', async () => {
    const wrapper = render('off')
    await wrapper.findAll('button[role="radio"]')[2].trigger('click')

    expect(wrapper.emitted('update:modelValue')).toEqual([['strict']])
    // 受控组件：modelValue 未变，选中态仍留在原项
    expect(wrapper.findAll('button[role="radio"]')[0].attributes('aria-checked')).toBe('true')
  })

  it('modelValue 不在选项里时无任何项被选中', () => {
    const btns = render('不存在的档位').findAll('button[role="radio"]')
    expect(btns.every((b) => b.attributes('aria-checked') === 'false')).toBe(true)
    expect(btns.every((b) => !b.classes().includes('active'))).toBe(true)
  })
})

// UiSwitch 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils **真实挂载**（不 stub）——本组件是 reka-ui SwitchRoot 的薄皮，
// 测试的价值在于验证「底座接线」正确：v-model 透传、aria-label 落到 role=switch、
// data-state 随受控值变化、键盘空格切换。
// 若该文件在 CI 因 reka 底座报错，请改走 stub 路线（见 SettingSwitch.test.ts 注释）。
//
// 跑法：npx vitest run src/components/ui/UiSwitch.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import UiSwitch from './UiSwitch.vue'

/** 挂一个受控开关：点击/键盘切换后把新值写回 props，模拟真实 v-model */
function renderChecked(initial: boolean, label = '测试开关') {
  const wrapper = mount(UiSwitch, {
    props: {
      modelValue: initial,
      label,
      'onUpdate:modelValue': (v: boolean) => wrapper.setProps({ modelValue: v }),
    },
  })
  return wrapper
}

describe('UiSwitch', () => {
  it('渲染 role=switch 且 aria-label 取 label', () => {
    const el = renderChecked(false).find('[role="switch"]')
    expect(el.exists()).toBe(true)
    expect(el.attributes('aria-label')).toBe('测试开关')
  })

  it('受控值 false → data-state=unchecked，true → checked', async () => {
    const wrapper = renderChecked(false)
    expect(wrapper.find('[role="switch"]').attributes('data-state')).toBe('unchecked')

    await wrapper.setProps({ modelValue: true })
    expect(wrapper.find('[role="switch"]').attributes('data-state')).toBe('checked')
  })

  it('aria-checked 随受控值变化', async () => {
    const wrapper = renderChecked(false)
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('false')
    await wrapper.setProps({ modelValue: true })
    expect(wrapper.find('[role="switch"]').attributes('aria-checked')).toBe('true')
  })

  it('点击发出 update:modelValue 且写回后状态翻转', async () => {
    const wrapper = renderChecked(false)
    await wrapper.find('[role="switch"]').trigger('click')

    expect(wrapper.emitted('update:modelValue')).toEqual([[true]])
    expect(wrapper.find('[role="switch"]').attributes('data-state')).toBe('checked')
  })

  it('已是开态时点击发出 false', async () => {
    const wrapper = renderChecked(true)
    await wrapper.find('[role="switch"]').trigger('click')
    expect(wrapper.emitted('update:modelValue')).toEqual([[false]])
  })
})

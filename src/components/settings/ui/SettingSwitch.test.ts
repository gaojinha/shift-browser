// SettingSwitch 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载。真内核 UiSwitch 走 reka-ui 底座，按项目惯例
//（ShieldPanel.test.ts）用 stub 替换——本地导入的组件必须走 stubs 而非 global.components。
// 本组件自家逻辑就是 checked/change ↔ v-model 的桥接，正是要测的点。
// 覆盖：checked 透传给内核 / 内核 update:modelValue 转成 change 事件（双向）/ label 透传。
//
// 跑法：npx vitest run src/components/settings/ui/SettingSwitch.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import SettingSwitch from './SettingSwitch.vue'

/** UiSwitch stub：点击切换并透传 update:modelValue（真组件走 reka-ui 底座，测试不引入） */
const UiSwitchStub = {
  name: 'UiSwitch',
  props: { modelValue: Boolean, label: String },
  emits: ['update:modelValue'],
  template: '<button class="ui-switch-stub" :data-label="label" @click="$emit(\'update:modelValue\', !modelValue)" />',
}

function render(checked: boolean, label = '启用某某') {
  return mount(SettingSwitch, {
    props: { checked, label },
    global: { stubs: { UiSwitch: UiSwitchStub } },
  })
}

describe('SettingSwitch', () => {
  it('checked 透传给内核的 modelValue', () => {
    expect(render(true).find('.ui-switch-stub').attributes('data-label')).toBe('启用某某')
    expect(render(true).findComponent(UiSwitchStub).props('modelValue')).toBe(true)
    expect(render(false).findComponent(UiSwitchStub).props('modelValue')).toBe(false)
  })

  it('内核切到开 → 发出 change(true)', async () => {
    const wrapper = render(false)
    await wrapper.find('.ui-switch-stub').trigger('click')
    expect(wrapper.emitted('change')).toEqual([[true]])
  })

  it('内核切到关 → 发出 change(false)', async () => {
    const wrapper = render(true)
    await wrapper.find('.ui-switch-stub').trigger('click')
    expect(wrapper.emitted('change')).toEqual([[false]])
  })

  it('label 透传给内核（无障碍名）', () => {
    expect(render(false, '开启同步').find('.ui-switch-stub').attributes('data-label')).toBe('开启同步')
  })
})

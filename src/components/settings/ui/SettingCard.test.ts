// SettingCard 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载；AppIcon 是 main.ts 全局注册组件，测试环境用 stub 注入
//（对齐 PrivacySection.test.ts 模式）。
// 覆盖：标题渲染 / data-anchor 透传（设置页锚点跳转依赖）/ 有 icon 才渲染图标 /
// extra 命名插槽落在标题右侧 / 默认插槽内容渲染。
//
// 跑法：npx vitest run src/components/settings/ui/SettingCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import SettingCard from './SettingCard.vue'

/** AppIcon 在 main.ts 全局注册，测试环境无注册，stub 避免渲染失败 */
const AppIconStub = {
  name: 'AppIcon',
  props: { name: String, size: Number },
  template: '<i class="app-icon-stub" :data-name="name" />',
}

function render(props: { title: string; icon?: string; anchor?: string }) {
  return mount(SettingCard, {
    props,
    global: { components: { AppIcon: AppIconStub } },
    slots: {
      default: '卡片正文',
      extra: '右侧按钮',
    },
  })
}

describe('SettingCard', () => {
  it('渲染标题与默认插槽内容', () => {
    const wrapper = render({ title: '外观' })
    expect(wrapper.find('.card-title').text()).toContain('外观')
    expect(wrapper.text()).toContain('卡片正文')
  })

  it('anchor 透传到 data-anchor（设置页锚点跳转依赖）', () => {
    expect(render({ title: '外观', anchor: 'sec-appearance' }).find('.card').attributes('data-anchor')).toBe(
      'sec-appearance',
    )
  })

  it('不给 anchor 时 data-anchor 为空', () => {
    expect(render({ title: '外观' }).find('.card').attributes('data-anchor')).toBeUndefined()
  })

  it('给了 icon 才渲染图标，并把 name 透传给图标组件', () => {
    const withIcon = render({ title: '外观', icon: 'palette' })
    const stub = withIcon.find('.app-icon-stub')
    expect(stub.exists()).toBe(true)
    expect(stub.attributes('data-name')).toBe('palette')

    expect(render({ title: '外观' }).find('.app-icon-stub').exists()).toBe(false)
  })

  it('extra 命名插槽落在标题右侧容器里', () => {
    const wrapper = render({ title: '外观' })
    expect(wrapper.find('.card-title-extra').text()).toContain('右侧按钮')
  })
})

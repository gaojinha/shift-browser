// AppIcon 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载（纯展示组件，无 IPC / 无 store）。
// 覆盖：已知图标名注入对应 SVG path / 未知图标名不报错且不产生图元 / size 透传到
// 容器样式与 svg 宽高 / svg 带 aria-hidden（装饰性图标不进无障碍树）/ 默认 size=16。
//
// 跑法：npx vitest run src/components/AppIcon.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import AppIcon from './AppIcon.vue'

function render(name: string, size?: number) {
  return mount(AppIcon, { props: size === undefined ? { name } : { name, size } })
}

describe('AppIcon', () => {
  it('已知图标名注入对应 SVG 图元', () => {
    const wrapper = render('house')
    const svg = wrapper.find('svg')
    expect(svg.exists()).toBe(true)
    // house 是两条 path（屋顶 + 屋身）
    expect(wrapper.findAll('path')).toHaveLength(2)
  })

  it('未知图标名不报错，且不产生任何图元', () => {
    const wrapper = render('no-such-icon-name')
    expect(wrapper.find('svg').exists()).toBe(true) // 空壳仍在，避免布局抖动
    expect(wrapper.findAll('path')).toHaveLength(0)
    expect(wrapper.findAll('circle')).toHaveLength(0)
    expect(wrapper.findAll('rect')).toHaveLength(0)
  })

  it('默认 size=16，透传到容器样式与 svg 宽高', () => {
    const wrapper = render('gear')
    const style = wrapper.find('span.app-icon').attributes('style') ?? ''
    expect(style).toContain('width: 16px')
    expect(style).toContain('height: 16px')
    expect(wrapper.find('svg').attributes('width')).toBe('16')
    expect(wrapper.find('svg').attributes('height')).toBe('16')
  })

  it('自定义 size 生效', () => {
    const wrapper = render('gear', 24)
    expect(wrapper.find('svg').attributes('width')).toBe('24')
    expect(wrapper.find('span.app-icon').attributes('style') ?? '').toContain('width: 24px')
  })

  it('svg 为装饰性图标（aria-hidden），不污染无障碍树', () => {
    const wrapper = render('shield')
    expect(wrapper.find('svg').attributes('aria-hidden')).toBe('true')
  })

  it('stroke 走 currentColor，能随主题着色', () => {
    const wrapper = render('lock')
    expect(wrapper.find('svg').attributes('stroke')).toBe('currentColor')
  })
})

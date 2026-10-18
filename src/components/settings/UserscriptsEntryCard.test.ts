// UserscriptsEntryCard 组件测试（F-87 · 前端测试补课）
//
// 跑法：npx vitest run src/components/settings/UserscriptsEntryCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import UserscriptsEntryCard from './UserscriptsEntryCard.vue'

describe('UserscriptsEntryCard', () => {
  it('点击按钮 emit("open")', async () => {
    const w = mount(UserscriptsEntryCard)
    await w.find('.mini-btn.primary').trigger('click')
    expect(w.emitted('open')).toHaveLength(1)
  })

  it('描述文案渲染（Tampermonkey 风格管理入口）', () => {
    const w = mount(UserscriptsEntryCard)
    expect(w.text()).toContain('Tampermonkey 风格')
    expect(w.find('.mini-btn.primary').text()).toBe('打开用户脚本管理')
  })
})

// AutomationCard 组件测试（F-12 前端组件第四批 · 2026-10-15）
//
// 手法：mock settings store 与 useSettingsSet，聚焦模板填充与输入绑定。
// 覆盖：标题渲染、定时清理/定时 Agent 分钟数初值渲染、数字输入 change 调用 onSet,
// 三个模板按钮填充 auto_agent_prompt、提示词 textarea 双向/只读性（value 取自 store）。
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'

const getMock = vi.fn((k: string) => {
  if (k === 'auto_clear_history_days') return '3'
  if (k === 'auto_agent_minutes') return '5'
  if (k === 'auto_agent_prompt') return '旧提示'
  return ''
})
const onSetMock = vi.fn()

vi.mock('../../stores/settings', () => ({
  useSettingsStore: () => ({ get: getMock }),
}))
vi.mock('../../composables/useSettingsSet', () => ({
  useSettingsSet: () => ({ onSet: onSetMock }),
}))

const AppIconStub = {
  name: 'AppIcon',
  props: { name: String, size: Number },
  template: '<i class="app-icon-stub" />',
}
import AutomationCard from './AutomationCard.vue'

function mountCard() {
  return mount(AutomationCard, {
    global: { components: { AppIcon: AppIconStub } },
  })
}

beforeEach(() => {
  getMock.mockClear()
  onSetMock.mockClear()
})

describe('AutomationCard', () => {
  it('渲染标题与初值', () => {
    const w = mountCard()
    expect(w.text()).toContain('定时任务')
    expect((w.find('#auto-clear-history-days').element as HTMLInputElement).value).toBe('3')
    expect((w.find('#auto-agent-minutes').element as HTMLInputElement).value).toBe('5')
    expect((w.find('textarea.net-rules').element as HTMLTextAreaElement).value).toBe('旧提示')
  })

  it('定时清理输入 change 调用 onSet', async () => {
    const w = mountCard()
    const input = w.find('#auto-clear-history-days')
    await input.setValue('7')
    await input.trigger('change')
    expect(onSetMock).toHaveBeenCalledWith('auto_clear_history_days', '7')
  })

  it('Agent 分钟数 change 调用 onSet', async () => {
    const w = mountCard()
    const input = w.find('#auto-agent-minutes')
    await input.setValue('10')
    await input.trigger('change')
    expect(onSetMock).toHaveBeenCalledWith('auto_agent_minutes', '10')
  })

  it('资讯早报模板填充提示词', async () => {
    const w = mountCard()
    await w.findAll('.mini-btn')[0].trigger('click')
    expect(onSetMock).toHaveBeenCalledWith('auto_agent_prompt', expect.stringContaining('最重要的3条资讯'))
  })

  it('页面总结模板填充提示词', async () => {
    const w = mountCard()
    await w.findAll('.mini-btn')[1].trigger('click')
    expect(onSetMock).toHaveBeenCalledWith('auto_agent_prompt', expect.stringContaining('总结三条要点'))
  })

  it('到点提醒模板填充提示词', async () => {
    const w = mountCard()
    await w.findAll('.mini-btn')[2].trigger('click')
    expect(onSetMock).toHaveBeenCalledWith('auto_agent_prompt', expect.stringContaining('站起来活动一下'))
  })
})
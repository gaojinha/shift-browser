// TranslateProgress 组件测试（F-12 前端组件第二批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + 真实 i18n（文案单源断言）+ mock Tauri 事件监听
//（捕获 listen 回调，手动喂 progress 载荷）。AddressBar.test.ts 只是间接渲染本组件，
// 不驱动事件，故单独立测。
// 覆盖：未收事件不渲染 / 监听的事件名 / start·update·done·hide 四态 / pct 夹取 0..100 /
// eta 有无两种文案 / done 置 100 且 5 秒后自动收起 / fails 角标与 title 拼装 /
// engine 空回落 Google / 点击收起 / 卸载时 unlisten 并清定时器。
//
// 跑法：npx vitest run src/components/TranslateProgress.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, type VueWrapper } from '@vue/test-utils'
import { nextTick } from 'vue'

interface ProgressPayload {
  state: 'start' | 'update' | 'done' | 'hide'
  engine: string
  pct: number
  info: string
  elapsed: number
  eta: number
  fails: number
}

let listenName = ''
let progressHandler: ((e: { payload: ProgressPayload }) => void) | null = null
const unlistenMock = vi.fn()

vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, cb: (e: unknown) => void) => {
    listenName = name
    progressHandler = cb as (e: { payload: ProgressPayload }) => void
    return Promise.resolve(unlistenMock)
  },
}))

import { i18n } from '../i18n'
import { EVENTS } from '../constants'
import TranslateProgress from './TranslateProgress.vue'

function payload(over: Partial<ProgressPayload> = {}): ProgressPayload {
  return {
    state: 'update',
    engine: 'Google',
    pct: 40,
    info: '正在翻译 3/8',
    elapsed: 5,
    eta: 0,
    fails: 0,
    ...over,
  }
}

function mountTP(): VueWrapper {
  return mount(TranslateProgress, { global: { plugins: [i18n] } })
}

/** 喂一条进度事件并让 DOM 落地 */
async function feed(p: ProgressPayload) {
  progressHandler!({ payload: p })
  await nextTick()
}

beforeEach(() => {
  vi.useFakeTimers()
  listenName = ''
  progressHandler = null
  unlistenMock.mockClear()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('TranslateProgress', () => {
  it('未收到进度事件时不渲染', () => {
    const wrapper = mountTP()
    expect(wrapper.find('.tp').exists()).toBe(false)
  })

  it('监听的是 translate-progress 事件', () => {
    mountTP()
    expect(listenName).toBe(EVENTS.TRANSLATE_PROGRESS)
    expect(listenName).toBe('translate-progress')
  })

  it('start：显示「准备中…」且进度为 0', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'start', pct: 0 }))
    expect(wrapper.find('.tp').exists()).toBe(true)
    expect(wrapper.find('.tp-pct').text()).toBe('0%')
    // 第二个 .tp-info 才是时间文案栏（第一个是进度 info）
    expect(wrapper.findAll('.tp-info')[1].text()).toContain('准备中')
  })

  it('update 有 eta：文案带剩余时间', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'update', pct: 25, elapsed: 5, eta: 20 }))
    expect(wrapper.find('.tp-pct').text()).toBe('25%')
    expect(wrapper.text()).toContain(i18n.global.t('progress.runningEta', { s: 5, eta: 20 }))
  })

  it('update 无 eta：只显示已用时间', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'update', pct: 25, elapsed: 7, eta: 0 }))
    expect(wrapper.text()).toContain(i18n.global.t('progress.running', { s: 7 }))
  })

  it('pct 越界被夹到 0..100', async () => {
    const wrapper = mountTP()
    await feed(payload({ pct: 150 }))
    expect(wrapper.find('.tp-pct').text()).toBe('100%')
    await feed(payload({ pct: -20 }))
    expect(wrapper.find('.tp-pct').text()).toBe('0%')
    // 进度条宽度也按夹取值
    expect(wrapper.find('.tp-fill').attributes('style')).toContain('width: 0%')
  })

  it('done：进度置 100 并显示完成用时，5 秒后自动收起', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'done', pct: 80, elapsed: 12 }))
    expect(wrapper.find('.tp-pct').text()).toBe('100%')
    expect(wrapper.text()).toContain(i18n.global.t('progress.done', { s: 12 }))

    await vi.advanceTimersByTimeAsync(5000)
    await nextTick()
    expect(wrapper.find('.tp').exists()).toBe(false)
  })

  it('hide：立即收起', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'update' }))
    expect(wrapper.find('.tp').exists()).toBe(true)
    await feed(payload({ state: 'hide' }))
    expect(wrapper.find('.tp').exists()).toBe(false)
  })

  it('fails>0 显示未译段数角标，title 拼上 failsText', async () => {
    const wrapper = mountTP()
    await feed(payload({ info: '第 2 段', fails: 3 }))
    expect(wrapper.find('.tp-fails').text()).toBe('✕3')
    const title = wrapper.find('.tp').attributes('title') ?? ''
    expect(title).toContain(i18n.global.t('progress.fails', { n: 3 }))
    expect(title).toContain('第 2 段')
  })

  it('fails=0 不显示角标', async () => {
    const wrapper = mountTP()
    await feed(payload({ fails: 0 }))
    expect(wrapper.find('.tp-fails').exists()).toBe(false)
  })

  it('engine 为空回落 Google', async () => {
    const wrapper = mountTP()
    await feed(payload({ engine: '' }))
    expect(wrapper.find('.tp-engine').text()).toBe('Google')
  })

  it('点击收起', async () => {
    const wrapper = mountTP()
    await feed(payload({}))
    await wrapper.find('.tp').trigger('click')
    expect(wrapper.find('.tp').exists()).toBe(false)
  })

  it('卸载时解除监听并清掉收起定时器', async () => {
    const wrapper = mountTP()
    await feed(payload({ state: 'done' }))
    wrapper.unmount()
    expect(unlistenMock).toHaveBeenCalledTimes(1)
    // 定时器已清：推进时间不再触发已卸载组件的副作用（不报错即为通过）
    await vi.advanceTimersByTimeAsync(5000)
  })
})

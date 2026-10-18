// AppearanceSection 组件测试（F-87 · 前端测试补课）
//
// 手法：settings store 真实 pinia + mock invoke（set 落库与 useSettingsSet
// 副作用都走它）。reka 开关 jsdom 下 click 穿透两层（F-86 教训），开关
// 交互断言"落库被写入字符串"而非具体值。
//
// 跑法：npx vitest run src/components/settings/AppearanceSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import AppearanceSection from './AppearanceSection.vue'
import { useSettingsStore } from '../../stores/settings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

const MOUNTS: VueWrapper[] = []
function mountSection() {
  const w = mount(AppearanceSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

beforeEach(() => {
  setActivePinia(createPinia())
})

describe('AppearanceSection · 主题与书签栏', () => {
  it('主题 radio 渲染，默认深色 active；点击浅色 → onSet("theme","light") 落库', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    const radios = w.findAll('[role="radio"]')
    expect(radios).toHaveLength(2)
    expect(radios[0]!.attributes('aria-checked')).toBe('true')
    expect(radios[1]!.attributes('aria-checked')).toBe('false')
    await radios[1]!.trigger('click')
    await flushPromises()
    expect(store.settings['theme']).toBe('light')
  })

  it('显示书签栏开关：默认 checked（≠false 口径）；点击落库为字符串', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    const sw = w.find('[role="switch"]')
    expect(sw.attributes('aria-checked')).toBe('true')
    await sw.trigger('click')
    await flushPromises()
    expect(['true', 'false']).toContain(store.settings['show_bookmarks_bar'])
  })
})

describe('AppearanceSection · 启动页', () => {
  it('四个启动页开关渲染（动态背景/农历/每日一句/天气），默认全 checked', async () => {
    const w = mountSection()
    await flushPromises()
    // 全组件共 5 个开关：第 1 个是外观卡的「显示书签栏」，后 4 个是启动页卡
    const switches = w.findAll('[role="switch"]')
    expect(switches).toHaveLength(5)
    expect(switches[0]!.attributes('aria-checked')).toBe('true') // 书签栏
    expect(switches.slice(1).every((s) => s.attributes('aria-checked') === 'true')).toBe(true)
  })

  it('字体大小 select change → 落库', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    await w.find('#font-size').setValue('18')
    await flushPromises()
    expect(store.settings['font_size']).toBe('18')
  })

  it('主题色 color change 落库；恢复默认按钮 → 落空串', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    await w.find('#theme-color').setValue('#ff0000')
    await w.find('#theme-color').trigger('change')
    await flushPromises()
    expect(store.settings['theme_color']).toBe('#ff0000')
    await w.findAll('.mini-btn')[0]!.trigger('click') // 恢复默认
    await flushPromises()
    expect(store.settings['theme_color']).toBe('')
  })

  it('壁纸 / 天气城市 trim 后落库', async () => {
    const store = useSettingsStore()
    const w = mountSection()
    await flushPromises()
    const wall = w.find('#wallpaper')
    await wall.setValue('  https://img.example.com/w.jpg  ')
    await wall.trigger('change')
    const city = w.find('#weather-city')
    await city.setValue('  上海  ')
    await city.trigger('change')
    await flushPromises()
    expect(store.settings['wallpaper']).toBe('https://img.example.com/w.jpg')
    expect(store.settings['weather_city']).toBe('上海')
  })

  it('启动页保存后重开生效的提示存在（HintBlock）', async () => {
    const w = mountSection()
    await flushPromises()
    expect(w.text()).toContain('重新打开启动页生效')
  })
})

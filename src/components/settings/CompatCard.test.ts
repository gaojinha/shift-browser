// CompatCard 组件测试（F-87 · 前端测试补课）
//
// 手法：settings store 真实 pinia；mock invoke（settings.set 落库 +
// compat_chrome_fp 的 set_compat_chrome_fp 副作用）。
//
// 跑法：npx vitest run src/components/settings/CompatCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import CompatCard from './CompatCard.vue'
import { useSettingsStore } from '../../stores/settings'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }))

const MOUNTS: VueWrapper[] = []
function mountCard() {
  const w = mount(CompatCard, { attachTo: document.body })
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

describe('CompatCard · 三开关', () => {
  it('三个开关渲染；checked 口径随设置（devtools 默认关、指纹补齐默认开、DMA-BUF 默认关）', async () => {
    const w = mountCard()
    await flushPromises()
    const sw = w.findAll('[role="switch"]')
    expect(sw).toHaveLength(3)
    expect(sw[0]!.attributes('aria-checked')).toBe('false') // enable_devtools === 'true'
    expect(sw[1]!.attributes('aria-checked')).toBe('true') // compat_chrome_fp !== 'false'
    expect(sw[2]!.attributes('aria-checked')).toBe('false') // render_disable_dmabuf === 'true'
  })

  it('点击开关落库为字符串（jsdom 双触发，断言映射被写入）', async () => {
    const store = useSettingsStore()
    const w = mountCard()
    await flushPromises()
    await w.findAll('[role="switch"]')[0]!.trigger('click')
    await flushPromises()
    expect(['true', 'false']).toContain(store.settings['enable_devtools'])
  })

  it('DMA-BUF 崩溃警示文案存在（实测教训锁进 UI）', async () => {
    const w = mountCard()
    await flushPromises()
    expect(w.text()).toContain('会导致浏览器崩溃')
    expect(w.text()).toContain('改后需重启生效')
  })
})

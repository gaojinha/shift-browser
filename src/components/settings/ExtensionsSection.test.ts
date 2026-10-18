// ExtensionsSection 组件测试（F-87 · 前端测试补课）
//
// 手法：extensions store 真实 pinia 后 spy 动作（与组件共享实例）。
// 无 portal，但按 F-86 教训统一 attachTo + 卸载清理。
//
// 跑法：npx vitest run src/components/settings/ExtensionsSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ExtensionsSection from './ExtensionsSection.vue'
import { useExtensionsStore } from '../../stores/extensions'
import { useToastStore } from '../../stores/toast'

const MOUNTS: VueWrapper[] = []
function mountSection() {
  const w = mount(ExtensionsSection, { attachTo: document.body })
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

function mkExt(over: Partial<Record<string, unknown>> = {}) {
  return { id: 1, name: 'uBlock', version: '1.0', description: '广告拦截', enabled: true, js_count: 3, css_count: 1, ...over }
}

describe('ExtensionsSection · 列表', () => {
  it('挂载即 loadExtensions', async () => {
    const store = useExtensionsStore()
    const spy = vi.spyOn(store, 'loadExtensions').mockResolvedValue(undefined)
    mountSection()
    await flushPromises()
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('空列表 → 「还没有扩展」引导', async () => {
    mountSection()
    await flushPromises()
    expect(document.querySelector('.ext-empty')?.textContent).toContain('还没有扩展')
  })

  it('行渲染：名称/版本/描述/JS·CSS badge 条件显示', async () => {
    const store = useExtensionsStore()
    store.list = [mkExt(), mkExt({ id: 2, name: 'dark-reader', version: '', description: '', js_count: 0, css_count: 2 })] as never
    const w = mountSection()
    await flushPromises()
    expect(w.findAll('.ext-item')).toHaveLength(2)
    expect(w.find('.ext-name').text()).toBe('uBlock')
    expect(w.findAll('.ext-badge')).toHaveLength(3) // JS+CSS / CSS
  })
})

describe('ExtensionsSection · 加载', () => {
  it('空路径 → 行内错误，不调 store', async () => {
    const store = useExtensionsStore()
    const spy = vi.spyOn(store, 'loadExtension').mockResolvedValue(undefined)
    const w = mountSection()
    await flushPromises()
    await w.find('.ext-load-row input').setValue('   ')
    await w.find('.ext-load-row .mini-btn').trigger('click')
    await flushPromises()
    expect(w.find('.ext-error').text()).toBe('请输入扩展目录或 manifest.json 路径')
    expect(spy).not.toHaveBeenCalled()
  })

  it('加载成功：loadExtension(path) 调用 + 输入清空', async () => {
    const store = useExtensionsStore()
    vi.spyOn(store, 'loadExtension').mockResolvedValue(undefined)
    const w = mountSection()
    await flushPromises()
    await w.find('.ext-load-row input').setValue('/tmp/my-ext')
    await w.find('.ext-load-row .mini-btn').trigger('click')
    await flushPromises()
    expect(store.loadExtension).toHaveBeenCalledWith('/tmp/my-ext')
    expect((w.find('.ext-load-row input').element as HTMLInputElement).value).toBe('')
  })

  it('加载失败：errText 进 ext-error', async () => {
    const store = useExtensionsStore()
    vi.spyOn(store, 'loadExtension').mockRejectedValue(new Error('manifest 缺失'))
    const w = mountSection()
    await flushPromises()
    await w.find('.ext-load-row input').setValue('/tmp/bad')
    await w.find('.ext-load-row .mini-btn').trigger('click')
    await flushPromises()
    expect(w.find('.ext-error').text()).toContain('manifest 缺失')
  })
})

describe('ExtensionsSection · 启停与删除', () => {
  it('启停：setEnabled(id, !enabled)；失败 → toast「切换扩展状态失败」', async () => {
    const store = useExtensionsStore()
    store.list = [mkExt()] as never
    vi.spyOn(store, 'setEnabled').mockResolvedValue(undefined)
    const w = mountSection()
    await flushPromises()
    await w.find('[role="switch"]').trigger('click')
    await flushPromises()
    expect(store.setEnabled).toHaveBeenCalledWith(1, false)

    // 失败分支
    vi.spyOn(store, 'setEnabled').mockRejectedValue(new Error('x'))
    await w.find('[role="switch"]').trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '切换扩展状态失败')).toBe(true)
  })

  it('删除：deleteExtension(id)；失败 → toast「删除扩展失败」', async () => {
    const store = useExtensionsStore()
    store.list = [mkExt()] as never
    vi.spyOn(store, 'deleteExtension').mockResolvedValue(undefined)
    const w = mountSection()
    await flushPromises()
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    expect(store.deleteExtension).toHaveBeenCalledWith(1)

    vi.spyOn(store, 'deleteExtension').mockRejectedValue(new Error('x'))
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '删除扩展失败')).toBe(true)
  })
})

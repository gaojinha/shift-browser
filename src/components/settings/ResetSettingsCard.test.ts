// ResetSettingsCard 组件测试（F-87 · 前端测试补课）
//
// 手法：mock invoke（reset_settings / get_all_settings）；settings store
// 真实 pinia。确认弹窗是 UiDialog（reka portal 挂 document.body，查
// document；attachTo + 卸载清理，F-86 教训）。
//
// 跑法：npx vitest run src/components/settings/ResetSettingsCard.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ResetSettingsCard from './ResetSettingsCard.vue'
import { useSettingsStore } from '../../stores/settings'
import { useToastStore } from '../../stores/toast'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)

const MOUNTS: VueWrapper[] = []
function mountCard() {
  const w = mount(ResetSettingsCard, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(12) // reset_settings → 12 项恢复默认
})

describe('ResetSettingsCard · 重置确认流', () => {
  it('点「重置设置」弹确认框（portal：标题/不受影响文案），未确认不调命令', async () => {
    const w = mountCard()
    await flushPromises()
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    expect(document.querySelector('.ui-dialog-title')?.textContent).toBe('确认重置设置')
    expect(document.body.textContent).toContain('不受影响')
    expect(invokeMock).not.toHaveBeenCalledWith('reset_settings')
  })

  it('取消关闭弹窗不调 reset_settings', async () => {
    const w = mountCard()
    await flushPromises()
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    const cancel = Array.from(document.querySelectorAll('.ui-dialog .mini-btn')).find(
      (b) => b.textContent === '取消',
    ) as HTMLElement
    cancel.click()
    await flushPromises()
    expect(invokeMock).not.toHaveBeenCalledWith('reset_settings')
    expect(document.querySelector('.ui-dialog-title')).toBeNull()
  })

  it('确认重置：reset_settings → loadSettings 重拉 + success toast（含项数）+ 弹窗关', async () => {
    const store = useSettingsStore()
    const spy = vi.spyOn(store, 'loadSettings').mockResolvedValue(undefined)
    const w = mountCard()
    await flushPromises()
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    const ok = Array.from(document.querySelectorAll('.ui-dialog .mini-btn.danger')).pop() as HTMLElement
    ok.click()
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('reset_settings')
    expect(spy).toHaveBeenCalledTimes(1)
    expect(useToastStore().items.some((t) => t.message.includes('设置已重置（12 项恢复默认）'))).toBe(true)
    expect(document.querySelector('.ui-dialog-title')).toBeNull()
  })

  it('重置失败 → toast「重置设置失败」，弹窗不关', async () => {
    invokeMock.mockRejectedValue(new Error('db'))
    const w = mountCard()
    await flushPromises()
    await w.find('.mini-btn.danger').trigger('click')
    await flushPromises()
    const ok = Array.from(document.querySelectorAll('.ui-dialog .mini-btn.danger')).pop() as HTMLElement
    ok.click()
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '重置设置失败')).toBe(true)
    expect(document.querySelector('.ui-dialog-title')).not.toBeNull()
  })
})

// PermissionPrompt 组件测试（F-12 前端组件首批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia（toast store）+ mock Tauri IPC 与
// webview 显隐（对齐 PasswordsDialog.test.ts 模式）。
// 覆盖：无请求不渲染 / 渲染主机+权限中文名+用途建议 / 拒绝（invoke 参数 + 提示 + emit close）/
// 允许并记住（remember 透传 + 提示带「已记住」）/ invoke 失败（报错提示、不关闭、可重试）/
// busy 期间重复点击只发一次 invoke / info 变化复位 remember / webview 显隐随 info 反转。
//
// 跑法：npx vitest run src/components/PermissionPrompt.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
const setWebviewVisibleMock = vi.fn(() => Promise.resolve(true))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: (...args: unknown[]) => setWebviewVisibleMock(...args),
}))

import { useToastStore } from '../stores/toast'
import { adviceFor } from '../utils/advice'
import PermissionPrompt from './PermissionPrompt.vue'

type PermReq = { id: number; tabId: string; host: string; kind: string; kindZh: string }

const MEDIA_REQ: PermReq = {
  id: 7,
  tabId: 't1',
  host: 'meet.example',
  kind: 'media',
  kindZh: '摄像头/麦克风',
}

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(undefined)
  setWebviewVisibleMock.mockClear()
})

function mountPrompt(info: PermReq | null = null) {
  const wrapper = mount(PermissionPrompt, {
    props: { info },
    global: { plugins: [pinia] },
  })
  return { wrapper, toast: useToastStore() }
}

/** toast 文案列表 */
function messages(toast: ReturnType<typeof useToastStore>) {
  return toast.items.map((t) => t.message)
}

describe('PermissionPrompt', () => {
  it('无请求时不渲染遮罩', () => {
    const { wrapper } = mountPrompt(null)
    expect(wrapper.find('.perm-mask').exists()).toBe(false)
  })

  it('渲染主机、权限中文名与用途建议（文案取自 advice 单源）', () => {
    const { wrapper } = mountPrompt(MEDIA_REQ)
    const text = wrapper.text()
    expect(wrapper.find('.perm-mask').exists()).toBe(true)
    expect(text).toContain('meet.example')
    expect(text).toContain('摄像头/麦克风')
    expect(text).toContain(adviceFor('media').use)
    expect(text).toContain(adviceFor('media').advice)
  })

  it('拒绝：invoke 参数正确 + 提示 + 关闭', async () => {
    const { wrapper, toast } = mountPrompt(MEDIA_REQ)
    await wrapper.find('.btn-deny').trigger('click')

    expect(invokeMock).toHaveBeenCalledWith('respond_permission', {
      id: 7,
      allow: false,
      remember: false,
      kind: 'media',
      host: 'meet.example',
    })
    expect(messages(toast)).toContain('已拒绝 摄像头/麦克风')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('允许并记住：remember 透传，提示带「已记住」', async () => {
    const { wrapper, toast } = mountPrompt(MEDIA_REQ)
    await wrapper.find('.perm-remember input').setValue(true)
    await wrapper.find('.btn-allow').trigger('click')

    expect(invokeMock).toHaveBeenCalledWith('respond_permission', {
      id: 7,
      allow: true,
      remember: true,
      kind: 'media',
      host: 'meet.example',
    })
    expect(messages(toast)).toContain('已允许 摄像头/麦克风（已记住）')
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('未勾选记住时 remember 为 false', async () => {
    const { wrapper } = mountPrompt(MEDIA_REQ)
    await wrapper.find('.btn-allow').trigger('click')
    expect(invokeMock.mock.calls[0][1]).toMatchObject({ allow: true, remember: false })
  })

  it('invoke 失败：报错提示、不关闭、busy 复位可重试', async () => {
    invokeMock.mockRejectedValueOnce('主线程不可用')
    const { wrapper, toast } = mountPrompt(MEDIA_REQ)

    await wrapper.find('.btn-allow').trigger('click')
    await flushPromises()

    expect(wrapper.emitted('close')).toBeUndefined()
    expect(toast.items.some((t) => t.type === 'error' && t.message.includes('权限处理失败'))).toBe(
      true,
    )
    // busy 已复位：按钮可再次点击
    expect(wrapper.find('.btn-allow').attributes('disabled')).toBeUndefined()
  })

  it('busy 期间重复点击只发一次 invoke', async () => {
    let release: (v: unknown) => void = () => {}
    invokeMock.mockImplementationOnce(
      () =>
        new Promise((res) => {
          release = res
        }),
    )
    const { wrapper } = mountPrompt(MEDIA_REQ)
    const btn = wrapper.find('.btn-allow')

    await btn.trigger('click')
    expect(btn.attributes('disabled')).toBeDefined() // 进行中禁用
    await btn.trigger('click')
    expect(invokeMock).toHaveBeenCalledTimes(1)

    release(undefined)
    await flushPromises()
  })

  it('info 变化时 remember 复位', async () => {
    const { wrapper } = mountPrompt(MEDIA_REQ)
    await wrapper.find('.perm-remember input').setValue(true)
    expect((wrapper.find('.perm-remember input').element as HTMLInputElement).checked).toBe(true)

    await wrapper.setProps({
      info: { ...MEDIA_REQ, id: 8, host: 'maps.example', kind: 'geolocation', kindZh: '地理位置' },
    })
    await flushPromises()

    expect((wrapper.find('.perm-remember input').element as HTMLInputElement).checked).toBe(false)
  })

  it('webview 显隐随 info 反转（弹出时隐藏子 webview）', async () => {
    const { wrapper } = mountPrompt(null)
    expect(setWebviewVisibleMock).not.toHaveBeenCalled()

    await wrapper.setProps({ info: MEDIA_REQ })
    await flushPromises()
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(false, 'permission-prompt')

    await wrapper.setProps({ info: null })
    await flushPromises()
    expect(setWebviewVisibleMock).toHaveBeenLastCalledWith(true, 'permission-prompt')
  })

  it('webview 显隐失败不影响弹窗渲染', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    setWebviewVisibleMock.mockRejectedValueOnce(new Error('webview 不在'))
    const { wrapper } = mountPrompt(MEDIA_REQ)
    await wrapper.setProps({ info: { ...MEDIA_REQ, id: 9 } })
    await flushPromises()

    expect(wrapper.find('.perm-mask').exists()).toBe(true)
    expect(warn).toHaveBeenCalled()
    warn.mockRestore()
  })
})

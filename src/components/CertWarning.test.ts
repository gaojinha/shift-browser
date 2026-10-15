// CertWarning 组件测试（F-12 前端组件第二批 · 2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia（toast store）+ 真实 advice 单源
// + mock Tauri IPC 与 webview 显隐（对齐 PermissionPrompt.test.ts 模式）。
// 覆盖：无错误信息不渲染 / 主机与每条原因研判渲染 / 证书详情默认折叠可展开 /
// 「继续前往」调用 accept_cert_exception 并透传指纹、提示、关闭 / 放行失败报错且不关闭 /
// busy 期间禁用并显示「处理中…」/ 「返回」只关警告不调用放行 / webview 显隐随 info 反转。
//
// 跑法：npx vitest run src/components/CertWarning.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeOkMock = vi.fn()
vi.mock('../utils/invoke', () => ({
  invokeOk: (...args: unknown[]) => invokeOkMock(...args),
  errText: (e: unknown) => String(e),
}))
const setWebviewVisibleMock = vi.fn(() => Promise.resolve(true))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: (...args: unknown[]) => setWebviewVisibleMock(...args),
}))

import { useToastStore } from '../stores/toast'
import { verdictFor } from '../utils/advice'
import CertWarning from './CertWarning.vue'

type TlsError = {
  tabId: string
  host: string
  port: number
  flags: number
  reasons: string[]
  fingerprint: string
  subject: string
  issuer: string
  notAfter: string | null
}

const TLS_INFO: TlsError = {
  tabId: 'tab-9',
  host: 'self-signed.example',
  port: 8443,
  flags: 1,
  reasons: ['证书签发者不受信任', '证书已过期'],
  fingerprint: 'AB:CD:EF',
  subject: 'CN=self-signed.example',
  issuer: 'CN=Me, O=Home',
  notAfter: '2020-01-01T00:00:00Z',
}

let pinia: Pinia

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeOkMock.mockReset()
  invokeOkMock.mockResolvedValue(undefined)
  setWebviewVisibleMock.mockClear()
})

function mountWarning(info: TlsError | null = null) {
  const wrapper = mount(CertWarning, {
    props: { info },
    global: { plugins: [pinia] },
  })
  return { wrapper, toast: useToastStore() }
}

describe('CertWarning', () => {
  it('无错误信息时不渲染警告页', () => {
    expect(mountWarning(null).wrapper.find('.warn-mask').exists()).toBe(false)
  })

  it('渲染主机与每条错误原因的研判（文案取自 advice 单源）', () => {
    const text = mountWarning(TLS_INFO).wrapper.text()
    expect(text).toContain('self-signed.example')
    expect(text).toContain('证书签发者不受信任')
    for (const r of TLS_INFO.reasons) expect(text).toContain(verdictFor(r))
  })

  it('证书详情默认折叠，点「查看证书详情」后展开', async () => {
    const { wrapper } = mountWarning(TLS_INFO)
    expect(wrapper.find('.warn-detail').exists()).toBe(false)

    await wrapper.find('.btn-link').trigger('click')
    expect(wrapper.find('.warn-detail').exists()).toBe(true)
    const detail = wrapper.find('.warn-detail').text()
    expect(detail).toContain('self-signed.example:8443')
    expect(detail).toContain('AB:CD:EF')
    expect(detail).toContain('CN=Me, O=Home')
  })

  it('「继续前往」调用 accept_cert_exception 并透传主机/端口/指纹，然后关闭', async () => {
    const { wrapper, toast } = mountWarning(TLS_INFO)
    await wrapper.find('.btn-danger').trigger('click')
    await flushPromises()

    expect(invokeOkMock).toHaveBeenCalledWith('accept_cert_exception', {
      id: 'tab-9',
      host: 'self-signed.example',
      port: 8443,
      fingerprint: 'AB:CD:EF',
      reason: '用户在警告页选择继续前往',
    })
    expect(toast.items.some((t) => t.message.includes('已放行该证书'))).toBe(true)
    expect(wrapper.emitted('close')).toHaveLength(1)
  })

  it('放行失败：报错提示且不关闭', async () => {
    invokeOkMock.mockRejectedValueOnce('后端拒绝')
    const { wrapper, toast } = mountWarning(TLS_INFO)

    await wrapper.find('.btn-danger').trigger('click')
    await flushPromises()

    expect(wrapper.emitted('close')).toBeUndefined()
    expect(toast.items.some((t) => t.type === 'error' && t.message.includes('放行失败'))).toBe(true)
  })

  it('busy 期间按钮禁用并显示「处理中…」，重复点击只调一次', async () => {
    let release: (v: unknown) => void = () => {}
    invokeOkMock.mockImplementationOnce(
      () =>
        new Promise((res) => {
          release = res
        }),
    )
    const { wrapper } = mountWarning(TLS_INFO)
    const btn = wrapper.find('.btn-danger')

    await btn.trigger('click')
    expect(btn.attributes('disabled')).toBeDefined()
    expect(btn.text()).toContain('处理中')
    await btn.trigger('click')
    expect(invokeOkMock).toHaveBeenCalledTimes(1)

    release(undefined)
    await flushPromises()
  })

  it('「返回」只关警告，不放行证书，且恢复 webview 可见', async () => {
    const { wrapper } = mountWarning(TLS_INFO)
    await wrapper.find('.btn-primary').trigger('click')
    await flushPromises()

    expect(invokeOkMock).not.toHaveBeenCalled()
    expect(wrapper.emitted('close')).toHaveLength(1)
    expect(setWebviewVisibleMock).toHaveBeenLastCalledWith(true, 'cert-warning')
  })

  it('webview 显隐随 info 反转', async () => {
    const { wrapper } = mountWarning(null)
    expect(setWebviewVisibleMock).not.toHaveBeenCalled()

    await wrapper.setProps({ info: TLS_INFO })
    await flushPromises()
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(false, 'cert-warning')

    await wrapper.setProps({ info: null })
    await flushPromises()
    expect(setWebviewVisibleMock).toHaveBeenLastCalledWith(true, 'cert-warning')
  })

  it('无原因列表时仍能渲染主体（不崩）', () => {
    const { wrapper } = mountWarning({ ...TLS_INFO, reasons: [] })
    expect(wrapper.find('.warn-mask').exists()).toBe(true)
    expect(wrapper.find('.warn-reasons').text()).toBe('')
  })
})

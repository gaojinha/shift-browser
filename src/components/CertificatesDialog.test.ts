// CertificatesDialog 组件测试（F-76 前端测试补课）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia（tabs 种子 activeTabId）+
// mock Tauri IPC + mock utils/webview（范式照 PasswordsDialog.test.ts）。
// 覆盖：打开触发加载（隐藏 webview + 四命令）、当前证书渲染（含错误位/已放行）、
// days_left 三分支 + 7 天边界（F-66 修复的渲染侧契约）、空态、
// 事件 KIND_ZH 映射与未知 kind 回退、severity 分级、扫描临期/删记录/撤例外接线。
//
// 跑法：npx vitest run src/components/CertificatesDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

import CertificatesDialog from './CertificatesDialog.vue'
import { useTabsStore } from '../stores/tabs'
import { setWebviewVisible } from '../utils/webview'

/** UiDialog stub：只渲染默认槽，不引入 reka-ui 底座 */
const UiDialogStub = {
  name: 'UiDialog',
  props: { open: Boolean, title: String, width: String, layer: String },
  emits: ['update:open'],
  template: '<div class="ui-dialog-stub"><slot /></div>',
}

let pinia: Pinia

const CERT = {
  host: 'example.com',
  port: 443,
  fingerprint_sha256: 'aabbccddeeff00112233',
  subject: 'CN=example.com',
  issuer: 'Example Root CA',
  not_before: '2026-01-01 00:00:00',
  not_after: '2026-12-31 00:00:00',
  dns_names: ['example.com', 'www.example.com'],
  ip_addresses: ['203.0.113.7'],
  errors: 1,
  error_flags: ['ERR_SELF_SIGNED'],
  chain: ['Leaf', 'Intermediate', 'Root'],
  valid: false,
  is_exception: true,
}

const record = (host: string, days_left: number | null, over: Record<string, unknown> = {}) => ({
  host,
  port: 443,
  subject: `CN=${host}`,
  issuer: 'Issuer X',
  not_before: '2026-01-01 00:00:00',
  not_after: days_left === null ? null : '2026-12-31 00:00:00',
  fingerprint_sha256: 'ff00ff00ff00ff00ff00',
  change_count: 1,
  last_seen: '2026-10-18 10:00:00',
  days_left,
  ...over,
})

const RECORDS = [
  record('ok.com', 30),
  record('warn.com', 7),
  record('zero.com', 0),
  record('expired.com', -5, { not_after: '2026-10-13 00:00:00' }),
  record('unknown.com', null, { not_after: null, issuer: null }),
]

const EXCEPTIONS = [
  {
    host: 'selfsigned.local',
    port: 8443,
    fingerprint_sha256: '0123456789abcdef0123456789abcdef',
    added_at: '2026-10-01 09:00:00',
    reason: '内网自签',
  },
  {
    host: 'no-reason.local',
    port: 443,
    fingerprint_sha256: 'abcdefabcdefabcd',
    added_at: '2026-10-02 09:00:00',
    reason: null,
  },
]

const EVENTS = [
  { id: 1, host: 'a.com', port: 443, kind: 'first_seen', severity: 'info', detail: '首次监测', created_at: '2026-10-18 10:00:00' },
  { id: 2, host: 'b.com', port: 443, kind: 'cert_changed', severity: 'danger', detail: '指纹变更', created_at: '2026-10-18 11:00:00' },
  { id: 3, host: 'c.com', port: 443, kind: 'brand_new_kind', severity: 'warn', detail: null, created_at: '2026-10-18 12:00:00' },
]

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_tls_info') return Promise.resolve(CERT)
    if (cmd === 'list_certificates') return Promise.resolve(RECORDS)
    if (cmd === 'list_cert_exceptions') return Promise.resolve(EXCEPTIONS)
    if (cmd === 'list_cert_events') return Promise.resolve(EVENTS)
    if (cmd === 'scan_cert_expiry') return Promise.resolve(RECORDS.slice(0, 2))
    return Promise.resolve(undefined)
  })
  vi.mocked(setWebviewVisible).mockClear()
})

/** 挂载（show=false）→ 打开 → 等加载完成 */
async function openDialog(opts: { tabId?: string | null } = {}) {
  // 显式判 in：tabId=null（无活动标签）不能被 ?? 吞掉
  useTabsStore().$patch({ activeTabId: 'tabId' in opts ? opts.tabId : 'tab-1' })
  const w = mount(CertificatesDialog, {
    props: { show: false },
    global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
  })
  await w.setProps({ show: true })
  await flushPromises()
  return w
}

/** 切到指定页签并等待渲染 */
async function gotoTab(w: Awaited<ReturnType<typeof openDialog>>, label: string) {
  const btn = w.findAll('.cert-tabs button').find((b) => b.text().includes(label))!
  await btn.trigger('click')
  await flushPromises()
  return w
}

describe('打开与加载', () => {
  it('打开时隐藏 webview 并发全部四个加载命令（事件限 200 条）', async () => {
    await openDialog()
    expect(setWebviewVisible).toHaveBeenCalledWith(false, 'certs-dialog')
    expect(invokeMock).toHaveBeenCalledWith('get_tls_info', { id: 'tab-1' })
    expect(invokeMock).toHaveBeenCalledWith('list_certificates')
    expect(invokeMock).toHaveBeenCalledWith('list_cert_exceptions')
    expect(invokeMock).toHaveBeenCalledWith('list_cert_events', { limit: 200 })
  })

  it('页签计数随加载结果更新', async () => {
    const w = await openDialog()
    const labels = w.findAll('.cert-tabs button').map((b) => b.text())
    expect(labels).toEqual(['当前网页', '已监测 (5)', '例外 (2)', '事件 (3)'])
  })
})

describe('当前网页页签', () => {
  it('无活动标签时显示空态提示', async () => {
    const w = await openDialog({ tabId: null })
    expect(invokeMock).not.toHaveBeenCalledWith('get_tls_info', expect.anything())
    expect(w.find('.cert-empty').text()).toContain('当前标签没有 TLS 证书信息')
  })

  it('渲染证书全字段：不安全/已放行标签/错误位/域名/IP/证书链', async () => {
    const w = await openDialog()
    const body = w.find('.cert-info')
    expect(body.exists()).toBe(true)
    expect(body.text()).toContain('example.com:443')
    expect(body.text()).toContain('⚠️ 不安全')
    expect(body.find('.tag').text()).toBe('已放行')
    expect(body.findAll('.cert-errs div')).toHaveLength(1)
    expect(body.find('.cert-errs').text()).toContain('ERR_SELF_SIGNED')
    expect(body.text()).toContain('CN=example.com')
    expect(body.text()).toContain('Example Root CA')
    expect(body.text()).toContain('2026-01-01 00:00:00 → 2026-12-31 00:00:00')
    expect(body.text()).toContain('aabbccddeeff00112233')
    expect(body.text()).toContain('example.com, www.example.com')
    expect(body.text()).toContain('203.0.113.7')
    expect(body.text()).toContain('Leaf → Intermediate → Root')
  })
})

describe('已监测页签：days_left 渲染分支（F-66 契约）', () => {
  it('正数 → "N 天" 绿；≤7 天边界转黄；0 天也是黄', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    // class 挂在 td 内层 span 上
    const spans = w.findAll('tbody tr').map((tr) => tr.findAll('td')[3].find('span'))
    expect(spans[0].text()).toBe('30 天')
    expect(spans[0].classes()).toContain('ok')
    expect(spans[1].text()).toBe('7 天')
    expect(spans[1].classes()).toContain('warn') // 7 天边界（<=7）
    expect(spans[2].text()).toBe('0 天')
    expect(spans[2].classes()).toContain('warn')
  })

  it('负数 → "已过期" 红', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    const span = w.findAll('tbody tr')[3].findAll('td')[3].find('span')
    expect(span.text()).toBe('已过期')
    expect(span.classes()).toContain('bad')
  })

  it('null → 显示 "—"（无到期信息）', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    const cell = w.findAll('tbody tr')[4].findAll('td')[3]
    expect(cell.text()).toBe('—')
  })

  it('其余列：host:port / 签发者兜底 / 到期只取日期部分', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    const rows = w.findAll('tbody tr')
    expect(rows[0].findAll('td')[0].text()).toBe('ok.com:443')
    // issuer=null 兜底 —，not_after=null 兜底 —
    expect(rows[4].findAll('td')[1].text()).toBe('—')
    expect(rows[4].findAll('td')[2].text()).toBe('—')
    expect(rows[0].findAll('td')[2].text()).toBe('2026-12-31')
  })

  it('删除记录按 host+port 调 delete_certificate', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    await w.findAll('tbody tr')[0].find('button.btn-x').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('delete_certificate', { host: 'ok.com', port: 443 })
  })

  it('空列表显示引导空态', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      // list_* 必须返回 []（组件直接存返回值，undefined 会让模板 .length 崩）
      if (cmd.startsWith('list_')) return Promise.resolve([])
      if (cmd === 'get_tls_info') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
    const w = await gotoTab(await openDialog(), '已监测')
    expect(w.find('.cert-empty').text()).toContain('还没有监测记录')
    expect(w.find('table').exists()).toBe(false)
  })
})

describe('例外页签', () => {
  it('渲染指纹截断 16 位 + 理由兜底 "—"', async () => {
    const w = await gotoTab(await openDialog(), '例外')
    const rows = w.findAll('tbody tr')
    expect(rows[0].findAll('td')[1].text()).toBe('0123456789abcdef…')
    expect(rows[0].findAll('td')[3].text()).toBe('内网自签')
    expect(rows[1].findAll('td')[3].text()).toBe('—')
  })

  it('撤销按 host+port+fingerprint 调 delete_cert_exception', async () => {
    const w = await gotoTab(await openDialog(), '例外')
    await w.findAll('tbody tr')[0].find('button.btn-x').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('delete_cert_exception', {
      host: 'selfsigned.local',
      port: 8443,
      fingerprint: '0123456789abcdef0123456789abcdef',
    })
  })

  it('空列表显示空态', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd.startsWith('list_')) return Promise.resolve([])
      if (cmd === 'get_tls_info') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
    const w = await gotoTab(await openDialog(), '例外')
    expect(w.find('.cert-empty').text()).toBe('没有放行的证书例外')
  })
})

describe('事件页签', () => {
  it('KIND_ZH 映射已知 kind，未知 kind 回退原文', async () => {
    const w = await gotoTab(await openDialog(), '事件')
    const kinds = w.findAll('.ev-kind').map((el) => el.text())
    expect(kinds).toEqual(['首次记录', '证书变更', 'brand_new_kind'])
  })

  it('severity 分级类名 danger/warn/info', async () => {
    const w = await gotoTab(await openDialog(), '事件')
    const rows = w.findAll('.ev-row')
    expect(rows[0].classes()).toContain('sev-info')
    expect(rows[1].classes()).toContain('sev-danger')
    expect(rows[2].classes()).toContain('sev-warn')
  })

  it('空列表显示空态', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd.startsWith('list_')) return Promise.resolve([])
      if (cmd === 'get_tls_info') return Promise.resolve(null)
      return Promise.resolve(undefined)
    })
    const w = await gotoTab(await openDialog(), '事件')
    expect(w.find('.cert-empty').text()).toBe('暂无证书事件')
  })
})

describe('扫描临期', () => {
  it('按 30 天窗口调 scan_cert_expiry 后刷新', async () => {
    const w = await gotoTab(await openDialog(), '已监测')
    await w.find('button.btn-mini').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('scan_cert_expiry', { days: 30 })
    // 扫描成功后 loadAll 再跑一遍
    expect(invokeMock.mock.calls.filter((c) => c[0] === 'list_certificates')).toHaveLength(2)
  })
})

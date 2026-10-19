// ImportModule 组件测试（F-94 前端测试补课）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia + mock Tauri IPC + mock
// utils/webview（范式照 CertificatesDialog.test.ts）。UiDialog stub 只渲染
// 默认槽。覆盖：打开触发扫描（隐藏 webview + scan_browser_import）、
// 两页签源摘要、书签导入全流程回执、密码导入（含降级与失败回执）、
// toast 提示、空源与扫描失败兜底。
//
// 跑法：npx vitest run src/components/ImportModule.test.ts
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

import ImportModule from './ImportModule.vue'
import { setWebviewVisible } from '../utils/webview'
import { useToastStore } from '../stores/toast'

/** UiDialog stub：只渲染默认槽，不引入 reka-ui 底座 */
const UiDialogStub = {
  name: 'UiDialog',
  props: { open: Boolean, title: String, width: String },
  emits: ['update:open'],
  template: '<div class="ui-dialog-stub"><slot /></div>',
}

let pinia: Pinia

const SOURCE = (over: Record<string, unknown> = {}) => ({
  browser: 'chrome',
  profile: 'Default',
  kind: 'bookmarks',
  file: 'AccountBookmarks',
  count: 120,
  encrypted: false,
  decryptable: null,
  ...over,
})

const SOURCES = [
  SOURCE(),
  SOURCE({ file: 'EncryptedAccountBookmarks2', encrypted: true, count: null, decryptable: true }),
  SOURCE({ browser: 'brave', file: 'Bookmarks', count: 127 }),
  SOURCE({ kind: 'passwords', file: 'Login Data For Account', count: 69 }),
  SOURCE({ kind: 'passwords', browser: 'brave', file: 'Login Data', count: 20 }),
]

const BM_RESULT = {
  imported: 247,
  merged_away: 3,
  rejected_unsafe: 1,
  skipped_sources: [],
  used_sources: ['Default/AccountBookmarks', 'Default/EncryptedAccountBookmarks2', 'Default/Bookmarks'],
}
const PW_RESULT = {
  imported: 89,
  skipped_empty: 6,
  failed: 0,
  skipped_sources: [],
  used_sources: ['Default/Login Data For Account', 'Default/Login Data'],
  degraded_reason: null,
}
const PW_RESULT_DEGRADED = {
  ...PW_RESULT,
  imported: 20,
  failed: 69,
  degraded_reason: 'chrome: keyring 不可用（锁定或无条目），v11 密文无法解密',
}

/** F-99 预览回执（后端保证无明文字段） */
const PREVIEW = {
  passwords: [
    { browser: 'chrome', profile: 'Default', domain: 'example.com', username: 'alice' },
    { browser: 'brave', profile: 'Default', domain: 'github.com', username: null },
  ],
  bookmark_groups: [
    { folder: '书签栏', items: [{ title: '示例', url: 'https://example.com/' }] },
    {
      folder: '开发与编程',
      items: [{ title: 'MDN', url: 'https://developer.mozilla.org/' }],
    },
  ],
  skipped_sources: [] as string[],
}

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'scan_browser_import') return Promise.resolve(SOURCES)
    if (cmd === 'preview_browser_import') return Promise.resolve(PREVIEW)
    // store 刷新拉取（导入成功后 refreshAfterImport 触发）
    if (cmd === 'get_bookmarks') return Promise.resolve([])
    if (cmd === 'list_passwords') return Promise.resolve([])
    if (cmd === 'import_bookmarks_from_browser') return Promise.resolve(BM_RESULT)
    if (cmd === 'import_passwords_from_browser') return Promise.resolve(PW_RESULT)
    return Promise.resolve(undefined)
  })
  vi.mocked(setWebviewVisible).mockClear()
})

/** 挂载（show=false）→ 打开 → 等扫描完成 */
async function openDialog() {
  const w = mount(ImportModule, {
    props: { show: false },
    global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
  })
  await w.setProps({ show: true })
  await flushPromises()
  return w
}

describe('导入预览（F-99）', () => {
  it('打开时调 preview_browser_import 并按文件夹分组渲染书签预览', async () => {
    const w = await openDialog()
    expect(invokeMock).toHaveBeenCalledWith('preview_browser_import')
    const bm = w.find('[data-testid="bm-preview"]')
    expect(bm.exists()).toBe(true)
    expect(bm.text()).toContain('预览 2 条')
    expect(bm.text()).toContain('书签栏（1）')
    expect(bm.text()).toContain('开发与编程（1）')
    expect(bm.text()).toContain('MDN')
  })

  it('密码页签预览只给站点/用户名，密码列一律 ••••（无明文字段）', async () => {
    const w = await openDialog()
    const tabs = w.findAll('button.import-tab')
    await tabs[1].trigger('click')
    const pw = w.find('[data-testid="pw-preview"]')
    expect(pw.exists()).toBe(true)
    expect(pw.text()).toContain('example.com')
    expect(pw.text()).toContain('alice')
    expect(pw.text()).toContain('（无用户名）') // username=null 兜底
    expect(pw.findAll('.preview-pass')).toHaveLength(2) // 每条一个 ••••
  })

  it('预览加载失败 → 不渲染预览区、不崩（记日志）', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'scan_browser_import') return Promise.resolve(SOURCES)
      if (cmd === 'preview_browser_import') return Promise.reject(new Error('no preview'))
      return Promise.resolve(undefined)
    })
    const w = await openDialog()
    expect(w.find('[data-testid="bm-preview"]').exists()).toBe(false)
    expect(w.find('[data-testid="pw-preview"]').exists()).toBe(false)
  })
})

describe('打开与扫描', () => {
  it('打开时隐藏 webview 并调 scan_browser_import', async () => {
    await openDialog()
    expect(setWebviewVisible).toHaveBeenCalledWith(false, 'import-dialog')
    expect(invokeMock).toHaveBeenCalledWith('scan_browser_import')
  })

  it('关闭再开重新扫描（结果重置）', async () => {
    const w = await openDialog()
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(false)
    await w.setProps({ show: false })
    await w.setProps({ show: true })
    await flushPromises()
    expect(invokeMock.mock.calls.filter((c) => c[0] === 'scan_browser_import')).toHaveLength(2)
  })

  it('扫描失败 → toast 错误 + 空源兜底（不崩）', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'scan_browser_import') return Promise.reject(new Error(' boom'))
      return Promise.resolve(undefined)
    })
    const w = await openDialog()
    const toast = useToastStore()
    expect(toast.items.some((t) => t.message.includes('扫描失败'))).toBe(true)
    expect(w.text()).not.toContain('Chrome：120 条')
  })
})

describe('书签页签', () => {
  it('源摘要计数（Chrome 120 + Brave 127，加密源未解密不计）', async () => {
    const w = await openDialog()
    const summary = w.find('.src-summary').text()
    expect(summary).toContain('Chrome：120 条')
    expect(summary).toContain('Brave：127 条')
  })

  it('导入触发 import_bookmarks_from_browser 并渲染回执', async () => {
    const w = await openDialog()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('import_bookmarks_from_browser')
    const result = w.find('[data-testid="bm-result"]')
    expect(result.exists()).toBe(true)
    expect(result.text()).toContain('写入 247 条')
    expect(result.text()).toContain('合并去重 3 条')
    expect(result.text()).toContain('拒收不安全链接 1 条')
  })

  it('导入失败 → toast 错误，无回执', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'import_bookmarks_from_browser') return Promise.reject(new Error('db down'))
      return Promise.resolve(SOURCES)
    })
    const w = await openDialog()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    const toast = useToastStore()
    expect(toast.items.some((t) => t.message.includes('书签导入失败'))).toBe(true)
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(false)
  })

  it('空源摘要显示「未发现」，按钮仍可点（后端返回零导入）', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'scan_browser_import') return Promise.resolve([])
      if (cmd === 'import_bookmarks_from_browser')
        return Promise.resolve({ imported: 0, merged_away: 0, rejected_unsafe: 0, skipped_sources: [], used_sources: [] })
      return Promise.resolve(undefined)
    })
    const w = await openDialog()
    expect(w.find('.src-summary').text()).toContain('未发现')
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="bm-result"]').text()).toContain('写入 0 条')
  })
})

describe('密码页签', () => {
  async function gotoPasswords(w: Awaited<ReturnType<typeof openDialog>>) {
    const tabs = w.findAll('button.import-tab')
    await tabs[1].trigger('click')
    await flushPromises()
  }

  it('切换页签显示密码源摘要与警示语', async () => {
    const w = await openDialog()
    await gotoPasswords(w)
    const summary = w.find('.src-summary').text()
    expect(summary).toContain('Chrome：69 条')
    expect(summary).toContain('Brave：20 条')
    expect(w.find('.import-warn').text()).toContain('明文不落盘')
  })

  it('导入触发 import_passwords_from_browser 并渲染回执', async () => {
    const w = await openDialog()
    await gotoPasswords(w)
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('import_passwords_from_browser')
    const result = w.find('[data-testid="pw-result"]')
    expect(result.exists()).toBe(true)
    expect(result.text()).toContain('写入 89 条')
    expect(result.text()).toContain('空密码跳过 6 条')
  })

  it('keyring 降级回执：degraded_reason 可见', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'import_passwords_from_browser') return Promise.resolve(PW_RESULT_DEGRADED)
      return Promise.resolve(SOURCES)
    })
    const w = await openDialog()
    await gotoPasswords(w)
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    const result = w.find('[data-testid="pw-result"]')
    expect(result.text()).toContain('写入 20 条')
    expect(result.text()).toContain('keyring 不可用')
  })

  it('导入失败 → toast 错误，无回执', async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'import_passwords_from_browser') return Promise.reject(new Error('vault locked'))
      return Promise.resolve(SOURCES)
    })
    const w = await openDialog()
    await gotoPasswords(w)
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    const toast = useToastStore()
    expect(toast.items.some((t) => t.message.includes('密码导入失败'))).toBe(true)
    expect(w.find('[data-testid="pw-result"]').exists()).toBe(false)
  })
})

describe('两块互不耦合', () => {
  it('书签导入后切密码页签：无书签回执泄漏，密码回执独立', async () => {
    const w = await openDialog()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(true)

    const tabs = w.findAll('button.import-tab')
    await tabs[1].trigger('click')
    await flushPromises()
    // 密码页签不渲染书签回执
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(false)
    // 密码导入独立出回执
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="pw-result"]').text()).toContain('写入 89 条')
  })

  it('关闭时重置两块回执', async () => {
    const w = await openDialog()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(true)
    await w.setProps({ show: false })
    await w.setProps({ show: true })
    await flushPromises()
    expect(w.find('[data-testid="bm-result"]').exists()).toBe(false)
    expect(w.find('[data-testid="pw-result"]').exists()).toBe(false)
  })
})

describe('导入成功后刷新 store（派单要求）', () => {
  it('书签导入 → get_bookmarks；密码导入 → list_passwords，各自独立触发', async () => {
    const w = await openDialog()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('get_bookmarks')
    expect(invokeMock).not.toHaveBeenCalledWith('list_passwords')

    const tabs = w.findAll('button.import-tab')
    await tabs[1].trigger('click')
    await flushPromises()
    await w.find('button.import-run').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('list_passwords')
  })
})

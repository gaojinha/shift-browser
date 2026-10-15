// C-2 密码库搜索框组件测试（2026-10-15）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子）+ mock Tauri IPC
//（对齐 ShieldPanel.test.ts 模式）。UiDialog stub 只渲染默认槽（搜索框 + 列表 + 空态）。
// 覆盖：搜索框渲染（aria-label）、按域名/用户名/备注客户端实时过滤（不区分大小写）、
// 「无匹配」空态、「还没有保存的密码」空态、无搜索词显示全部。
//
// 跑法：npx vitest run src/components/PasswordsDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import type { PasswordMeta } from '../types/password'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

import { usePasswordsStore } from '../stores/passwords'
import PasswordsDialog from './PasswordsDialog.vue'

/** UiDialog stub：只渲染默认槽，不引入 reka-ui 底座 */
const UiDialogStub = {
  name: 'UiDialog',
  props: { open: Boolean, title: String, width: String },
  emits: ['update:open'],
  template: '<div class="ui-dialog-stub"><slot /></div>',
}

let pinia: Pinia

const ENTRIES: PasswordMeta[] = [
  { id: 1, domain: 'example.com', username: 'alice', notes: 'work account', created_at: '', updated_at: '' },
  { id: 2, domain: 'github.com', username: 'bob', notes: null, created_at: '', updated_at: '' },
  { id: 3, domain: 'example.org', username: null, notes: 'personal', created_at: '', updated_at: '' },
]

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'list_passwords') return Promise.resolve(ENTRIES)
    return Promise.resolve(undefined)
  })
})

function mountDialog(entries: PasswordMeta[] = ENTRIES) {
  usePasswordsStore().$patch({ list: entries, loaded: true })
  return mount(PasswordsDialog, {
    props: { show: true },
    global: { plugins: [pinia], stubs: { UiDialog: UiDialogStub } },
  })
}

const domains = (w: ReturnType<typeof mountDialog>) =>
  w.findAll('.item-domain').map((el) => el.text())

describe('C-2 密码库搜索框', () => {
  it('渲染搜索框（带 aria-label）', () => {
    const w = mountDialog()
    const input = w.find('.search-input')
    expect(input.exists()).toBe(true)
    expect(input.attributes('aria-label')).toBe('搜索密码')
  })

  it('无搜索词时显示全部', async () => {
    const w = mountDialog()
    await w.vm.$nextTick()
    expect(domains(w)).toEqual(['example.com', 'github.com', 'example.org'])
  })

  it('按域名实时过滤（不区分大小写）', async () => {
    const w = mountDialog()
    await w.find('.search-input').setValue('EXAMPLE')
    await w.vm.$nextTick()
    expect(domains(w)).toEqual(['example.com', 'example.org'])
  })

  it('按用户名过滤', async () => {
    const w = mountDialog()
    await w.find('.search-input').setValue('bob')
    await w.vm.$nextTick()
    expect(domains(w)).toEqual(['github.com'])
  })

  it('按备注过滤', async () => {
    const w = mountDialog()
    await w.find('.search-input').setValue('personal')
    await w.vm.$nextTick()
    expect(domains(w)).toEqual(['example.org'])
  })

  it('无匹配时显示「无匹配」空态', async () => {
    const w = mountDialog()
    await w.find('.search-input').setValue('zzz-not-found')
    await w.vm.$nextTick()
    expect(w.findAll('.item')).toHaveLength(0)
    expect(w.find('.empty').text()).toBe('无匹配')
  })

  it('无密码时显示「还没有保存的密码」空态', async () => {
    const w = mountDialog([])
    await w.vm.$nextTick()
    expect(w.find('.empty').text()).toBe('还没有保存的密码')
  })
})

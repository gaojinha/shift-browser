// UserscriptsDialog 组件测试（F-85 · 前端测试补课）
//
// 手法：mock invoke 与 webview 工具；toast 走真实 store 断言 items。
// 弹窗正文在 UiDialog 的 reka portal 里挂 document.body——查询/触发一律走
// DOMWrapper(document)（F-88/F-84 同款坑）。
//
// 跑法：npx vitest run src/components/UserscriptsDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import UserscriptsDialog from './UserscriptsDialog.vue'
import { useToastStore } from '../stores/toast'
import { setWebviewVisible } from '../utils/webview'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('../utils/webview', () => ({ setWebviewVisible: vi.fn().mockResolvedValue(undefined) }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)
const webviewMock = vi.mocked(setWebviewVisible)

function mkScript(over: Partial<Record<string, unknown>> = {}) {
  return {
    id: 1,
    name: '去广告',
    matches: '*://*.example.com/*',
    code: '// code',
    enabled: true,
    created_at: '2026-10-18T09:00:00',
    ...over,
  }
}

function q(sel: string): DOMWrapper<Element> {
  const el = document.querySelector(sel)
  expect(el, `document 上找不到 ${sel}`).toBeTruthy()
  return new DOMWrapper(el!)
}
function qa(sel: string): DOMWrapper<Element>[] {
  return Array.from(document.querySelectorAll(sel)).map((el) => new DOMWrapper(el))
}

async function openDialog() {
  const w = mount(UserscriptsDialog, { props: { show: false }, attachTo: document.body })
  await w.setProps({ show: true })
  await flushPromises()
  await flushPromises()
  return w
}

beforeEach(() => {
  setActivePinia(createPinia())
  invokeMock.mockReset()
  webviewMock.mockClear()
  invokeMock.mockResolvedValue([])
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('UserscriptsDialog · 打开与列表', () => {
  it('打开：webview 隐藏 + list_userscripts 拉取 + 名称/matches 渲染', async () => {
    invokeMock.mockResolvedValue([mkScript()])
    await openDialog()
    expect(invokeMock).toHaveBeenCalledWith('list_userscripts')
    expect(webviewMock).toHaveBeenCalledWith(false, 'userscripts-dialog')
    expect(q('.us-name').text()).toBe('去广告')
    expect(q('.us-match').text()).toBe('*://*.example.com/*')
  })

  it('空列表 → 「还没有用户脚本」', async () => {
    await openDialog()
    expect(q('.us-empty').text()).toBe('还没有用户脚本')
  })

  it('list_userscripts 拒绝 → toast「加载用户脚本失败」不崩', async () => {
    invokeMock.mockRejectedValue(new Error('db'))
    await openDialog()
    expect(useToastStore().items.some((t) => t.message === '加载用户脚本失败')).toBe(true)
  })

  it('关闭时恢复 webview', async () => {
    const w = await openDialog()
    await w.setProps({ show: false })
    await flushPromises()
    expect(webviewMock).toHaveBeenLastCalledWith(true, 'userscripts-dialog')
  })
})

describe('UserscriptsDialog · 新建与保存', () => {
  it('名称为空 → toast「名称与代码不能为空」，不调 add_userscript', async () => {
    await openDialog()
    await q('.us-code').setValue('// code')
    await qa('.us-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '名称与代码不能为空')).toBe(true)
    expect(invokeMock).not.toHaveBeenCalledWith('add_userscript', expect.anything())
  })

  it('代码为空 → 同样拒绝', async () => {
    await openDialog()
    await qa('.us-input')[0]!.setValue('有名字')
    await qa('.us-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message === '名称与代码不能为空')).toBe(true)
  })

  it('新建成功：add_userscript 带全参、表单重置、列表重拉、success toast', async () => {
    invokeMock.mockResolvedValueOnce([]).mockResolvedValueOnce([mkScript()]) // add → list
    await openDialog()
    await qa('.us-input')[0]!.setValue('新脚本')
    await qa('.us-input')[1]!.setValue('*://*.test.io/*')
    await q('.us-code').setValue('alert(1)')
    await qa('.us-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('add_userscript', {
      name: '新脚本',
      matches: '*://*.test.io/*',
      code: 'alert(1)',
    })
    expect((q('.us-input').element as HTMLInputElement).value).toBe('')
    expect(useToastStore().items.some((t) => t.message.includes('已保存'))).toBe(true)
  })

  it('保存失败 → toast「保存失败：…」', async () => {
    await openDialog()
    invokeMock.mockRejectedValueOnce(new Error('db')) // add（须在 openDialog 之后入队，否则被 list 消费）
    await qa('.us-input')[0]!.setValue('脚本')
    await q('.us-code').setValue('x')
    await qa('.us-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(useToastStore().items.some((t) => t.message.startsWith('保存失败：'))).toBe(true)
  })
})

describe('UserscriptsDialog · 编辑 / 启用 / 删除', () => {
  it('点条目进编辑：表单回填、按钮变「保存修改」、保存走 update_userscript', async () => {
    invokeMock.mockResolvedValue([mkScript()])
    await openDialog()
    await q('.us-meta').trigger('click')
    await flushPromises()
    expect((q('.us-input').element as HTMLInputElement).value).toBe('去广告')
    expect(qa('.us-btn.primary').pop()!.text()).toContain('保存修改')
    invokeMock.mockClear()
    invokeMock.mockResolvedValue([mkScript()])
    await qa('.us-btn.primary').pop()!.trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith(
      'update_userscript',
      expect.objectContaining({ id: 1, name: '去广告', enabled: null }),
    )
  })

  it('取消编辑：表单重置回新建态', async () => {
    invokeMock.mockResolvedValue([mkScript()])
    await openDialog()
    await q('.us-meta').trigger('click')
    await flushPromises()
    await qa('.us-btn').filter((b) => b.text() === '取消编辑')[0]!.trigger('click')
    await flushPromises()
    expect((q('.us-input').element as HTMLInputElement).value).toBe('')
    expect(qa('.us-btn.primary').pop()!.text()).toContain('添加')
  })

  it('启用开关：update_userscript enabled 取反 + 重拉', async () => {
    invokeMock.mockResolvedValueOnce([mkScript()])
    await openDialog()
    invokeMock.mockClear()
    invokeMock.mockResolvedValue([mkScript({ enabled: false })])
    await q('input[type="checkbox"]').setValue(false)
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith(
      'update_userscript',
      expect.objectContaining({ id: 1, enabled: false, name: null }),
    )
  })

  it('删除：delete_userscript + 重拉；删正在编辑的 → 表单重置', async () => {
    invokeMock.mockResolvedValue([mkScript()])
    await openDialog()
    await q('.us-meta').trigger('click') // 正在编辑 id=1
    invokeMock.mockClear()
    invokeMock.mockResolvedValue([])
    await q('.us-btn.danger').trigger('click')
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('delete_userscript', { id: 1 })
    expect((q('.us-input').element as HTMLInputElement).value).toBe('')
  })
})

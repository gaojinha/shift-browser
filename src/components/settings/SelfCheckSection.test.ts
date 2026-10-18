// SelfCheckSection 组件测试（F-87 · 前端测试补课）
//
// 手法：mock invoke（selfcheck_cmd）；fixFor 用真实 utils/advice（纯映射，
// 只断言 fix 行存在不锁具体文案）。
//
// 跑法：npx vitest run src/components/settings/SelfCheckSection.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, type VueWrapper } from '@vue/test-utils'
import SelfCheckSection from './SelfCheckSection.vue'

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))

import { invoke } from '@tauri-apps/api/core'
const invokeMock = vi.mocked(invoke)

const MOUNTS: VueWrapper[] = []
function mountSection() {
  const w = mount(SelfCheckSection, { attachTo: document.body })
  MOUNTS.push(w)
  return w
}
afterEach(() => {
  while (MOUNTS.length) MOUNTS.pop()!.unmount()
  document.body.innerHTML = ''
})

function item(name: string, ok: boolean, detail = 'detail') {
  return { name, ok, detail }
}
function report(over: Record<string, unknown> = {}) {
  return {
    healthy: true,
    timestamp: 1,
    versions: [item('app 版本', true)],
    storage: [item('数据库', true)],
    runtime: [item('webview', true)],
    capabilities: [item('网络栈', true)],
    ...over,
  }
}

beforeEach(() => {
  invokeMock.mockReset()
  invokeMock.mockResolvedValue(report())
})

describe('SelfCheckSection · 自检渲染', () => {
  it('挂载自动跑一次 selfcheck_cmd；healthy → 「✅ 一切正常」ok 总结', async () => {
    const w = mountSection()
    await flushPromises()
    expect(invokeMock).toHaveBeenCalledWith('selfcheck_cmd')
    expect(w.find('.selfcheck-summary').text()).toContain('一切正常')
    expect(w.find('.selfcheck-summary').classes()).toContain('ok')
  })

  it('四分组标题渲染；行 ✅ mark + 名称 + detail', async () => {
    const w = mountSection()
    await flushPromises()
    const titles = w.findAll('.selfcheck-group-title').map((n) => n.text())
    expect(titles).toEqual(['版本与环境', '数据存储', '运行时', '能力状态'])
    expect(w.find('.selfcheck-mark').text()).toBe('✅')
    expect(w.find('.selfcheck-name').text()).toBe('app 版本')
    expect(w.find('.selfcheck-detail').text()).toBe('detail')
  })

  it('有异常项：bad 总结 + ❌ mark + fix 行（→ name：处置建议）', async () => {
    invokeMock.mockResolvedValue(report({
      healthy: false,
      storage: [item('数据库', false, '无法写入')],
    }))
    const w = mountSection()
    await flushPromises()
    expect(w.find('.selfcheck-summary').text()).toContain('发现异常项')
    expect(w.find('.selfcheck-summary').classes()).toContain('bad')
    const marks = w.findAll('.selfcheck-mark').map((n) => n.text())
    expect(marks).toContain('❌')
    const fix = w.find('.selfcheck-fix')
    expect(fix.text()).toContain('→ 数据库：')
    expect(fix.text().length).toBeGreaterThan('→ 数据库：'.length) // fixFor 有内容
  })

  it('自检失败：错误区显示 + 重新检查可再调', async () => {
    invokeMock.mockRejectedValue(new Error('自检后端未起'))
    const w = mountSection()
    await flushPromises()
    expect(w.find('.selfcheck-error').text()).toContain('自检后端未起')
    invokeMock.mockResolvedValue(report())
    await w.find('.selfcheck-btn').trigger('click')
    await flushPromises()
    expect(w.find('.selfcheck-summary').exists()).toBe(true)
    expect(w.find('.selfcheck-error').exists()).toBe(false)
  })

  it('检查中按钮禁用且文案「检查中…」（受控 promise）', async () => {
    invokeMock.mockReturnValue(new Promise(() => {}))
    const w = mountSection()
    await flushPromises()
    const btn = w.find('.selfcheck-btn')
    expect(btn.text()).toBe('检查中…')
    expect(btn.attributes('disabled')).toBeDefined()
  })
})

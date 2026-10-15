// SpaceSwitcher 组件测试（F-12 前端组件第三批 · 2026-10-15）
//
// 手法：mock workspaces store 模块（其 switchWorkspace 深度耦合 tabs store 与真实
// 标签会话，端到端不属本组件职责），聚焦本组件自身的交互与副作用：
// 展开/收起、webview 显隐、点选空间、新建、行内重命名、删除确认、点击外部关闭、
// 卸载时解绑 document 监听。
//
// 跑法：npx vitest run src/components/SpaceSwitcher.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { reactive, nextTick } from 'vue'

type Ws = { id: number; name: string; color: string; sort_order: number; created_at: string }

const fakeWs = reactive({
  currentId: null as number | null,
  currentName: '默认',
  currentColor: '',
  workspaces: [] as Ws[],
  switchWorkspace: vi.fn(),
  createWorkspace: vi.fn(async (): Promise<number | null> => 7),
  renameWorkspace: vi.fn(async () => {}),
  deleteWorkspace: vi.fn(async () => {}),
})

vi.mock('../stores/workspaces', () => ({
  useWorkspacesStore: () => fakeWs,
}))

const setWebviewVisibleMock = vi.fn(() => Promise.resolve(true))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: (...args: unknown[]) => setWebviewVisibleMock(...args),
}))

import SpaceSwitcher from './SpaceSwitcher.vue'

function ws(id: number, name: string, color = ''): Ws {
  return { id, name, color, sort_order: id, created_at: '2026-10-15' }
}

async function open() {
  const wrapper = mount(SpaceSwitcher)
  await wrapper.find('.space-btn').trigger('click')
  return wrapper
}

beforeEach(() => {
  fakeWs.currentId = null
  fakeWs.currentName = '默认'
  fakeWs.currentColor = ''
  fakeWs.workspaces = []
  fakeWs.switchWorkspace.mockClear()
  fakeWs.createWorkspace.mockClear()
  fakeWs.renameWorkspace.mockClear()
  fakeWs.deleteWorkspace.mockClear()
  setWebviewVisibleMock.mockClear()
})

describe('SpaceSwitcher', () => {
  it('按钮显示当前空间名与无障碍名', () => {
    fakeWs.currentId = 1
    fakeWs.currentName = '工作'
    const wrapper = mount(SpaceSwitcher)
    expect(wrapper.find('.space-name').text()).toBe('工作')
    expect(wrapper.find('.space-btn').attributes('aria-label')).toBe('空间切换：工作')
  })

  it('默认不展开菜单', () => {
    expect(mount(SpaceSwitcher).find('.space-menu').exists()).toBe(false)
  })

  it('展开菜单并隐藏子 webview；再点收起并恢复', async () => {
    const wrapper = await open()
    expect(wrapper.find('.space-menu').exists()).toBe(true)
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(false, 'space-switcher')

    await wrapper.find('.space-btn').trigger('click')
    expect(wrapper.find('.space-menu').exists()).toBe(false)
    expect(setWebviewVisibleMock).toHaveBeenLastCalledWith(true, 'space-switcher')
  })

  it('菜单包含默认项、各空间与新建入口', async () => {
    fakeWs.workspaces = [ws(1, '工作', '#f00'), ws(2, '娱乐', '#0f0')]
    const wrapper = await open()
    const names = wrapper.findAll('.space-item-name').map((n) => n.text())
    expect(names).toContain('默认')
    expect(names).toContain('工作')
    expect(names).toContain('娱乐')
    expect(wrapper.find('.space-item.add').text()).toContain('新建空间')
  })

  it('当前空间项高亮（默认空间以 currentId === null 判定）', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    fakeWs.currentId = 1
    const wrapper = await open()
    const items = wrapper.findAll('.space-item')
    expect(items[0].classes()).not.toContain('active') // 默认项
    expect(items[1].classes()).toContain('active') // 工作
  })

  it('点选某空间 → 切换并关闭菜单', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    const wrapper = await open()
    await wrapper.findAll('.space-item-name')[1].trigger('click')

    expect(fakeWs.switchWorkspace).toHaveBeenCalledWith(1)
    expect(wrapper.find('.space-menu').exists()).toBe(false)
  })

  it('点默认项 → switchWorkspace(null)', async () => {
    fakeWs.currentId = 1
    fakeWs.currentName = '工作'
    const wrapper = await open()
    await wrapper.findAll('.space-item')[0].trigger('click')
    expect(fakeWs.switchWorkspace).toHaveBeenCalledWith(null)
  })

  it('新建空间 → 创建后切到新空间并关闭', async () => {
    const wrapper = await open()
    await wrapper.find('.space-item.add').trigger('click')
    await flushPromises()

    expect(fakeWs.createWorkspace).toHaveBeenCalledTimes(1)
    expect(fakeWs.switchWorkspace).toHaveBeenCalledWith(7)
    expect(wrapper.find('.space-menu').exists()).toBe(false)
  })

  it('行内重命名：预填原名，回车提交并去空格', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    const wrapper = await open()

    await wrapper.find('[aria-label="重命名空间"]').trigger('click')
    const input = wrapper.find('.space-edit')
    expect(input.exists()).toBe(true)
    expect((input.element as HTMLInputElement).value).toBe('工作')

    await input.setValue('  新名字  ')
    await input.trigger('keydown.enter')
    await flushPromises()

    expect(fakeWs.renameWorkspace).toHaveBeenCalledWith(1, '新名字')
    expect(wrapper.find('.space-edit').exists()).toBe(false)
  })

  it('重命名留空则不提交', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    const wrapper = await open()
    await wrapper.find('[aria-label="重命名空间"]').trigger('click')
    await wrapper.find('.space-edit').setValue('   ')
    await wrapper.find('.space-edit').trigger('keydown.enter')
    await flushPromises()
    expect(fakeWs.renameWorkspace).not.toHaveBeenCalled()
  })

  it('重命名按 Esc 取消，不提交', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    const wrapper = await open()
    await wrapper.find('[aria-label="重命名空间"]').trigger('click')
    await wrapper.find('.space-edit').trigger('keydown.esc')
    await nextTick()
    expect(wrapper.find('.space-edit').exists()).toBe(false)
    expect(fakeWs.renameWorkspace).not.toHaveBeenCalled()
  })

  it('删除需二次确认', async () => {
    fakeWs.workspaces = [ws(1, '工作')]
    const wrapper = await open()

    await wrapper.find('[aria-label="删除空间"]').trigger('click')
    expect(fakeWs.deleteWorkspace).not.toHaveBeenCalled() // 只进入确认态

    await wrapper.find('[aria-label="确认删除"]').trigger('click')
    await flushPromises()
    expect(fakeWs.deleteWorkspace).toHaveBeenCalledWith(1)
  })

  it('点菜单外部关闭菜单并恢复 webview', async () => {
    const wrapper = await open()
    setWebviewVisibleMock.mockClear()

    document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
    await nextTick()
    await flushPromises()

    expect(wrapper.find('.space-menu').exists()).toBe(false)
    expect(setWebviewVisibleMock).toHaveBeenCalledWith(true, 'space-switcher')
  })

  it('卸载时解绑 document mousedown 监听', async () => {
    const removeSpy = vi.spyOn(document, 'removeEventListener')
    const wrapper = await open()
    wrapper.unmount()
    expect(removeSpy).toHaveBeenCalledWith('mousedown', expect.any(Function))
    removeSpy.mockRestore()
  })
})

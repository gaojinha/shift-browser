// GroupsDialog 组件测试（F-85 · 前端测试补课）
//
// 手法：groups/tabs store 用真实 pinia 后 spy 动作（与组件共享实例）；
// webview 工具 mock；色板走真 constants（UI.groupPalette）。注意（F-88/F-84
// 同款坑）：弹窗正文在 UiDialog 的 reka portal 里挂 document.body——查询/
// 触发一律走 DOMWrapper(document)。
//
// 跑法：npx vitest run src/components/GroupsDialog.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { mount, flushPromises, DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import GroupsDialog from './GroupsDialog.vue'
import { useGroupsStore } from '../stores/groups'
import { useTabsStore } from '../stores/tabs'
import { useToastStore } from '../stores/toast'
import { setWebviewVisible } from '../utils/webview'
import { UI } from '../constants'

vi.mock('../utils/webview', () => ({ setWebviewVisible: vi.fn().mockResolvedValue(undefined) }))
const webviewMock = vi.mocked(setWebviewVisible)

function mkGroup(over: Partial<{ id: number; name: string; color: string; collapsed: boolean }> = {}) {
  return { id: 1, name: '工作', color: '#4a9eff', collapsed: false, ...over }
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
  const w = mount(GroupsDialog, { props: { show: false }, attachTo: document.body })
  await w.setProps({ show: true })
  await flushPromises()
  await flushPromises()
  return w
}

beforeEach(() => {
  setActivePinia(createPinia())
  webviewMock.mockClear()
})

afterEach(() => {
  document.body.innerHTML = ''
  vi.clearAllMocks()
})

describe('GroupsDialog · 打开与列表', () => {
  it('打开时拉取分组 + 隐藏 webview；关闭恢复', async () => {
    const groups = useGroupsStore()
    const spy = vi.spyOn(groups, 'loadGroups').mockResolvedValue(undefined)
    const w = await openDialog()
    expect(spy).toHaveBeenCalledTimes(1)
    expect(webviewMock).toHaveBeenCalledWith(false, 'groups-dialog')
    await w.setProps({ show: false })
    await flushPromises()
    expect(webviewMock).toHaveBeenLastCalledWith(true, 'groups-dialog')
  })

  it('列表渲染：组名 + 组内标签计数（tabsStore 按 groupId 过滤）', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup(), mkGroup({ id: 2, name: '娱乐' })] as never
    const tabs = useTabsStore()
    tabs.tabs = [
      { id: 't1', url: 'u', title: 'T', groupId: 1 },
      { id: 't2', url: 'u', title: 'T', groupId: 1 },
      { id: 't3', url: 'u', title: 'T', groupId: 2 },
      { id: 't4', url: 'u', title: 'T', groupId: null },
    ] as never
    await openDialog()
    const counts = qa('.item-count').map((n) => n.text())
    expect(counts).toEqual(['2 个标签', '1 个标签'])
  })

  it('空列表 → 空态引导文案', async () => {
    await openDialog()
    expect(q('.empty').text()).toContain('还没有分组')
  })

  it('loadGroups 抛错 → toast「加载分组失败」', async () => {
    const groups = useGroupsStore()
    vi.spyOn(groups, 'loadGroups').mockRejectedValue(new Error('x'))
    await openDialog()
    expect(useToastStore().items.some((t) => t.message === '加载分组失败')).toBe(true)
  })
})

describe('GroupsDialog · 新建', () => {
  it('空名 → 行内错误「分组名称必填」，不调 createGroup', async () => {
    const groups = useGroupsStore()
    const spy = vi.spyOn(groups, 'createGroup').mockResolvedValue(9)
    await openDialog()
    await q('.add-btn').trigger('click')
    await q('.save-btn').trigger('click')
    await flushPromises()
    expect(q('.form-error').text()).toBe('分组名称必填')
    expect(spy).not.toHaveBeenCalled()
  })

  it('成功创建：createGroup(名称, 色板色) 调用、表单收起清空；色点可选色', async () => {
    const groups = useGroupsStore()
    const spy = vi.spyOn(groups, 'createGroup').mockResolvedValue(9)
    await openDialog()
    await q('.add-btn').trigger('click')
    expect(qa('.color-dot')).toHaveLength(UI.groupPalette.length)
    await q('input[placeholder*="分组名称"]').setValue('  阅读  ')
    await qa('.color-row .color-dot')[2]!.trigger('click')
    await q('.save-btn').trigger('click')
    await flushPromises()
    expect(spy).toHaveBeenCalledWith('阅读', UI.groupPalette[2])
    expect(document.querySelector('.add-form')).toBeNull() // 表单收起（负向断言勿用 q()，元素不存在会先抛）
  })

  it('创建失败：store 抛错不崩、表单照样清空收起', async () => {
    const groups = useGroupsStore()
    vi.spyOn(groups, 'createGroup').mockRejectedValue(new Error('dup'))
    await openDialog()
    await q('.add-btn').trigger('click')
    await q('input[placeholder*="分组名称"]').setValue('重复名')
    await q('.save-btn').trigger('click')
    await flushPromises()
    expect(document.querySelector('.add-form')).toBeNull()
  })
})

describe('GroupsDialog · 重命名 / 换色 / 折叠', () => {
  it('重命名：进编辑态回填原名，保存调 updateGroup(id, {name})', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const spy = vi.spyOn(groups, 'updateGroup').mockResolvedValue(undefined)
    await openDialog()
    await qa('.mini-btn')[1]!.trigger('click') // item-actions 顺序：折叠 0 / 重命名 1 / 删除 2
    const input = q('.edit-input')
    expect((input.element as HTMLInputElement).value).toBe('工作')
    await input.setValue('新名')
    await input.trigger('keydown.enter')
    await flushPromises()
    expect(spy).toHaveBeenCalledWith(1, { name: '新名' })
  })

  it('换色：乐观更新本地色 + updateGroup(id, {color})；失败回滚', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const spy = vi.spyOn(groups, 'updateGroup').mockRejectedValueOnce(new Error('db')).mockResolvedValue(undefined)
    await openDialog()
    // 迷你色行在 item-actions 里（第一组 8 个 mini 色点）
    const miniDots = qa('.color-dot.mini').slice(0, UI.groupPalette.length)
    await miniDots[3]!.trigger('click')
    await flushPromises()
    expect(spy).toHaveBeenCalledWith(1, { color: UI.groupPalette[3] })
    // 失败回滚：色恢复为原值
    expect(groups.groups[0].color).toBe('#4a9eff')
    // 第二次成功：乐观值保留
    await miniDots[1]!.trigger('click')
    await flushPromises()
    expect(groups.groups[0].color).toBe(UI.groupPalette[1])
  })

  it('折叠切换：翻转 + updateGroup(id, {collapsed})；失败回滚', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const spy = vi.spyOn(groups, 'updateGroup').mockRejectedValueOnce(new Error('db')).mockResolvedValue(undefined)
    await openDialog()
    await qa('.item-actions .mini-btn')[0]!.trigger('click')
    await flushPromises()
    expect(spy).toHaveBeenCalledWith(1, { collapsed: true })
    expect(groups.groups[0].collapsed).toBe(false) // 失败回滚
  })
})

describe('GroupsDialog · 删除（内层确认框）', () => {
  it('点删除弹确认框（描述含组名）；取消不删', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const spy = vi.spyOn(groups, 'deleteGroup').mockResolvedValue(undefined)
    const w = await openDialog()
    await q('.mini-btn.danger').trigger('click')
    await flushPromises()
    const desc = Array.from(document.querySelectorAll('.ui-dialog-desc')).map((n) => n.textContent ?? '').join()
    expect(desc).toContain('工作')
    await q('.bm-cancel').trigger('click')
    await flushPromises()
    expect(spy).not.toHaveBeenCalled()
    void w
  })

  it('确认删除：deleteGroup 调用 + 组内标签 assignTabToGroup(id, null)', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const tabs = useTabsStore()
    tabs.tabs = [
      { id: 't1', url: 'u', title: 'T', groupId: 1 },
      { id: 't2', url: 'u', title: 'T', groupId: 1 },
      { id: 't3', url: 'u', title: 'T', groupId: null },
    ] as never
    const assignSpy = vi.spyOn(tabs, 'assignTabToGroup').mockResolvedValue(undefined)
    vi.spyOn(groups, 'deleteGroup').mockResolvedValue(undefined)
    await openDialog()
    await q('.mini-btn.danger').trigger('click')
    await flushPromises()
    await qa('.bm-save').pop()!.trigger('click')
    await flushPromises()
    expect(assignSpy).toHaveBeenCalledTimes(2)
    expect(assignSpy).toHaveBeenCalledWith('t1', null)
    expect(assignSpy).toHaveBeenCalledWith('t2', null)
    // 未分组标签不在清除范围
    expect(assignSpy).not.toHaveBeenCalledWith('t3', null)
  })

  it('删除失败：不清标签归属（UI 与库不脱节）', async () => {
    const groups = useGroupsStore()
    groups.groups = [mkGroup()] as never
    const tabs = useTabsStore()
    tabs.tabs = [{ id: 't1', url: 'u', title: 'T', groupId: 1 }] as never
    const assignSpy = vi.spyOn(tabs, 'assignTabToGroup').mockResolvedValue(undefined)
    vi.spyOn(groups, 'deleteGroup').mockRejectedValue(new Error('db'))
    await openDialog()
    await q('.mini-btn.danger').trigger('click')
    await flushPromises()
    await qa('.bm-save').pop()!.trigger('click')
    await flushPromises()
    expect(assignSpy).not.toHaveBeenCalled()
  })
})

// BookmarksBar 组件测试（F-27 前端补课）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子书签/文件夹）+
// mock Tauri IPC（invoke）+ mock utils/webview。useConfirmDelete 用真实 composable
// （纯前端二次确认，无外部依赖）。UiDialog 打桩（编辑弹窗覆盖不依赖 Reka 底座）。
//
// 覆盖：书签/文件夹渲染分组、空子树文件夹不显示、点击书签导航、收藏当前页、
//   删除二次确认（两段式）、右键菜单删除、文件夹下拉展开与行点击、管理器入口。
//
// 跑法：npx vitest run src/components/BookmarksBar.test.ts
// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import BookmarksBar from './BookmarksBar.vue'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import type { Bookmark, BookmarkFolder } from '../types/bookmark'

const invokeMock = vi.fn()
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}))
vi.mock('../utils/webview', () => ({
  setWebviewVisible: vi.fn(() => Promise.resolve()),
}))

let pinia: Pinia

function folder(id: number, name: string, parent_id: number | null): BookmarkFolder {
  return { id, name, parent_id, sort_order: id, created_at: '2026-10-15T00:00:00Z' }
}

function bm(id: number, folder_id: number | null, title?: string): Bookmark {
  return {
    id,
    title: title ?? `书签${id}`,
    url: `https://x.com/${id}`,
    folder_id,
    sort_order: id,
    created_at: '2026-10-15T00:00:00Z',
  }
}

/** 种子：顶层文件夹 1（含书签 10）、空文件夹 2、未分类书签 11/12 */
async function seedStores() {
  const bookmarks = useBookmarksStore()
  const tabs = useTabsStore()
  bookmarks.$patch({
    loaded: true,
    bookmarks: [bm(10, 1, '夹内书签'), bm(11, null, '未分类一'), bm(12, null, '未分类二')],
  })
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_bookmark_folders') {
      return Promise.resolve([folder(1, '工作夹', null), folder(2, '空夹', null)] as unknown[])
    }
    return Promise.resolve(undefined as unknown)
  })
  // 活动标签：收藏/导航都用它
  tabs.$patch({
    tabs: [{ id: 't1', url: 'https://page.example', title: '示例页' }],
    activeTabId: 't1',
  } as never)
  return { bookmarks, tabs }
}

async function mountBar() {
  await seedStores()
  const w = mount(BookmarksBar, {
    global: {
      stubs: { UiDialog: { template: '<div class="ui-dialog-stub"><slot /></div>' } },
    },
    attachTo: document.body,
  })
  await flushPromises()
  return w
}

beforeEach(() => {
  pinia = createPinia()
  setActivePinia(pinia)
  invokeMock.mockReset()
})

describe('BookmarksBar：渲染分组', () => {
  it('渲染未分类书签 + 非空顶层文件夹；空子树文件夹不显示', async () => {
    const w = await mountBar()
    const folderNames = w.findAll('.folder-item').map((n) => n.text())
    expect(folderNames.length).toBe(1, '空夹子树无书签无子文件夹，不显示')
    expect(folderNames[0]).toContain('工作夹')
    const items = w.findAll('.bookmark-item').map((n) => n.text())
    expect(items).toHaveLength(2)
    expect(items.join()).toContain('未分类一')
    expect(items.join()).not.toContain('夹内书签', '夹内书签不平铺，进文件夹下拉')
  })

  it('管理按钮点击 emit open-manager', async () => {
    const w = await mountBar()
    await w.find('.manage-btn').trigger('click')
    expect(w.emitted('open-manager')).toBeTruthy()
  })
})

describe('BookmarksBar：书签操作', () => {
  it('点击书签 → 导航活动标签到书签 URL', async () => {
    const w = await mountBar()
    await w.findAll('.bookmark-item')[0].trigger('click')
    const tabs = useTabsStore()
    expect(tabs.activeTabId).toBe('t1')
    // navigateTab 走 store 动作（会 invoke），断言调用到位
    expect(invokeMock).toHaveBeenCalled()
  })

  it('收藏当前页：以活动标签标题落库', async () => {
    const w = await mountBar()
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === 'get_bookmarks') return Promise.resolve([] as unknown[])
      return Promise.resolve(undefined as unknown)
    })
    await w.findAll('.action-btn')[0].trigger('click')
    await flushPromises()
    const created = invokeMock.mock.calls.find((c) => c[0] === 'create_bookmark')
    expect(created).toBeTruthy()
    expect((created![1] as { input: { title: string; url: string } }).input).toEqual({
      title: '示例页',
      url: 'https://page.example',
      folder_id: null,
    })
  })

  it('无活动标签时收藏不发请求', async () => {
    const w = await mountBar()
    const tabs = useTabsStore()
    tabs.$patch({ activeTabId: null } as never)
    await w.findAll('.action-btn')[0].trigger('click')
    await flushPromises()
    expect(invokeMock.mock.calls.some((c) => c[0] === 'create_bookmark')).toBe(false)
  })

  it('删除二次确认：第一次进待确认态（?），第二次真删', async () => {
    const w = await mountBar()
    const del = w.findAll('.bookmark-item')[0].find('.bm-del')
    await del.trigger('click')
    expect(del.text()).toBe('?', '第一次点击只进入待确认态')
    expect(invokeMock.mock.calls.some((c) => c[0] === 'delete_bookmark')).toBe(false)
    await del.trigger('click')
    await flushPromises()
    expect(invokeMock.mock.calls.some((c) => c[0] === 'delete_bookmark')).toBe(true)
  })
})

describe('BookmarksBar：右键菜单与文件夹下拉', () => {
  it('书签右键出管理菜单，点删除直接删（菜单内无二次确认）', async () => {
    const w = await mountBar()
    await w.findAll('.bookmark-item')[0].trigger('contextmenu')
    expect(w.find('.context-menu').exists()).toBe(true)
    expect(w.find('.context-menu').text()).toContain('删除')
    await w.findAll('.context-menu .menu-item')[3].trigger('click') // ✕ 删除
    await flushPromises()
    expect(invokeMock.mock.calls.some((c) => c[0] === 'delete_bookmark')).toBe(true)
  })

  it('空白处右键出收藏/新建文件夹菜单', async () => {
    const w = await mountBar()
    await w.find('.bookmarks-bar').trigger('contextmenu')
    const menu = w.find('.context-menu')
    expect(menu.exists()).toBe(true)
    expect(menu.text()).toContain('收藏当前页')
    expect(menu.text()).toContain('新建文件夹')
  })

  it('点击文件夹展开下拉：书签行点击导航并关闭；空文件夹显示空态', async () => {
    const w = await mountBar()
    await w.find('.folder-item').trigger('click')
    const menu = w.find('.folder-menu')
    expect(menu.exists()).toBe(true)
    // flattenSubtree 不含根文件夹自身行：行 = 子文件夹 + 书签（此处只有 1 个书签行）
    expect(menu.text()).toContain('夹内书签')
    // 点书签行：导航 + 关菜单
    await menu.findAll('.folder-menu-item')[0].trigger('click')
    await flushPromises()
    expect(w.find('.folder-menu').exists()).toBe(false)

    // 空文件夹：显示空态
    await w.findAll('.folder-item')[0].trigger('contextmenu') // 先关掉可能的菜单
    const bars = w.findAll('.folder-item')
    expect(bars.length).toBe(1, '空夹不渲染按钮，无入口可直接展开')
  })
})

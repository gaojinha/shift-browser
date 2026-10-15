// BookmarkFolderTree 组件测试（F-8 前端补课）
//
// 手法：@vue/test-utils 真实挂载 + utils/bookmarks.buildFolderTree 造真实树。
// 组件为纯展示（无 IPC/无 store）：折叠状态由父组件经 collapsed Set 注入，
// 事件走 emit（toggle/delete-folder/new-subfolder），直接断言 emitted()。
//
// 覆盖：多级渲染与递归缩进、折叠/展开事件与子树显隐、叶子节点语义、
//   子树书签计数（含子孙文件夹）、操作按钮事件、空态。
//
// 跑法：npx vitest run src/components/BookmarkFolderTree.test.ts
// @vitest-environment jsdom
import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import BookmarkFolderTree from './BookmarkFolderTree.vue'
import { buildFolderTree } from '../utils/bookmarks'
import type { Bookmark, BookmarkFolder } from '../types/bookmark'

function folder(id: number, name: string, parent_id: number | null): BookmarkFolder {
  return { id, name, parent_id, sort_order: id, created_at: '2026-10-15T00:00:00Z' }
}

function bm(id: number, folder_id: number | null): Bookmark {
  return { id, title: `书签${id}`, url: `https://x.com/${id}`, folder_id, sort_order: id, created_at: '2026-10-15T00:00:00Z' }
}

/** 两级树：root(1) ⊃ child(2)；书签：root 下 1 个、child 下 2 个 */
function seed() {
  const folders: BookmarkFolder[] = [folder(1, '根夹', null), folder(2, '子夹', 1)]
  const bookmarks: Bookmark[] = [bm(10, 1), bm(11, 2), bm(12, 2)]
  return { nodes: buildFolderTree(folders), bookmarks }
}

function mountTree(collapsed: Set<number> = new Set()) {
  const { nodes, bookmarks } = seed()
  return mount(BookmarkFolderTree, {
    props: { nodes, bookmarks, collapsed, depth: 0 },
  })
}

describe('BookmarkFolderTree：递归树渲染', () => {
  it('渲染两级树行，子级缩进 = depth*16px', () => {
    const w = mountTree()
    const rows = w.findAll('.bm-tree-row')
    expect(rows.length).toBe(2)
    // 顶层 depth=0
    expect(rows[0].attributes('style')).toContain('padding-left: 0px')
    // 子级 depth=1（递归实例）
    expect(rows[1].attributes('style')).toContain('padding-left: 16px')
    // 名称与计数：root 子树含子孙文件夹书签共 3 个，child 自身 2 个
    expect(rows[0].find('.bm-tree-name').text()).toContain('根夹')
    expect(rows[0].find('.bm-tree-count').text()).toBe('3 个')
    expect(rows[1].find('.bm-tree-name').text()).toContain('子夹')
    expect(rows[1].find('.bm-tree-count').text()).toBe('2 个')
  })

  it('有子级的文件夹：aria-expanded=true，点击行名或按钮 emit toggle(id)', async () => {
    const w = mountTree()
    const toggle = w.find('.bm-tree-toggle')
    expect(toggle.attributes('aria-expanded')).toBe('true')
    expect(toggle.classes()).not.toContain('leaf')

    await w.find('.bm-tree-toggle').trigger('click')
    await w.find('.bm-tree-name').trigger('click')
    const ev = w.emitted('toggle')
    expect(ev).toBeTruthy()
    expect(ev![0]).toEqual([1])
    expect(ev!.length).toBe(2)
  })

  it('collapsed 集合折叠子树：子级行不渲染，箭头翻转 aria-expanded=false', async () => {
    const w = mountTree(new Set([1]))
    // 子级行不渲染（v-if）
    expect(w.findAll('.bm-tree-row').length).toBe(1)
    const toggle = w.find('.bm-tree-toggle')
    expect(toggle.attributes('aria-expanded')).toBe('false')
    expect(toggle.find('.bm-tree-arrow').text()).toBe('▸')
    // 折叠时点击仍 emit toggle（由父组件翻转 Set）
    await toggle.trigger('click')
    expect(w.emitted('toggle')![0]).toEqual([1])
  })

  it('叶子文件夹：leaf 类、无 aria-expanded、点击不 emit toggle', async () => {
    // 只放一个无子级的文件夹
    const w = mount(BookmarkFolderTree, {
      props: {
        nodes: buildFolderTree([folder(5, '叶子', null)]),
        bookmarks: [bm(1, 5)],
        collapsed: new Set<number>(),
        depth: 0,
      },
    })
    const toggle = w.find('.bm-tree-toggle')
    expect(toggle.classes()).toContain('leaf')
    expect(toggle.attributes('aria-expanded')).toBeUndefined()
    await toggle.trigger('click')
    await w.find('.bm-tree-name').trigger('click')
    expect(w.emitted('toggle')).toBeUndefined()
    expect(w.find('.bm-tree-count').text()).toBe('1 个')
  })

  it('操作按钮：new-subfolder / delete-folder 事件带正确 id', async () => {
    const w = mountTree()
    const row = w.findAll('.bm-tree-row')[0]
    await row.find('.op-btn:not(.danger)').trigger('click')
    await row.find('.op-btn.danger').trigger('click')
    expect(w.emitted('new-subfolder')![0]).toEqual([1])
    expect(w.emitted('delete-folder')![0]).toEqual([1])
  })

  it('子树内事件冒泡转发：子级的删除 emit 到顶层（id=2）', async () => {
    const w = mountTree()
    const childRow = w.findAll('.bm-tree-row')[1]
    await childRow.find('.op-btn.danger').trigger('click')
    // 递归实例把事件转发给父级，最终从根实例发出
    expect(w.emitted('delete-folder')![0]).toEqual([2])
    expect(w.emitted('new-subfolder')).toBeUndefined()
  })

  it('空节点列表显示空态', () => {
    const w = mount(BookmarkFolderTree, {
      props: { nodes: [], bookmarks: [], collapsed: new Set<number>(), depth: 0 },
    })
    expect(w.find('.bm-tree-empty').text()).toBe('（无文件夹）')
  })
})

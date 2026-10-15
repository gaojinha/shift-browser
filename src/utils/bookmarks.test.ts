// 书签文件夹树形纯函数单测（构树 / 子树展平 / 层级路径名）
import { describe, it, expect } from 'vitest'
import { buildFolderTree, flattenSubtree, folderPathName } from './bookmarks'
import type { Bookmark, BookmarkFolder } from '../types/bookmark'

const TS = '2026-10-15T00:00:00Z'
const f = (id: number, name: string, parent_id: number | null = null, sort_order = 0): BookmarkFolder => ({
  id,
  name,
  parent_id,
  sort_order,
  created_at: TS,
})
const b = (id: number, title: string, url: string, folder_id: number | null = null, sort_order = 0): Bookmark => ({
  id,
  title,
  url,
  folder_id,
  sort_order,
  created_at: TS,
})

describe('buildFolderTree', () => {
  it('空数组返回空', () => {
    expect(buildFolderTree([])).toEqual([])
  })
  it('全顶层按 sort_order 排序', () => {
    const tree = buildFolderTree([f(2, 'B', null, 1), f(1, 'A', null, 0), f(3, 'C', null, 2)])
    expect(tree.map((n) => n.id)).toEqual([1, 2, 3])
    expect(tree.every((n) => n.children.length === 0)).toBe(true)
  })
  it('嵌套层级正确（children 逐层挂在父下）', () => {
    const tree = buildFolderTree([f(1, 'root', null, 0), f(2, 'child', 1, 0), f(3, 'grandchild', 2, 0)])
    expect(tree.length).toBe(1)
    expect(tree[0].id).toBe(1)
    expect(tree[0].children.map((n) => n.id)).toEqual([2])
    expect(tree[0].children[0].children.map((n) => n.id)).toEqual([3])
  })
  it('parent_id 悬空时归顶层不丢节点', () => {
    const tree = buildFolderTree([f(1, 'orphan', 999, 0)])
    expect(tree.map((n) => n.id)).toEqual([1])
  })
  it('子节点顺序按 sort_order（防御乱序输入）', () => {
    const tree = buildFolderTree([f(1, 'root', null, 0), f(2, 'c2', 1, 1), f(3, 'c1', 1, 0)])
    expect(tree[0].children.map((n) => n.id)).toEqual([3, 2])
  })
})

describe('flattenSubtree', () => {
  it('空输入返回空', () => {
    expect(flattenSubtree(1, [], [])).toEqual([])
  })
  it('直接书签在 depth 0', () => {
    const rows = flattenSubtree(1, [f(1, 'root', null, 0)], [b(10, 'x', 'http://x', 1)])
    expect(rows).toHaveLength(1)
    expect(rows[0].depth).toBe(0)
    expect(rows[0].bookmark?.id).toBe(10)
  })
  it('子文件夹与其深层书签缩进逐级递增', () => {
    const folders = [f(1, 'root', null, 0), f(2, 'mid', 1, 0), f(3, 'deep', 2, 0)]
    const bms = [b(10, 'r', 'http://r', 1), b(20, 'm', 'http://m', 2), b(30, 'd', 'http://d', 3)]
    const rows = flattenSubtree(1, folders, bms)
    // r(depth0) → mid(depth0) → m(depth1) → deep(depth1) → d(depth2)
    expect(rows.map((r) => r.depth)).toEqual([0, 0, 1, 1, 2])
    expect(rows[0].bookmark?.id).toBe(10)
    expect(rows[1].folder?.id).toBe(2)
    expect(rows[2].bookmark?.id).toBe(20)
    expect(rows[3].folder?.id).toBe(3)
    expect(rows[4].bookmark?.id).toBe(30)
  })
  it('未分类书签不属于任何文件夹子树', () => {
    const rows = flattenSubtree(1, [f(1, 'root', null, 0)], [b(10, 'unfiled', 'http://u', null)])
    expect(rows).toEqual([])
  })
})

describe('folderPathName', () => {
  it('顶层只返回名字', () => {
    expect(folderPathName(1, [f(1, 'root', null, 0)])).toBe('root')
  })
  it('嵌套返回 父 › 子 路径', () => {
    const folders = [f(1, 'root', null, 0), f(2, 'mid', 1, 0), f(3, 'deep', 2, 0)]
    expect(folderPathName(3, folders)).toBe('root › mid › deep')
  })
  it('不存在的 id 返回空串', () => {
    expect(folderPathName(999, [f(1, 'root', null, 0)])).toBe('')
  })
  it('parent_id 成环不死循环（guard 兜底，函数能终止）', () => {
    const folders = [f(1, 'a', 2, 0), f(2, 'b', 1, 0)]
    const result = folderPathName(1, folders)
    expect(typeof result).toBe('string')
    expect(result.length).toBeGreaterThan(0)
  })
})

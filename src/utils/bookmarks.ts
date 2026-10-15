// 书签文件夹树形工具：把后端扁平的 bookmark_folders 构建成前端可展开的树。
// 后端 get_bookmark_folders 按 (sort_order, id) 升序返回；这里仍对每层再排一次序
// 保证子节点顺序稳定（防御乱序输入），并容错 parent_id 悬空/成环的脏数据（不丢节点、不死循环）。
import type { Bookmark, BookmarkFolder } from '../types/bookmark'

/** 文件夹树节点：在 BookmarkFolder 基础上挂 children */
export interface FolderTreeNode extends BookmarkFolder {
  children: FolderTreeNode[]
}

/** 书签栏下拉菜单的一行：文件夹标题（folder）或书签（bookmark），depth 表层级（CSS 缩进用） */
export interface SubtreeRow {
  depth: number
  folder?: { id: number; name: string }
  bookmark?: Bookmark
}

/** 扁平文件夹数组 → 顶层节点列表（children 递归挂好，各层按 sort_order,id 有序） */
export function buildFolderTree(folders: BookmarkFolder[]): FolderTreeNode[] {
  const map = new Map<number, FolderTreeNode>()
  const roots: FolderTreeNode[] = []
  for (const f of folders) map.set(f.id, { ...f, children: [] })
  for (const f of folders) {
    const node = map.get(f.id)
    if (!node) continue
    if (f.parent_id != null) {
      const parent = map.get(f.parent_id)
      if (parent) {
        parent.children.push(node)
        continue
      }
    }
    // 顶层，或 parent_id 悬空（脏数据）→ 归顶层，避免节点丢失
    roots.push(node)
  }
  const byOrder = (a: FolderTreeNode, b: FolderTreeNode) =>
    a.sort_order - b.sort_order || a.id - b.id
  const sortRec = (arr: FolderTreeNode[]) => {
    arr.sort(byOrder)
    for (const n of arr) sortRec(n.children)
  }
  sortRec(roots)
  return roots
}

/**
 * 某文件夹的整棵子树 → 展平行列表（书签栏下拉菜单用）。
 * depth 0 = 该文件夹的直接书签 / 直接子文件夹；子文件夹行之后紧跟其子树（depth+1）。
 * 书签排在同层子文件夹之前（与浏览器书签栏常见呈现一致）。
 */
export function flattenSubtree(
  rootId: number,
  folders: BookmarkFolder[],
  bookmarks: Bookmark[],
): SubtreeRow[] {
  const childrenByParent = new Map<number | null, BookmarkFolder[]>()
  for (const f of folders) {
    const key = f.parent_id ?? null
    const arr = childrenByParent.get(key)
    if (arr) arr.push(f)
    else childrenByParent.set(key, [f])
  }
  const bmByFolder = new Map<number | null, Bookmark[]>()
  for (const b of bookmarks) {
    const key = b.folder_id ?? null
    const arr = bmByFolder.get(key)
    if (arr) arr.push(b)
    else bmByFolder.set(key, [b])
  }
  const rows: SubtreeRow[] = []
  const walk = (folderId: number | null, depth: number) => {
    for (const b of bmByFolder.get(folderId) ?? []) rows.push({ depth, bookmark: b })
    for (const f of childrenByParent.get(folderId) ?? []) {
      rows.push({ depth, folder: { id: f.id, name: f.name } })
      walk(f.id, depth + 1)
    }
  }
  walk(rootId, 0)
  return rows
}

/** 文件夹的层级路径名（筛选下拉显示层级用），如 "父 › 子"。guard 防 parent_id 成环死循环 */
export function folderPathName(folderId: number, folders: BookmarkFolder[]): string {
  const byId = new Map(folders.map((f) => [f.id, f]))
  const parts: string[] = []
  let cur: BookmarkFolder | undefined = byId.get(folderId)
  let guard = 0
  while (cur && guard++ < 100) {
    parts.unshift(cur.name)
    cur = cur.parent_id != null ? byId.get(cur.parent_id) : undefined
  }
  return parts.join(' › ')
}

/** 某文件夹整棵子树（含自身）下的书签总数（扁平文件夹列表版，删除确认弹窗文案用） */
export function countBookmarksInSubtree(
  folderId: number,
  folders: BookmarkFolder[],
  bookmarks: Bookmark[],
): number {
  const childrenByParent = new Map<number | null, BookmarkFolder[]>()
  for (const f of folders) {
    const key = f.parent_id ?? null
    const arr = childrenByParent.get(key)
    if (arr) arr.push(f)
    else childrenByParent.set(key, [f])
  }
  const ids = new Set<number>()
  const stack = [folderId]
  while (stack.length) {
    const id = stack.pop()!
    if (ids.has(id)) continue
    ids.add(id)
    for (const f of childrenByParent.get(id) ?? []) stack.push(f.id)
  }
  return bookmarks.filter((bm) => bm.folder_id != null && ids.has(bm.folder_id)).length
}

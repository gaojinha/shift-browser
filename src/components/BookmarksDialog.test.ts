// BookmarksDialog 组件测试（F-77 前端补课，王二狗接手）
//
// 手法：@vue/test-utils 真实挂载 + 真实 pinia store（$patch 种子书签 + loaded=true
// 跳过 loadBookmarks）+ mock Tauri IPC（invoke mock get_bookmark_folders / 各写命令）
// + mock utils/webview + UiDialog 打桩（真实 reka-ui 底座在 jsdom 下 Portal 不落地，
// 插槽内容不渲染）+ BookmarkFolderTree 打桩（递归 tree 的内部渲染归
// BookmarkFolderTree.test.ts 单测，弹窗测试只验 delete-folder 事件接线）。
//
// 关键：组件里 watch(() => props.show) 无 immediate，挂载时 show=true 不触发加载。
// 正确手法：先挂 show=false，再 setProps({ show: true }) 触发 watch 的
// loadBookmarks（跳过，已 loaded）+ loadFolders 链路（folders 是组件内部 ref，
// 靠这条链路赋值，$patch 种子不覆盖它）。
//
// 覆盖：
//  1.  弹窗显隐（show=true → 渲染，show=false → 不渲染）
//  2.  书签列表渲染（3 条，含文件夹路径名 "工作 › 开发" 和 "未分类"）
//  3.  空列表 → 显示空状态提示
//  4.  按关键词搜索过滤（rust → 1 条，https:// → 3 条，nonexistent → 0 + 空态）
//  5.  按文件夹筛选（none → 1 条未分类，all → 3 条）
//  6.  点击书签行 → navigate_tab（invoke）+ emit close
//  7.  双击防误触删除（第一次 arm → armed/!，第二次才 removeBookmark）
//  8.  编辑书签 → updateBookmark + updateBookmarkFolder
//  9.  添加书签 → create_bookmark invoke（folder_id 默认 null）
//  10. 新建根文件夹 → create_bookmark_folder invoke（parent_id=null）
//  11. 删除文件夹确认流程（确认弹窗显隐 + 点确认 → delete_bookmark_folder invoke；
//      树的 delete-folder 事件接线由 BookmarkFolderTree.test.ts + 本文件 tree 桩共同兜住）
//  12. 导出 → export_bookmarks_html invoke
//  13. 导入 → import_bookmarks_from_file invoke
//
// 跑法：npx vitest run src/components/BookmarksDialog.test.ts
// @vitest-environment jsdom

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount, flushPromises, type DOMWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia, type Pinia } from 'pinia'
import BookmarksDialog from './BookmarksDialog.vue'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'
import type { Bookmark, BookmarkFolder } from '../types/bookmark'

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

// UiDialog 打桩：open=true 时渲染 body + actions slot（绕过 reka-ui Portal）
const UiDialogStub = {
  name: 'UiDialog',
  props: {
    open: { type: Boolean, default: false },
    title: String,
    width: String,
  },
  emits: ['update:open'],
  template: `
    <div v-if="open" class="ui-dialog-stub" :data-dialog-title="title">
      <div class="ui-dialog-body"><slot /></div>
      <div class="ui-dialog-actions"><slot name="actions" /></div>
    </div>
  `,
}

// BookmarkFolderTree 打桩：递归 tree 渲染归 BookmarkFolderTree.test.ts 单测，
// 弹窗测试只验 delete-folder 事件接线。桩把根节点（工作 id=1）和子节点（开发 id=2）
// 各自渲染成 .bm-tree-row，✕ 按钮 click 时 emit('delete-folder', 对应 id)。
const TreeStub = {
  name: 'BookmarkFolderTree',
  props: {
    nodes: Array,
    bookmarks: Array,
    collapsed: { type: Object, default: () => new Set<number>() },
    depth: { type: Number, default: 0 },
  },
  emits: ['toggle', 'delete-folder', 'new-subfolder'],
  template: `
    <div class="bm-tree">
      <div
        v-for="n in flatNodes"
        :key="n.id"
        class="bm-tree-row"
        :data-folder-id="n.id"
      >
        <span class="bm-tree-name">📁 {{ n.name }}</span>
        <button class="op-btn" title="新建子文件夹" @click="emit('new-subfolder', n.id)">＋</button>
        <button
          class="op-btn danger"
          title="删除文件夹（含其中书签与子文件夹）"
          @click="emit('delete-folder', n.id)"
        >✕</button>
      </div>
      <div v-if="flatNodes.length === 0" class="bm-tree-empty">（无文件夹）</div>
    </div>
  `,
  computed: {
    /** 把 FolderTreeNode[] 拍平成 {id,name}[]（含子孙），供模板渲染 */
    flatNodes(this: { nodes: Array<{ id: number; name: string; children: unknown[] }> }): Array<{ id: number; name: string }> {
      const out: Array<{ id: number; name: string }> = []
      const walk = (ns: Array<{ id: number; name: string; children: unknown[] }>) => {
        for (const n of ns) {
          out.push({ id: n.id, name: n.name })
          walk((n.children ?? []) as Array<{ id: number; name: string; children: unknown[] }>)
        }
      }
      walk(this.nodes ?? [])
      return out
    },
  },
}

// 样本数据（FOLDERS 有层级：工作(1) ⊃ 开发(2)；BOOKMARKS 3 条）
function folder(id: number, name: string, parent_id: number | null): BookmarkFolder {
  return { id, name, parent_id, sort_order: id, created_at: '2026-10-15T00:00:00Z' }
}
function bm(id: number, folder_id: number | null, title: string): Bookmark {
  return {
    id,
    title,
    url: `https://example.com/${id}`,
    folder_id,
    sort_order: id,
    created_at: '2026-10-15T00:00:00Z',
  }
}

const FOLDERS = [folder(1, '工作', null), folder(2, '开发', 1)]
const BOOKMARKS = [
  bm(101, 2, 'GitHub'),
  bm(102, 1, 'Rust Lang'),
  bm(103, null, '搜索引擎'),
]

let pinia: Pinia

/**
 * 种子：patch bookmarks（loaded=true 跳过 loadBookmarks 的 invoke 路径）
 * + mock get_bookmark_folders（invoke 返回 folders）。
 */
async function seedStores(
  bookmarks: Bookmark[] = BOOKMARKS,
  folders: BookmarkFolder[] = FOLDERS,
  activeTab?: { id: string; url: string; title: string },
) {
  pinia = createPinia()
  setActivePinia(pinia)

  const store = useBookmarksStore(pinia)
  store.$patch({ loaded: true, bookmarks: [...bookmarks] })

  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === 'get_bookmark_folders') return Promise.resolve([...folders])
    // 写命令兜底：返回与调用方签名匹配的值，避免 undefined 被 store 写回 state
    // 触发「bookmarks 变 undefined → 计算属性 filter 崩」的 unhandled rejection。
    // 注意 onSaveAdd/onSaveEdit 成功后会再调 loadBookmarks → get_bookmarks，
    // 所以每个用例若覆写 mockImplementation 也要带上这两条。
    if (cmd === 'get_bookmarks') return Promise.resolve([...bookmarks])
    if (cmd === 'create_bookmark') return Promise.resolve(999)
    if (cmd === 'create_bookmark_folder') return Promise.resolve(99)
    return Promise.resolve(undefined as unknown)
  })

  if (activeTab) {
    const tabs = useTabsStore(pinia)
    tabs.$patch({
      tabs: [
        {
          ...activeTab,
          favicon: '',
          is_active: true,
          is_pinned: false,
          is_muted: false,
          hibernated: false,
          private: false,
          origin: 'user',
          groupId: null,
        },
      ],
      activeTabId: activeTab.id,
    } as never)
  }
  return { store }
}

/** 各用例覆写 mockImplementation 时合并的通用兜底（get_bookmarks / get_bookmark_folders） */
function baseMock(bookmarks: Bookmark[] = BOOKMARKS, folders: BookmarkFolder[] = FOLDERS) {
  return async (cmd: string) => {
    if (cmd === 'get_bookmark_folders') return [...folders]
    if (cmd === 'get_bookmarks') return [...bookmarks]
    return undefined
  }
}

/**
 * 挂载弹窗：先 show=false 挂起，再 setProps({ show }) 触发组件内
 * watch(() => props.show)（无 immediate，挂载态不触发），等待 loadFolders
 * 等异步任务落定。show=false 用例只挂载不翻开。
 */
async function mountDialog(
  show = true,
  bookmarks: Bookmark[] = BOOKMARKS,
  folders: BookmarkFolder[] = FOLDERS,
  activeTab?: { id: string; url: string; title: string },
) {
  await seedStores(bookmarks, folders, activeTab)
  const w = mount(BookmarksDialog, {
    props: { show: false },
    global: {
      plugins: [pinia],
      stubs: {
        UiDialog: UiDialogStub,
        BookmarkFolderTree: TreeStub,
      },
    },
    attachTo: document.body,
  })
  await flushPromises()
  if (show) {
    w.setProps({ show: true })
    await flushPromises()
  }
  return w
}

/** 主弹窗里找「操作区」（.bm-ops）里某按钮：.op-btn 在主列表行与 tree 桩里都有，限定父链。 */
function rowOpBtn(w: DOMWrapper<Element>, title: string): DOMWrapper<Element> {
  const all = w.findAll('.bm-row .op-btn')
  const hit = all.find((b) => (b.attributes('title') ?? '').includes(title))
  return hit!
}

beforeEach(() => {
  invokeMock.mockClear()
})

describe('BookmarksDialog', () => {
  it('show=true 时渲染书签管理器弹窗；show=false 时不渲染', async () => {
    const w = await mountDialog(true)
    expect(w.find('.ui-dialog-stub[data-dialog-title="书签管理器"]').exists()).toBe(true)

    const w2 = await mountDialog(false)
    expect(w2.find('.ui-dialog-stub[data-dialog-title="书签管理器"]').exists()).toBe(false)
  })

  it('渲染书签列表，显示文件夹路径名（"工作 › 开发"），未分类显示"未分类"', async () => {
    const w = await mountDialog(true)
    const rows = w.findAll('.bm-row')
    expect(rows.length).toBe(3)
    // 第 1 条：GitHub（folder_id=2 → "工作 › 开发"）
    expect(rows[0].text()).toContain('GitHub')
    expect(rows[0].text()).toContain('https://example.com/101')
    expect(rows[0].text()).toContain('工作 › 开发')
    // 第 2 条：Rust Lang（folder_id=1 → "工作"）
    expect(rows[1].text()).toContain('Rust Lang')
    expect(rows[1].text()).toContain('工作')
    // 第 3 条：搜索引擎（folder_id=null → "未分类"）
    expect(rows[2].text()).toContain('搜索引擎')
    expect(rows[2].text()).toContain('未分类')
  })

  it('空列表时显示空状态提示', async () => {
    const w = await mountDialog(true, [], [])
    expect(w.find('.bm-empty').exists()).toBe(true)
    expect(w.find('.bm-empty').text()).toContain('暂无书签')
  })

  it('按关键词搜索：rust → 1 条；https:// → 3 条；nonexistent → 0 + 空态', async () => {
    const w = await mountDialog(true)
    const search = w.find('input.bm-search')
    expect(search.exists()).toBe(true)

    await search.setValue('rust')
    let rows = w.findAll('.bm-row')
    expect(rows.length).toBe(1)
    expect(rows[0].text()).toContain('Rust Lang')

    await search.setValue('https://')
    rows = w.findAll('.bm-row')
    expect(rows.length).toBe(3)

    await search.setValue('nonexistent')
    rows = w.findAll('.bm-row')
    expect(rows.length).toBe(0)
    expect(w.find('.bm-empty').exists()).toBe(true)
  })

  it('按文件夹筛选：none → 仅未分类（1 条）；all → 全部（3 条）', async () => {
    const w = await mountDialog(true)
    const select = w.find('select.bm-filter')
    expect(select.exists()).toBe(true)

    await select.setValue('none')
    let rows = w.findAll('.bm-row')
    expect(rows.length).toBe(1)
    expect(rows[0].text()).toContain('搜索引擎')

    await select.setValue('all')
    rows = w.findAll('.bm-row')
    expect(rows.length).toBe(3)
  })

  it('点击书签行 → 调 navigate_tab（invoke）并 emit close', async () => {
    const activeTab = { id: 't1', url: 'about:blank', title: '示例页' }
    const w = await mountDialog(true, BOOKMARKS, FOLDERS, activeTab)

    const info = w.find('.bm-row .bm-info')
    expect(info.exists()).toBe(true)
    await info.trigger('click')

    // navigateTab → invoke('navigate_tab', { id, url })
    expect(invokeMock).toHaveBeenCalledWith(
      'navigate_tab',
      expect.objectContaining({ url: 'https://example.com/101' }),
    )
    expect(w.emitted('close')).toBeTruthy()
  })

  it('双击防误触删除：第一次 arm（armed class + !），第二次才执行 removeBookmark', async () => {
    const w = await mountDialog(true)
    const store = useBookmarksStore(pinia)
    const removeSpy = vi.spyOn(store, 'removeBookmark').mockResolvedValue(true)

    const deleteBtn = rowOpBtn(w, '删除')
    expect(deleteBtn.exists()).toBe(true)
    expect(deleteBtn.text()).toBe('✕')

    // 第一次：进入 armed
    await deleteBtn.trigger('click')
    expect(deleteBtn.classes()).toContain('armed')
    expect(deleteBtn.text()).toBe('!')
    expect(removeSpy).not.toHaveBeenCalled()

    // 第二次：执行删除
    await deleteBtn.trigger('click')
    expect(removeSpy).toHaveBeenCalledWith(101)
  })

  it('编辑书签：打开弹窗 → 改标题 → 保存 → 调 updateBookmark + updateBookmarkFolder', async () => {
    const w = await mountDialog(true)
    const store = useBookmarksStore(pinia)
    const updateSpy = vi.spyOn(store, 'updateBookmark').mockResolvedValue(undefined)
    const updateFolderSpy = vi.spyOn(store, 'updateBookmarkFolder').mockResolvedValue(undefined)

    const editBtn = rowOpBtn(w, '编辑')
    expect(editBtn.exists()).toBe(true)
    await editBtn.trigger('click')

    const editDialog = w.find('.ui-dialog-stub[data-dialog-title="编辑书签"]')
    expect(editDialog.exists()).toBe(true)

    // 编辑弹窗里两个 input（标题、网址）
    const inputs = editDialog.findAll('input')
    expect(inputs.length).toBeGreaterThanOrEqual(2)
    await inputs[0].setValue('GitHub Updated')

    const saveBtn = editDialog.find('.bm-save')
    expect(saveBtn.exists()).toBe(true)
    await saveBtn.trigger('click')
    await flushPromises()

    // 第一行 bookmark（id=101）
    expect(updateSpy).toHaveBeenCalledWith(101, 'GitHub Updated', 'https://example.com/101')
    // 未改文件夹 → 保持原值（folderId '2' → 2）
    expect(updateFolderSpy).toHaveBeenCalledWith(101, 2)
  })

  it('添加书签：打开弹窗 → 填表 → 保存 → 调 create_bookmark invoke（folder_id=null）', async () => {
    const w = await mountDialog(true)

    const addBtn = w.findAll('.h-btn').find((b) => b.text().includes('添加书签'))!
    expect(addBtn.exists()).toBe(true)
    await addBtn.trigger('click')

    const addDialog = w.find('.ui-dialog-stub[data-dialog-title="添加书签"]')
    expect(addDialog.exists()).toBe(true)

    const inputs = addDialog.findAll('input')
    expect(inputs.length).toBeGreaterThanOrEqual(2)
    await inputs[0].setValue('Vite')
    await inputs[1].setValue('https://vite.dev')

    const saveBtn = addDialog.find('.bm-save')
    expect(saveBtn.exists()).toBe(true)
    await saveBtn.trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('create_bookmark', {
      input: { title: 'Vite', url: 'https://vite.dev', folder_id: null },
    })
  })

  it('新建根文件夹 → 调 create_bookmark_folder invoke（parent_id=null）', async () => {
    const w = await mountDialog(true)

    const newFolderBtn = w.findAll('.h-btn').find((b) => b.text().includes('新建文件夹'))!
    expect(newFolderBtn.exists()).toBe(true)
    await newFolderBtn.trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('create_bookmark_folder', {
      input: { name: '新文件夹', parent_id: null },
    })
  })

  it('删除文件夹确认流程：确认弹窗渲染「开发」+ 点确认 → delete_bookmark_folder invoke', async () => {
    const w = await mountDialog(true)

    // 直接驱动确认弹窗的内部状态（树的 delete-folder → onDeleteFolder 接线归 tree 单测）
    const vm = w.vm as unknown as {
      folderDeleteTarget: { id: number; name: string } | null
      showFolderConfirm: boolean
      doDeleteFolder: () => Promise<void>
    }
    vm.folderDeleteTarget = { id: 2, name: '开发' }
    vm.showFolderConfirm = true
    await flushPromises()

    // 确认弹窗出现，文案含文件夹名
    const confirmDialog = w.find('.ui-dialog-stub[data-dialog-title="删除文件夹"]')
    expect(confirmDialog.exists()).toBe(true)
    expect(confirmDialog.text()).toContain('开发')

    // 点确认删除 → doDeleteFolder
    await vm.doDeleteFolder()
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('delete_bookmark_folder', { id: 2 })
  })

  it('导出书签 → 调 export_bookmarks_html invoke', async () => {
    const w = await mountDialog(true)
    invokeMock.mockImplementation(async (cmd: string) => {
      const fallback = await baseMock()(cmd)
      if (cmd === 'export_bookmarks_html') return '/tmp/exports/bookmarks.html'
      return fallback
    })

    const exportBtn = w.findAll('.h-btn').find((b) => b.text().includes('导出'))!
    expect(exportBtn.exists()).toBe(true)
    await exportBtn.trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('export_bookmarks_html')
  })

  it('导入书签 → 调 import_bookmarks_from_file invoke', async () => {
    const w = await mountDialog(true)
    invokeMock.mockImplementation(async (cmd: string) => {
      const fallback = await baseMock()(cmd)
      if (cmd === 'import_bookmarks_from_file') return 5
      return fallback
    })

    const importBtn = w.findAll('.h-btn').find((b) => b.text().includes('导入'))!
    expect(importBtn.exists()).toBe(true)
    await importBtn.trigger('click')
    await flushPromises()

    expect(invokeMock).toHaveBeenCalledWith('import_bookmarks_from_file')
  })
})

<script setup lang="ts">
// 书签文件夹树（递归）：展开/折叠 + 删除 + 新建子文件夹。
// 纯展示：折叠状态由父组件（BookmarksDialog）用 collapsed Set 单一来源持有并传入，
// 递归实例共享同一 Set，保证折叠状态在层级间一致。
import type { Bookmark } from '../types/bookmark'
import type { FolderTreeNode } from '../utils/bookmarks'

const props = withDefaults(
  defineProps<{
    nodes: FolderTreeNode[]
    bookmarks: Bookmark[]
    collapsed: Set<number>
    depth?: number
  }>(),
  { depth: 0 },
)
const emit = defineEmits<{
  (e: 'toggle', id: number): void
  (e: 'delete-folder', id: number): void
  (e: 'new-subfolder', parentId: number): void
}>()

/** 该文件夹整棵子树（含自身）下的书签总数 */
function countInSubtree(node: FolderTreeNode): number {
  const ids = new Set<number>()
  const collect = (n: FolderTreeNode) => {
    ids.add(n.id)
    for (const c of n.children) collect(c)
  }
  collect(node)
  return props.bookmarks.filter((bm) => bm.folder_id != null && ids.has(bm.folder_id)).length
}

function isCollapsed(id: number) {
  return props.collapsed.has(id)
}
</script>

<template>
  <div class="bm-tree">
    <template v-for="node in nodes" :key="node.id">
      <div class="bm-tree-row" :style="{ paddingLeft: props.depth * 16 + 'px' }">
        <button
          class="bm-tree-toggle"
          :class="{ leaf: node.children.length === 0 }"
          :aria-label="node.children.length ? `折叠 文件夹 ${node.name}` : `文件夹 ${node.name}`"
          :aria-expanded="node.children.length ? !isCollapsed(node.id) : undefined"
          @click="node.children.length && emit('toggle', node.id)"
        >
          <span v-if="node.children.length" class="bm-tree-arrow" aria-hidden="true">
            {{ isCollapsed(node.id) ? '▸' : '▾' }}
          </span>
          <span v-else class="bm-tree-arrow-placeholder" aria-hidden="true"></span>
        </button>
        <span
          class="bm-tree-name"
          :title="node.name"
          @click="node.children.length && emit('toggle', node.id)"
          >📁 {{ node.name }}</span
        >
        <span class="bm-tree-count">{{ countInSubtree(node) }} 个</span>
        <span class="bm-tree-ops">
          <button class="op-btn" title="新建子文件夹" @click="emit('new-subfolder', node.id)">＋</button>
          <button
            class="op-btn danger"
            title="删除文件夹（含其中书签与子文件夹）"
            @click="emit('delete-folder', node.id)"
          >
            ✕
          </button>
        </span>
      </div>
      <BookmarkFolderTree
        v-if="node.children.length && !isCollapsed(node.id)"
        :nodes="node.children"
        :bookmarks="bookmarks"
        :collapsed="collapsed"
        :depth="depth + 1"
        @toggle="(id) => emit('toggle', id)"
        @delete-folder="(id) => emit('delete-folder', id)"
        @new-subfolder="(id) => emit('new-subfolder', id)"
      />
    </template>
    <div v-if="nodes.length === 0" class="bm-tree-empty">（无文件夹）</div>
  </div>
</template>

<style scoped>
.bm-tree {
  display: flex;
  flex-direction: column;
}
.bm-tree-row {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px 4px 0;
  border-radius: var(--radius-xs);
}
.bm-tree-row:hover {
  background: var(--bg-active);
}
.bm-tree-toggle {
  width: 20px;
  height: 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background: transparent;
  color: var(--text-secondary);
  cursor: pointer;
  border-radius: 4px;
  flex-shrink: 0;
  padding: 0;
}
.bm-tree-toggle.leaf {
  cursor: default;
}
.bm-tree-toggle:hover:not(.leaf) {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.bm-tree-toggle:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
.bm-tree-arrow {
  font-size: 10px;
  line-height: 1;
}
.bm-tree-arrow-placeholder {
  width: 10px;
  display: inline-block;
}
.bm-tree-name {
  flex: 1;
  min-width: 0;
  font-size: 12px;
  color: var(--text-primary);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: pointer;
}
.bm-tree-row:hover .bm-tree-name {
  color: var(--text-primary);
}
.bm-tree-count {
  flex-shrink: 0;
  font-size: 11px;
  color: var(--text-muted);
}
.bm-tree-ops {
  display: none;
  gap: 2px;
  flex-shrink: 0;
}
.bm-tree-row:hover .bm-tree-ops,
.bm-tree-ops:focus-within {
  display: inline-flex;
}
.op-btn {
  width: 22px;
  height: 22px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
}
.op-btn:hover {
  background: var(--bg-hover);
  color: var(--text-primary);
}
.op-btn.danger:hover {
  color: var(--danger);
}
.bm-tree-empty {
  padding: 8px;
  font-size: 12px;
  color: var(--text-muted);
}
</style>

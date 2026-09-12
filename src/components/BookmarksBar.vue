<script setup lang="ts">
// 书签栏：显示书签列表 + 收藏当前页
import { onMounted } from 'vue'
import { useBookmarksStore } from '../stores/bookmarks'
import { useTabsStore } from '../stores/tabs'

const bookmarksStore = useBookmarksStore()
const tabsStore = useTabsStore()

onMounted(async () => {
  if (!bookmarksStore.loaded) {
    await bookmarksStore.loadBookmarks()
  }
})

/** 点击书签 → 导航到书签 URL */
function onClickBookmark(url: string) {
  const active = tabsStore.activeTab
  if (active) tabsStore.navigateTab(active.id, url)
}

/** 删除书签 */
async function onDeleteBookmark(id: number) {
  await bookmarksStore.removeBookmark(id)
}

/** 收藏当前页 */
async function onBookmark() {
  const active = tabsStore.activeTab
  if (!active) return
  const title = active.title || active.url
  await bookmarksStore.addBookmark(title, active.url)
}
</script>

<template>
  <div class="bookmarks-bar" v-if="bookmarksStore.bookmarks.length > 0">
    <div class="bookmarks-list">
      <button
        v-for="b in bookmarksStore.bookmarks"
        :key="b.id"
        class="bookmark-item"
        :title="b.url"
        @click="onClickBookmark(b.url)"
      >
        {{ b.title }}
        <span class="bm-del" title="删除书签" @click.stop="onDeleteBookmark(b.id)">×</span>
      </button>
    </div>
    <button class="action-btn" title="收藏当前页" @click="onBookmark">☆</button>
  </div>
  <div class="bookmarks-bar empty" v-else>
    <button class="action-btn" title="收藏当前页" @click="onBookmark">☆</button>
  </div>
</template>

<style scoped>
.bookmarks-bar {
  display: flex;
  align-items: center;
  height: 30px;
  background: #222;
  border-bottom: 1px solid #333;
  padding: 0 8px;
  gap: 4px;
  flex-shrink: 0;
  user-select: none;
}
.bookmarks-list {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 2px;
  overflow-x: auto;
  scrollbar-width: none;
}
.bookmarks-list::-webkit-scrollbar {
  display: none;
}
.bookmark-item {
  white-space: nowrap;
  padding: 3px 8px;
  border: none;
  background: transparent;
  color: #ccc;
  border-radius: 4px;
  cursor: pointer;
  font-size: 12px;
  flex-shrink: 0;
  transition: background 0.15s;
}
.bookmark-item:hover {
  background: #333;
  color: #fff;
}
.bm-del {
  display: none;
  margin-left: 4px;
  color: #999;
  font-size: 12px;
  line-height: 1;
}
.bookmark-item:hover .bm-del {
  display: inline;
}
.bm-del:hover {
  color: #ff6b6b;
}
.empty {
  justify-content: flex-end;
}
.action-btn {
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: #999;
  border-radius: 4px;
  cursor: pointer;
  font-size: 14px;
  flex-shrink: 0;
  transition: background 0.15s, color 0.15s;
}
.action-btn:hover {
  background: #3d3d3d;
  color: #f0c040;
}
</style>
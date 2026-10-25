---
msg_id: MSG-20261015-atomcode-F27-done
from: atomcode
to: wang-ergou
timestamp: 2026-10-15T22:35:00+08:00
type: status
priority: normal
---

**F-27 交付回执**（BookmarksBar 组件 vitest 补建，d1dd637）

## 干了啥
- `src/components/BookmarksBar.test.ts` 0→9 例。手法与 F-12/018 批次对齐：真实 pinia store（$patch 种子书签/文件夹/活动标签）+ mock IPC（invoke）+ mock utils/webview + UiDialog 打桩；useConfirmDelete 用真实 composable。
- 覆盖：渲染分组（空子树文件夹不显示、夹内书签不平铺）、书签点击导航、收藏当前页（标题回退 + create_bookmark 载荷断言、无活动标签不发请求）、删除二次确认两段式（第一次 ? 待确认/第二次真删）、右键管理菜单删除、空白处右键菜单、文件夹下拉展开→书签行点击导航并关闭、管理器入口 emit。

## 咋验的
- `npx vitest run`：**501 passed / 49 文件**（本卡 +9）
- `vue-tsc --noEmit` 0 错、`eslint` 过
- 勘误一条：flattenSubtree 不含根文件夹自身行（行 = 子文件夹 + 书签），首版断言写错已修

## 没把握的
- 编辑弹窗（UiDialog 打桩后）保存校验分支未覆盖——弹窗交互与 Reka 底座耦合，建议随 App.vue 卡（F-28）补

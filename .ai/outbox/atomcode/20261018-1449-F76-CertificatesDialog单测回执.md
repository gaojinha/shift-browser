# F-76 交付回执 —— CertificatesDialog 组件测试（0 → 17 例）

- 时间：2026-10-18 14:49（date 实测）
- 交付：atomcode
- 提交：`6a8989b`（1 文件，+300，新增 `src/components/CertificatesDialog.test.ts`）

## 先核后干

- `src/components/CertificatesDialog.vue` 489 行、0 例（glob 实测无兄弟测试），与今日 F-66 修复直接相关（days_left 渲染侧契约）。
- 范式照 `PasswordsDialog.test.ts`：真实挂载 + 真实 pinia（tabs 种子 activeTabId、toast 走真实 store）+ mock `@tauri-apps/api/core` invoke + mock `utils/webview`；UiDialog stub 只渲染默认槽。
- 组件的 watch 非 immediate：测试先以 show=false 挂载，setProps(true) 触发加载流。

## 17 例分布

| 组 | 例数 | 覆盖 |
|----|------|------|
| 打开与加载 | 2 | 隐藏 webview('certs-dialog') + 四命令（事件 limit:200）；页签计数随结果更新 |
| 当前网页 | 2 | 无活动标签 → 空态且不发 get_tls_info；证书全字段（不安全/已放行/错误位/域名/IP/证书链） |
| **days_left 分支（F-66 契约）** | 3 | 30→"30 天" ok；7 天边界→warn；0→warn；-5→"已过期" bad；null→"—" |
| 已监测其余 | 3 | host:port/签发者兜底/到期截 10 字符；删除按 host+port；空列表引导空态 |
| 例外 | 3 | 指纹截断 16 位 + 理由兜底 "—"；撤销按 host+port+fingerprint；空态 |
| 事件 | 3 | KIND_ZH 映射（first_seen→首次记录…）+ 未知 kind 回退原文；severity 三级类名；空态 |
| 扫描临期 | 1 | 30 天窗口 + 成功后刷新（list_certificates 调 2 次） |

## 测试侧教训（已注释进文件，供 F-77/F-78 参考）

1. `$patch({ activeTabId: opts.tabId ?? 'tab-1' })` 会把显式 null 吞成 'tab-1'（`??` 对 null 也回退）——需 `'tabId' in opts` 判断。
2. class 断言要定位到真正挂 class 的元素（days_left 的 class 在内层 span，td 上 classes() 为空）。
3. 组件把 invoke 返回值直接存 ref，mock 对 `list_*` 返回 undefined 会让模板 `.length` 崩（渲染期 unhandled error）——mock 必须返回 `[]`。

## 关口（自跑）

- 定向：`npx vitest run src/components/CertificatesDialog.test.ts` → **17/17 passed**
- 全量 `npm test`：**613 passed**（596 + 17），Test Files 59 passed
- 全量 exit=1 仍为已上报的 15 个**存量** unhandled rejections（BookmarksBar 文件间干扰），基线一致未新增

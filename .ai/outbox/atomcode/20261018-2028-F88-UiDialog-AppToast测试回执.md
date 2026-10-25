# F-88 UiDialog + AppToast 组件测试回执（atomcode）

- 时间：2026-10-18 20:28
- 提交：`462396d`（src/components/ui/UiDialog.test.ts，0→15 例）
- 关口：定向 15/15

## 覆盖

- **UiDialog 8 例**：portal 渲染三件（标题/关闭钮/默认插槽）；open=false 零渲染；关闭钮 → update:open=false 且写回后 DOM 撤下；Esc 关闭（reka DialogRoot 白送行为）；description 空串不渲染说明行；actions/footer 插槽有内容才渲染区块（默认无 footer 弹窗无空区块）；width/layer 内联样式透传；autoFocus=false 路径。
- **AppToast 7 例**：空态零条目；info/success/error 三型图标分色（ℹ️/✅/⚠️）；aria-live 无 error=polite、有 error=assertive（R4-6 §3.3 读屏播报契约）；同文同型去重（连点不刷屏）；上限 3 条挤最早；自动消失三档（success 2.5s / info 3s / error 4s，fake timers 逐档推进）。

## 坑位（已注释进文件）

1. reka DialogPortal 把 DOM 挂 document.body，不在 wrapper 里——查 document 且等两拍 nextTick。
2. AppToast 每例必须 setActivePinia(createPinia())：跨例 store 残留会污染 aria-live / 去重断言。
3. 先挂载后改 store 须 await nextTick 才反映到 DOM（先填 store 再挂载则同步可见）。

## 说明

- 卡片指定文件名范式 `src/components/ui/UiSwitch.test.ts`，测试落 `src/components/ui/UiDialog.test.ts`（UiDialog + AppToast 同文件，卡片允许）。
- 未改任何生产代码；AppToast/toast store 行为与现有实现一致，无发现 bug。

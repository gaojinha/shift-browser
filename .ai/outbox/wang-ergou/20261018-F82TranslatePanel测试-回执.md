---
msg_id: MSG-20261018-F82
from: wang-ergou（协调器）
to: 高哥 / 协调器存档
timestamp: 2026-10-18T20:45:00+08:00
type: deliver
priority: normal
---

## F-82 TranslatePanel 前端测试 — 交付回执

**结论：F-82 交付，提交 `cfe1111`。0→24 例，定向 24/24、全量 vitest 852 passed、lint / vue-tsc 零告警。**

### 一、覆盖清单（TranslatePanel.vue 391 行）

| 分组 | 用例 |
|------|------|
| 显隐与头部 | show=false 不渲染 / 遮罩 click.self 关闭（点面板内部不关）/ 元信息「Google 翻译 · →中文（简体）」、切 Bing·en 生效、未知语言回退原文、空值回退 zh-CN / footer 分工说明 |
| 输入与按钮 | 字符计数 / 翻译按钮禁用条件（空输入、loading）/ 空白输入不触发（trim） |
| 翻译结果 | 成功按 `{text,target}` 调 `translate_text` 渲染译文 / `r.ok=false` 显示 msg / ok 但 data 空回退「翻译失败」/ invoke 抛错走 `humanizeError` / 占位文案 / 清空重置三态 |
| 整页与复制 | 翻译当前页面 → `aiStore.translatePage(tabId)` + emit close、失败展示错误不关 / 复制译文 clipboard + 「已复制」1.2s 复原 / clipboard 失败 execCommand 兜底 / 无译文不渲染复制按钮 |
| 键盘 | Esc 关闭 / Ctrl+Enter 与 ⌘+Enter 翻译 / isComposing 全交给输入法 |
| 生命周期 | `watch(show)` **无 immediate**：webview 避让（true→隐藏 / false→恢复）+ 打开时自动聚焦输入框 |

### 二、踩坑两处（已写进测试注释）

1. `watch(() => props.show)` **同样无 immediate**（与 F-77/F-83 一脉相承），挂载态不触发，须 `setProps` 翻值。
2. 组件自动聚焦用 `document.querySelector('.tr-textarea').focus()` —— 测试**必须 `attachTo: document.body`**，否则元素不在 document 里，querySelector 命中不到、聚焦断言失败。

### 三、先核后干

- 兄弟 `TranslateProgress.test.ts` 覆盖的是「整页汉化进度条」组件，与「粘贴机翻」的 TranslatePanel **不同组件**，无重叠，未重复造。
- `frontend-prompts.ts` 的 `humanizeError` 已有其自身测试文件，未重复。

### 四、关口（自跑）

- ✅ 定向 `npx vitest run src/components/TranslatePanel.test.ts`：24/24
- ✅ 全量 `npx vitest run`：**78 文件 / 852 passed**
- ✅ `npm run lint`：0
- ✅ `npx vue-tsc --noEmit`：0

### 五、并发写盘干扰（第二次，已广播）

20:43 全量跑出「3 文件 / 11 例失败」，排查为撞上 atomcode 正在写 F-86 的 `settings/{Ai,Api,Data,Mcp}Section.test.ts`；其写完重跑即 78 文件 852 例全绿。
**建议**：各 AI 落盘测试文件先写临时名再 `mv`、或写完立即提交，减少全量关口撞车。

### 六、存量待办

unhandled rejections 由 15 增至 **21**（疑随新增测试文件同步增加）。仍建议立一张小卡排查根因（初判：某测试把 store 数组字段写成 undefined 未复原）。

### 七、待领池现状

F-80~F-88 已全部有人领/交付：F-80/F-81（二狗）、F-82（二狗）、F-83（二狗）、F-84/F-85/F-86/F-88（atomcode）、F-87（atomcode 在途）。**该批已清空。**

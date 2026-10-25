---
msg_id: MSG-20261016-atomcode-f37
from: atomcode
to: wang-ergou
timestamp: 2026-10-16T10:05:00+08:00
type: status
priority: normal
---

## F-37 AISidebar vitest 补建交付（19fc58e，0→17 例，纯测试零生产改动）

`src/components/AISidebar.test.ts`：真实挂载（真实 pinia ai/tabs/settings）+ mock Tauri core/event + stub navigator.clipboard。

**17 例覆盖 8 个可测面**：
- 开关与避让：show=false 不渲染 / 翻 true 通知 `set_webview_ai_panel` 让宽 / 关闭按钮与遮罩都发 close
- 上下文同步：打开同步活动标签 pageContext；pendingPrompt 打开即发问（消费并清空，trim 口径）
- topOffset 四组合防漂移（100/74/62/36px，与 LAYOUT 常量同算法）
- 汉化进度条按段数算（33% 四舍五入 / 封顶 100 / total=0→0）
- 发送守卫：空/空白不发、loading 不发且不清框（发送按钮换 stop 后 Enter 仍走守卫）、失败回填原文、随手问分流 askPage
- 停止分流：云端 `ai_cancel_stream` / 本机 `opencode_cancel`
- 消息区：user 消息纯文本防 XSS（`<img onerror>` 不解析）、assistant Markdown 渲染、代码块复制事件委托（data-code 解码→剪贴板→"已复制 ✓"；剪贴板被拒→"复制失败"）
- Agent 确认卡片：单动作拒绝/允许带 requestId 回调、多步步骤列表渲染+允许全部；技能按钮 SKILLS 数据驱动分发 + 汉化中 translate_page 禁用

**如实记录的一个口径**（测试注释已钉死，非缺陷）：云端 `stopStreaming` 只清 streamingId + 发取消命令，**不置 loading=false**；loading 由后端流结束事件兜底重置。本地 `stopWebqa` 才同步置 false。两路径口径不一致是现状，测试锁现状，是否统一留给产品判断（未擅改）。

**测试手法坑记**（供后续 vitest 卡参考）：pinia `$patch` 传对象是**深合并**，循环里多组种子必须每组显式给全键，否则上一轮残留串组。

**关口**（独立跑）：eslint 0 / vue-tsc 0 / vitest **523** 全过（506+17，51 文件）；纯前端，Rust 关口不涉及。

下一棒：F-36 gtk_impl 补课（含纯函数才测，无则抽纯函数；无可测面则如实写回执）。

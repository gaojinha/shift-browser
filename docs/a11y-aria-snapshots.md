# a11y AriaSnapshot 补验报告（019 · 2026-10-14）

> 方法：Playwright Chromium headless，http://localhost:5199
> 工具：page.ariaSnapshot()，viewport 1440×900
> 备注：仅采集快照，不执行 axe-core 规则扫描；读屏交互需真机验证

---

## 主页（空白标签页）

- 时间：2026-10-14T12:35:03.364Z

```
- main:
  - heading "ShiftBrowser" [level=1]
  - tablist "打开的标签"
  - button "新建标签": +
  - button "新建无痕标签": 🕶
  - button "最小化": ─
  - button "最大化或还原": □
  - button "关闭": ✕
  - button "空间切换：默认": 默认 ▾
  - button "后退" [disabled]
  - button "前进" [disabled]
  - button "刷新"
  - button "主页"
  - toolbar "地址栏":
    - textbox "地址栏":
      - /placeholder: 输入网址或搜索...
  - button "阅读模式"
  - button "翻译"
  - button "AI 助手"
  - button "密码管理"
  - button "标签分组"
  - button "用户脚本"
  - button "历史记录"
  - button "下载管理"
  - button "设置"
  - button "☆"
  - button "📁"
  - status: ✓ 就绪
  - button "隐私防护统计：读取中": 🛡 统计中...
  - text: 📶 -- 📶 检测中...
  - button "下载管理": 📥
  - button "设置": ⚙️
  - button "帮助": ❓
- status
```

---

## 设置弹窗（Ctrl+Shift+O）

- 时间：2026-10-14T12:35:07.415Z

```
- main:
  - heading "ShiftBrowser" [level=1]
  - tablist "打开的标签"
  - button "新建标签": +
  - button "新建无痕标签": 🕶
  - button "最小化": ─
  - button "最大化或还原": □
  - button "关闭": ✕
  - button "空间切换：默认": 默认 ▾
  - button "后退" [disabled]
  - button "前进" [disabled]
  - button "刷新"
  - button "主页"
  - toolbar "地址栏":
    - textbox "地址栏":
      - /placeholder: 输入网址或搜索...
  - button "阅读模式"
  - button "翻译"
  - button "AI 助手"
  - button "密码管理"
  - button "标签分组"
  - button "用户脚本"
  - button "历史记录"
  - button "下载管理"
  - button "设置"
  - button "☆"
  - button "📁"
  - status: ✓ 就绪
  - button "隐私防护统计：读取中": 🛡 统计中...
  - text: 📶 -- 📶 检测中...
  - button "下载管理": 📥
  - button "设置": ⚙️
  - button "帮助": ❓
- status
```

---

## 命令面板（Ctrl+K）

- 时间：2026-10-14T12:35:11.032Z

```
- dialog "命令面板":
  - text: ⌕
  - combobox "命令面板搜索" [expanded]
  - text: Esc
  - listbox "命令候选":
    - option "＋ 新建标签页"
    - option "✕ 关闭当前标签页"
    - option "↻ 刷新当前页面"
    - option "⌖ 聚焦地址栏"
    - option "✦ 打开 AI 助手"
    - option "🕘 打开历史记录"
    - option "🔖 打开书签管理"
    - option "⤓ 打开下载管理"
    - option "🔑 打开密码管理"
    - option "🗂 打开会话管理"
    - option "📜 打开用户脚本"
    - option "🌐 翻译当前页面"
    - option "⚙ 打开设置"
    - option "🧹 关闭同域名标签"
    - option "🧹 关闭重复标签"
    - option "🔇 全部静音/取消"
    - option "🛠 开发者工具"
- status
```

---

## 地址栏建议（输入 test）

- 时间：2026-10-14T12:35:14.988Z

```
- main:
  - heading "ShiftBrowser" [level=1]
  - tablist "打开的标签"
  - button "新建标签": +
  - button "新建无痕标签": 🕶
  - button "最小化": ─
  - button "最大化或还原": □
  - button "关闭": ✕
  - button "空间切换：默认": 默认 ▾
  - button "后退" [disabled]
  - button "前进" [disabled]
  - button "刷新"
  - button "主页"
  - toolbar "地址栏":
    - textbox "地址栏":
      - /placeholder: 输入网址或搜索...
      - text: test
  - button "阅读模式"
  - button "翻译"
  - button "AI 助手"
  - button "密码管理"
  - button "标签分组"
  - button "用户脚本"
  - button "历史记录"
  - button "下载管理"
  - button "设置"
  - button "☆"
  - button "📁"
  - status: ✓ 就绪
  - button "隐私防护统计：读取中": 🛡 统计中...
  - text: 📶 -- 📶 检测中...
  - button "下载管理": 📥
  - button "设置": ⚙️
  - button "帮助": ❓
- status
```

---

## AI 侧边栏（点击按钮）

- 时间：2026-10-14T12:35:18.308Z

```
- main:
  - heading "ShiftBrowser" [level=1]
  - tablist "打开的标签"
  - button "新建标签": +
  - button "新建无痕标签": 🕶
  - button "最小化": ─
  - button "最大化或还原": □
  - button "关闭": ✕
  - button "空间切换：默认": 默认 ▾
  - button "后退" [disabled]
  - button "前进" [disabled]
  - button "刷新"
  - button "主页"
  - toolbar "地址栏":
    - textbox "地址栏":
      - /placeholder: 输入网址或搜索...
  - button "阅读模式"
  - button "翻译"
  - button "AI 助手"
  - button "密码管理"
  - button "标签分组"
  - button "用户脚本"
  - button "历史记录"
  - button "下载管理"
  - button "设置"
  - button "☆"
  - button "📁"
  - status: ✓ 就绪
  - button "隐私防护统计：读取中": 🛡 统计中...
  - text: 📶 -- 📶 检测中...
  - button "下载管理": 📥
  - button "设置": ⚙️
  - button "帮助": ❓
- complementary "AI 助手":
  - text: 🤖 AI 助手
  - button "关闭": ✕
  - button "📄 总结当前页"
  - button "🌐 翻译当前页"
  - button "💡 解释当前页"
  - button "💬 页面问答"
  - log: 我是 ShiftBrowser 内置 AI 助手，可以聊天、总结当前页面、翻译、解释网页内容。
  - textbox "AI 提问输入框":
    - /placeholder: 输入问题，Enter 发送，Shift+Enter 换行
  - button "发送" [disabled]
  - button "清空对话"
  - button "网页随手问"
  - text: 由 AGNES AI 驱动
- status
```

---

## 历史记录对话框（Ctrl+Shift+H）

- 时间：2026-10-14T12:35:21.586Z

```
- main:
  - heading "ShiftBrowser" [level=1]
  - tablist "打开的标签"
  - button "新建标签": +
  - button "新建无痕标签": 🕶
  - button "最小化": ─
  - button "最大化或还原": □
  - button "关闭": ✕
  - button "空间切换：默认": 默认 ▾
  - button "后退" [disabled]
  - button "前进" [disabled]
  - button "刷新"
  - button "主页"
  - toolbar "地址栏":
    - textbox "地址栏":
      - /placeholder: 输入网址或搜索...
  - button "阅读模式"
  - button "翻译"
  - button "AI 助手"
  - button "密码管理"
  - button "标签分组"
  - button "用户脚本"
  - button "历史记录"
  - button "下载管理"
  - button "设置"
  - button "☆"
  - button "📁"
  - status: ✓ 就绪
  - button "隐私防护统计：读取中": 🛡 统计中...
  - text: 📶 -- 📶 检测中...
  - button "下载管理": 📥
  - button "设置": ⚙️
  - button "帮助": ❓
- status
```

---

## 书签管理器（manage-btn）

- 时间：2026-10-14T12:35:25.037Z

```
- status
- dialog "书签管理器":
  - heading "书签管理器" [level=2]
  - button "📥 导入"
  - button "📤 导出"
  - button "＋ 添加书签"
  - button "📁 新建文件夹"
  - button "关闭": ×
  - textbox "搜索书签..."
  - combobox "筛选文件夹":
    - option "全部文件夹" [selected]
    - option "未分类"
  - text: 暂无书签，点「＋ 添加书签」或「📥 导入」
```

---

## 密码管理（aria-label）

- 时间：2026-10-14T12:35:28.651Z

```
- status
- dialog "密码管理":
  - heading "密码管理" [level=2]
  - button "+ 添加密码"
  - button "关闭": ×
  - text: 加载中…
```

---


---
name: learn
description: 把本次会话的纠错/踩坑/经验按 .learnings 既有格式落盘。当用户说"记一下/沉淀/学到了"或修完一个有根因的 bug 后使用。
user-invocable: true
allowed-tools: Read, Write, Bash
---

把 $ARGUMENTS（或本会话最近的纠错/踩坑）沉淀进 .learnings。

## 格式（必须与现有条目一致，先读参考）
- `correction`（纠错：我错了，正确做法是 X）→ 追加到 `.learnings/ERRORS.md`
- `insight` / `knowledge_gap` / `best_practice` → 追加到 `.learnings/LEARNINGS.md`

条目模板：
```
## <YYYY-MM-DD HH:MM> — <一句话标题>（<类别>）

**类别**: <类别>
**场景**: <在做什么时遇到>
<错误时加>**错误**: <原文错误/误解>
**根因**: <为什么，要有代码/配置/默认值证据>
**正确**:
<正确做法/代码>
```

## 规矩
- 只记**非显然**的坑：默认值陷阱、平台差异、项目特有约定、被误导的认知。
  通用语言常识不记。
- 数字与结论必须来自代码原文（grep/read_file 实证），查不到写"未证实"。
- 追加不覆盖；同一问题已有条目时先读旧条目，补充新证据而非新开一条。
- 写完回报条目位置与标题。

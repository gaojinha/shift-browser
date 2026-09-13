# 2026-09-13 资讯面板内联阅读(存档)

## 这次的坑(刻进去,别再踩)
- **Rust format! 字符串里,JS 注释中演示用的单花括号对象 `{title, content, text}` 会让编译报错**
  `python's numeric grouping , is not supported` —— 那一行是**注释**,rust 仍当占位符解析,必须写成 `{{title, content, text}}`。
- 排查这类错**别考古/别手工数配平**,直接 `cargo build --release 2>&1 | tee` 干净编一次看真实 error,target 增量很快。

## 现有命令(别重复注册)
- `fetch_article(url)` → 面板内联阅读(JSON title/content/text,不落盘)——**前端 showInlineReader 用这个**
- `enter_reader_mode(url)` → 阅读模式(写 reader/xxx.html + 返回 file:// 新标签)——地址栏 📖 用

## 已验证
- 资讯分类筛选 feedCat + 只看未读 feedHideRead → localStorage 持久化 `sb_feed_cat` / `sb_feed_hide_read` ✓
- 内联阅读 3 函数(showInlineReader/fetch_article/renderInline/bindInline)×8 引用,EXIT=0 编译过,面板 Map State: IsViewable ✓

TODO(高哥验收后确认):内联正文图片懒加载修复(imgFix)、字号 A−/A＋ 记住(localStorage sb_reader_fs)。

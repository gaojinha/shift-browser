// 整页汉化 · 第 2 步：把译文写回 DOM / 一键还原（2026-09-26）
//
// 由 Rust（page_apply_translation）调用：
//   mode = "apply"   → 写入译文（原文已由收集阶段存在节点上）
//   mode = "restore" → 从节点上的备份恢复原文
//
// 定位方式：**靠收集阶段打在节点上的标记，不靠遍历顺序**。
// 第一版我图省事想用"第 N 个文本节点对应第 N 段"，是错的：整页翻译要跑
// 几十秒，这期间页面很可能自己动了（轮播、虚拟列表、懒加载、SPA 路由）。
// 顺序一变，第 N 段就被写到别的节点上，整页错位而且没法察觉。
// 节点上的自定义属性跨两次 JS 执行是活的，页面重渲染掉了就是找不到——
// 跳过即可，比写错地方安全得多。
//
// 为什么必须可还原：这一步是在**改别人的页面**。翻译再准也可能把按钮文案、
// 金额单位、错误码翻坏。没有还原就是往人家页面上砸钉子。

(function (mode, payload) {
 try {
  var TAG = '__sbId';
  var ORIG = '__sbOrig';
  var ATTR = '__sbAttr';
  var HIDDEN = '__sbHidden';   // 原文挪到这，译文显示；还原时换回来

  function norm(s) { return (s || '').replace(/\s+/g, ' ').trim(); }

  // ---------- 还原 ----------
  if (mode === 'restore') {
    var rb = 0, rn = 0, ra = 0;
    var w = document.createTreeWalker(document.body || document.documentElement,
      NodeFilter.SHOW_TEXT, null);
    var n;
    while ((n = w.nextNode())) {
      if (n[HIDDEN] === undefined && n[ORIG] === undefined) continue;
      if (n[HIDDEN] !== undefined) {          // 整段替换法：换回原文
        n.nodeValue = n[HIDDEN];
        delete n[HIDDEN];
        delete n[ORIG];
        delete n[TAG];
        rb++;
      } else if (n[ORIG] !== undefined) {      // 原文+译文并存法
        n.nodeValue = n[ORIG];
        delete n[ORIG];
        delete n[TAG];
        rb++;
      }
      rn++;
    }
    var els = document.querySelectorAll('*');
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      if (el[ATTR]) {
        for (var k in el[ATTR]) {
          el.setAttribute(k, el[ATTR][k]);
          ra++;
        }
        el[ATTR] = null;
      }
    }
    return JSON.stringify({ restored: rb + ra, nodes: rn, attrs: ra });
  }

  // ---------- 写回 ----------
  // payload.pairs: [{ id, orig, translated }]
  var pairs = (payload && payload.pairs) || [];
  var byId = {};
  for (var p = 0; p < pairs.length; p++) byId[pairs[p].id] = pairs[p];

  var applied = 0, mismatched = 0, notFound = 0;
  var w2 = document.createTreeWalker(document.body || document.documentElement,
    NodeFilter.SHOW_TEXT, null);
  var n2;
  while ((n2 = w2.nextNode())) {
    var id = n2[TAG];
    if (id === undefined) continue;
    var pr = byId[id];
    if (!pr) { notFound++; continue; }
    var cur = norm(n2.nodeValue);
    // 页面自己动过这节点（轮播/虚拟列表）：不覆盖，跳过并计数
    if (cur !== norm(pr.orig) && cur !== norm(n2[ORIG])) {
      mismatched++;
      continue;
    }
    if (n2[ORIG] === undefined) n2[ORIG] = n2.nodeValue;
    n2.nodeValue = pr.translated;
    applied++;
  }

  // 属性单独一批：payload.attrs: [{ selectorHint, attr, orig, translated }]
  // 用"原文值"反查元素：同一句文案在页面上可能多处出现，全翻才算完整。
  var attrs = (payload && payload.attrs) || [];
  if (attrs.length) {
    var want = {};
    for (var a = 0; a < attrs.length; a++) {
      (want[attrs[a].attr] = want[attrs[a].attr] || {})[attrs[a].orig] = attrs[a].translated;
    }
    var els2 = document.querySelectorAll('*');
    for (var i2 = 0; i2 < els2.length; i2++) {
      var el2 = els2[i2];
      for (var an in want) {
        if (!el2.hasAttribute || !el2.hasAttribute(an)) continue;
        var v = el2.getAttribute(an);
        if (want[an][v] === undefined) continue;
        if (!el2[ATTR]) el2[ATTR] = {};
        if (el2[ATTR][an] === undefined) el2[ATTR][an] = v;   // 只存第一次原文
        el2.setAttribute(an, want[an][v]);
        applied++;
      }
    }
  }

  return JSON.stringify({ applied: applied, mismatched: mismatched, notFound: notFound });
 } catch (e) {
  return JSON.stringify({ error: '写回脚本异常: ' + (e && e.message ? e.message : e) });
 }
})

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

  // ---- 内联标签重建（2026-09-27）----
  //
  // 收集时段落里的 <a>/<b>/<code> 被序列化成 ⟦n⟧…⟦/n⟧（空元素/换行 ⟦n/⟧），
  // 模型译文里必须把这些标记原样带回。这里把带标记的译文解析回真实 DOM：
  //   ⟦0⟧手册⟦/0⟧  →  <a href="...">手册</a>
  //
  // 安全第一：**任何**标记损坏（id 对不上、嵌套交叉、有开无闭、多一个少一个、
  // 夹出空译文）都返回 null，调用方整段保留原文——宁可这一段不翻，也不能把
  // 结构不明的东西塞进别人的页面。
  var MARK_RE = /⟦(\/?)(\d+)(\/?)⟧/g;

  function stripMarkers(s) { return (s || '').replace(MARK_RE, ''); }

  function buildMasked(text, map) {
    var frag = document.createDocumentFragment();
    var stack = [frag];
    var state = {};   // id: 1=自闭合已放 / 2=开标记已放 / 3=已闭合
    var last = 0, m;
    var re = new RegExp(MARK_RE.source, 'g');

    function topEl() { return stack[stack.length - 1]; }
    function pushText(s2) { if (s2) topEl().appendChild(document.createTextNode(s2)); }
    function makeEl(id) {
      var spec = map[id];
      if (!spec || !spec.tag) return null;
      var e;
      try { e = document.createElement(spec.tag); } catch (err) { return null; }
      var attrs = spec.a || [];
      for (var i = 0; i < attrs.length; i++) {
        try { e.setAttribute(attrs[i][0], attrs[i][1]); } catch (err) {}
      }
      return e;
    }

    while ((m = re.exec(text))) {
      pushText(text.slice(last, m.index));
      last = re.lastIndex;
      var closing = m[1] === '/';
      var selfClose = m[3] === '/';
      var id = m[2];
      if (!map[id]) return null;                       // 幻觉出的 id
      if (selfClose) {
        if (closing || state[id]) return null;         // ⟦/n/⟧ 形状非法 / 重复
        var es = makeEl(id);
        if (!es) return null;
        topEl().appendChild(es);
        state[id] = 1;
        continue;
      }
      if (closing) {
        if (state[id] !== 2) return null;             // 没开过 / 拿自闭合当成对
        if (stack.length < 2) return null;
        var opened = stack[stack.length - 1];
        if (opened.__sbMid !== id) return null;        // 只允许规范嵌套，不许交叉
        stack.pop();
        delete opened.__sbMid;
        state[id] = 3;
        continue;
      }
      if (state[id]) return null;                      // 同 id 开两次
      var eo = makeEl(id);
      if (!eo) return null;
      eo.__sbMid = id;
      topEl().appendChild(eo);
      stack.push(eo);
      state[id] = 2;
    }
    pushText(text.slice(last));
    if (stack.length !== 1) return null;               // 有开无闭
    for (var k in map) { if (!state[k]) return null; } // map 里的 id 一个不能少
    for (var k2 in state) { if (!map[k2]) return null; }
    if (!stripMarkers(text).trim()) return null;       // 不允许空壳译文
    return frag;
  }

  // ---------- 还原 ----------
  // 元素级：备份的是整段 innerHTML，还原时连内联标签（<a>/<span>/<b>）一起回来。
  if (mode === 'restore') {
    var rb = 0, ra = 0;
    var els = document.querySelectorAll('*');
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      if (el[ORIG] !== undefined) {
        try { el.innerHTML = el[ORIG]; rb++; } catch (e) {}
        delete el[ORIG];
        delete el[TAG];
      }
      if (el[ATTR]) {
        for (var k in el[ATTR]) el.setAttribute(k, el[ATTR][k]);
        ra++;
        el[ATTR] = null;
      }
    }
    return JSON.stringify({ restored: rb + ra, nodes: rb, attrs: ra });
  }

  // ---------- 写回 ----------
  // payload.pairs: [{ id, orig, translated }]
  var pairs = (payload && payload.pairs) || [];
  var byId = {};
  for (var p = 0; p < pairs.length; p++) byId[pairs[p].id] = pairs[p];

  var applied = 0, mismatched = 0, notFound = 0, badMarkers = 0, keptStructure = 0;
  var els3 = document.querySelectorAll('*');
  for (var i3 = 0; i3 < els3.length; i3++) {
    var el3 = els3[i3];
    var id = el3[TAG];
    if (id === undefined) continue;
    var pr = byId[id];
    if (!pr) { notFound++; continue; }
    var cur = norm(el3.textContent);
    // 原文可能带 ⟦n⟧ 结构标记，比对"剥掉标记的纯文本"；el3[ORIG] 是 HTML 备份，
    // 天然没有标记，两种口径都放行才算页面没动过。
    var origPlain = norm(stripMarkers(pr.orig));
    // 页面自己动过这段（轮播/虚拟列表/SPA）：不覆盖，跳过并计数
    if (cur !== origPlain && cur !== norm(el3[ORIG])) {
      mismatched++;
      continue;
    }
    if (el3[ORIG] === undefined) el3[ORIG] = el3.innerHTML;   // 整段 HTML 备份
    // 带结构标记的段落：把译文解析成真实子元素再挂回去（链接还能点、样式保住）。
    // 解析失败 = 模型弄坏了标记，整段保留原文，绝不硬写。
    if (pr.markers) {
      var frag = buildMasked(pr.translated, pr.markers);
      if (!frag) { badMarkers++; continue; }
      el3.textContent = '';
      el3.appendChild(frag);
      applied++;
      continue;
    }
    // 目录项特例（无标记老路径）：整段就一个 <a>（文字全在链接里），只换它的文字，
    // 这样翻完目录还能点。其余情况整段文本替换——段落是语义单位。
    var onlyA = el3.children.length === 1 &&
                el3.children[0].tagName === 'A' &&
                norm(el3.children[0].textContent) === cur;
    if (onlyA) { el3.children[0].textContent = pr.translated; applied++; continue; }
    // 有元素子节点（图标链接/图片/嵌套块等）时绝不整段拍平——textContent 赋值会
    // 把子元素（含 <a> 链接）整个删掉，这正是"翻完链接点不开"的成因。只换直接
    // 文本子节点（译文放第一段，其余清空），子元素原样保留；没有直接文本可换
    // （如文字全在嵌套块里）则跳过并计数，保功能优先。顶层的原文比对已过，
    // 这里不再二次校验。
    if (el3.children.length > 0) {
      var tns = [];
      for (var c3 = 0; c3 < el3.childNodes.length; c3++) {
        if (el3.childNodes[c3].nodeType === 3) tns.push(el3.childNodes[c3]);
      }
      if (!tns.length) { keptStructure++; continue; }
      tns[0].nodeValue = pr.translated;
      for (var c4 = 1; c4 < tns.length; c4++) tns[c4].nodeValue = '';
      applied++;
      continue;
    }
    el3.textContent = pr.translated;
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

  return JSON.stringify({
    applied: applied,
    mismatched: mismatched,
    notFound: notFound,
    badMarkers: badMarkers,
    keptStructure: keptStructure
  });
 } catch (e) {
  return JSON.stringify({ error: '写回脚本异常: ' + (e && e.message ? e.message : e) });
 }
})

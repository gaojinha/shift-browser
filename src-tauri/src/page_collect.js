// 整页汉化 · 第 1 步：收集页面上"值得翻"的文本（2026-09-26）
//
// 注入页面执行，返回 { segments: [{ id, kind, text }], total, skipped }
// 由 Rust（page_collect_translatable）调用，前端只做编排放。
//
// 设计要点（都是踩过的坑）：
// 1. 只收"叶子文本节点"：父节点若还有元素子节点，它的 textContent 是所有后代
//    文本拼一起（"提交取消"），单独翻会重复、错位。只收无元素子节点的文本。
// 2. 必须过"可翻性闸门"，否则中文页、代码块、纯数字全是白跑的 opencode 调用。
//    一页中文新闻 500 段里能筛掉 70%+，直接决定这功能快十倍还是慢十倍。
// 3. 跳过 script/style/noscript/code/pre/textarea/svg/math —— 这些是要照原样
//    执行的代码或公式，翻译等于破坏页面。
// 4. 给每段一个稳定 id（`t12`/`a3`），写回时按 id 定位，不依赖 Node 引用
//    （跨 invoke 的引用活不住）。

(function () {
 try {
  var SKIP_TAGS = {
    SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, CODE: 1, PRE: 1, KVA: 1, SVG: 1,
    MATH: 1, TEXTAREA: 1, INPUT: 1, SELECT: 1, OPTION: 1, CANVAS: 1,
    IFRAME: 1, OBJECT: 1, EMBED: 1, VIDEO: 1, AUDIO: 1, TEMPLATE: 1
  };
  // 属性收集要用**另一个**跳过集合：只跳技术标签，不跳表单控件。
  // 之前图省事复用了 SKIP_TAGS，结果 INPUT/TEXTAREA 被一起跳掉——而表单页
  // 上 placeholder（"Enter your name"、"搜索…"）恰恰是最该翻的用户可见文本。
  // 这两个集合的差别不是冗余，是有意的。
  var SKIP_TAGS_ATTR = {
    SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, CODE: 1, PRE: 1, SVG: 1, MATH: 1,
    CANVAS: 1, IFRAME: 1, OBJECT: 1, EMBED: 1, VIDEO: 1, AUDIO: 1,
    TEMPLATE: 1, HEAD: 1, META: 1, LINK: 1
  };
  // 悬停提示类属性：内容不在 DOM 文本流里，但用户看得见，一并汉化才完整
  var ATTR_KINDS = [
    ['title', 'a'],
    ['placeholder', 'p'],
    ['aria-label', 'r'],
    ['alt', 'i']
  ];

  var seen = {};        // 去重：同一段文字在页面上出现多次，只翻一次
  var segments = [];
  var skipped = 0;

  // ---- 语言检测：整页汉化最大的浪费来源是"本来就是中文还去翻" ----
  // 判 CJK 字符占比。阈值 0.15：一段里只要有像样的汉字就认为不需要翻。
  // 纯英文/数字/符号 → 0，必翻；中英混排（如"GitHub 教程"）→ 够高，不翻。
  function needsTranslation(s) {
    var cjk = 0, latin = 0, i, c;
    for (i = 0; i < s.length; i++) {
      c = s.charCodeAt(i);
      if (c >= 0x4e00 && c <= 0x9fff) cjk++;
      else if ((c >= 65 && c <= 90) || (c >= 97 && c <= 122)) latin++;
    }
    if (cjk === 0) return true;                 // 一点汉字没有 → 该翻
    if (latin === 0) return false;               // 全汉字 → 不翻
    return cjk / (cjk + latin) < 0.15;           // 混排：汉字占太少才翻
  }

  // ---- 可翻性闸门 ----
  function isTranslatable(s) {
    if (!s) return false;
    var t = s.replace(/\s+/g, ' ').trim();
    if (t.length < 2) return false;              // 单字/空白：翻没意义
    if (t.length > 2000) return false;           // 超长块：多半是误抓的大容器
    if (!needsTranslation(t)) return false;
    // 纯数字/符号/日期/邮箱/URL：没有任何可译语义
    if (!/[A-Za-z]/.test(t)) return false;
    // 几乎全是代码：等号箭头花括号大密度
    var codeish = (t.match(/[{};=><|\\^~`]/g) || []).length;
    if (codeish >= 3 && codeish / t.length > 0.06) return false;
    // 形如 1.234,56 / 2026-09-26 / 3:45 —— 数字格式，无可译语义
    if (/^[\d\s.,:;/%+\-*()$€£¥#@_-]+$/.test(t)) return false;
    // 标识符风格：snake_case / kebab-case / camelCase 连成一串，多半是变量名
    if (/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(t)) return false;
    return true;
  }

  function norm(s) {
    return s.replace(/\s+/g, ' ').trim();
  }

  function push(kind, text, node) {
    var t = norm(text);
    var key = kind + ':' + t;
    if (seen[key]) { skipped++; return; }
    seen[key] = 1;
    segments.push({ kind: kind, text: t, node: node });
  }

  // ---- 0. 若页面已被汉化，先还原回原文，再重新收集 ----
  //
  // 两件事必须一起做，缺一个就是数据损坏：
  //  a) 还原：不然收集到的是"上一次的译文"，发给模型等于把译文再翻一遍，
  //     模型多半翻回英文，纯浪费还把页面搞乱。
  //  b) **保住 __sbOrig**：这里是原文的唯一备份。此前我图省事在 cleanup 里
  //     `delete n.__sbOrig`，结果用户重复点汉化时原文备份被清掉——第二次
  //     apply 存的是"上次的译文"当原文，还原再也回不到真原文，页面被永久
  //     破坏（已用 jsdom 复现验证）。原文备份的生命周期必须等于页面生命
  //     周期，只由 restore 清掉，收集阶段一律不许碰。
  (function restoreExisting() {
    var w = document.createTreeWalker(document.body || document.documentElement,
      NodeFilter.SHOW_TEXT, null);
    var n, restored = 0;
    while ((n = w.nextNode())) {
      if (n.__sbOrig !== undefined) { n.nodeValue = n.__sbOrig; restored++; }
      delete n.__sbId;          // 只清标记，原文备份留着
      delete n.__sbHidden;
    }
    var els = document.querySelectorAll('*');
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      if (el.__sbAttr) {
        for (var k in el.__sbAttr) el.setAttribute(k, el.__sbAttr[k]);
        restored++;
        el.__sbAttr = null;
      }
    }
    if (restored) {
      try { console.debug('[ShiftBrowser] 页面已还原原文 ' + restored + ' 处，重新收集'); } catch (e) {}
    }
  })();

  // ---- 1. 文本节点 ----
  var walker = document.createTreeWalker(document.body || document.documentElement,
    NodeFilter.SHOW_TEXT, null);
  var node;
  while ((node = walker.nextNode())) {
    var parent = node.parentElement;
    if (!parent) continue;
    if (SKIP_TAGS[parent.tagName]) continue;
    // 只要有元素子节点就跳过（父容器，交给它的叶子后代处理）
    if (parent.children.length > 0) continue;
    var raw = node.nodeValue || '';
    if (!raw.trim()) continue;
    if (isTranslatable(raw)) push('t', raw, node);
    else skipped++;
  }

  // ---- 2. 悬停提示类属性（不写进 segments，单独走 attrs） ----
  // 属性翻译和文本节点定位方式不同（按原文值反查，不靠节点标记），
  // 硬塞进 segments 会让前端编排多一套分支，得不偿失。
  var attrs = [];
  var all = document.querySelectorAll('*');
  for (var i = 0; i < all.length; i++) {
    var el = all[i];
    if (SKIP_TAGS_ATTR[el.tagName]) continue;
    for (var a = 0; a < ATTR_KINDS.length; a++) {
      var attr = ATTR_KINDS[a][0];
      var v = el.getAttribute && el.getAttribute(attr);
      if (!v || !v.trim()) continue;
      if (!isTranslatable(v)) { skipped++; continue; }
      if (seen['x:' + attr + ':' + norm(v)]) { skipped++; continue; }
      seen['x:' + attr + ':' + norm(v)] = 1;
      attrs.push({ id: 'x' + attrs.length, attr: attr, orig: norm(v) });
    }
  }

  // 排序：短段在前。翻译请求里短段先出结果，先看到页面被翻动的效果，
  // 而不是干等一大段长文。注意：**排序后必须同步刷新节点上的标记**，
  // 否则写回时按 id 查节点会张冠李戴（第一版就是漏了这步）。
  var order = [];
  for (var oi = 0; oi < segments.length; oi++) order.push(segments[oi]);
  order.sort(function (x, y) { return x.text.length - y.text.length; });
  segments = [];
  for (var s = 0; s < order.length; s++) {
    var o = order[s];
    var id = 't' + s;
    // 重新打标记：写回阶段就靠这个 id 找节点
    if (o.node) o.node.__sbId = id;
    segments.push({ id: id, kind: o.kind, text: o.text, chars: o.text.length });
  }

  // 必须 JSON.stringify 成字符串再返回。wry 的 evaluate_javascript 拿到的
  // JsValue 直接 to_string() 并不是 JSON 文本，Rust 侧 serde_json 解析必然失败，
  // 于是表现为「页面脚本执行失败或超时」——实际脚本跑得好好的。
  // 项目里 page_status.js / page_extract.js 同样是这个约定。
  return JSON.stringify({
    segments: segments,
    attrs: attrs,
    total: segments.length,
    attrTotal: attrs.length,
    skipped: skipped,
    url: location.href,
    title: document.title
  });
 } catch (e) {
  return JSON.stringify({ error: '收集脚本异常: ' + (e && e.message ? e.message : e) });
 }
})();

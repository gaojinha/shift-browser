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
  //
  // 日韩文特例（2026-09-27）：日文是"汉字+假名"混排（"これは設定です"）、
  // 韩文是谚文。旧逻辑只数汉字，会把它们当成中文**跳过**，日韩页面整页漏翻。
  // 出现假名/谚文一律视为需要汉化成中文。
  function needsTranslation(s) {
    var han = 0, latin = 0, kana = 0, hangul = 0, i, c;
    for (i = 0; i < s.length; i++) {
      c = s.charCodeAt(i);
      if (c >= 0x4e00 && c <= 0x9fff) han++;
      else if ((c >= 65 && c <= 90) || (c >= 97 && c <= 122)) latin++;
      // 平假名 3040-309F / 片假名 30A0-30FF（含长音符号、浊音记号）
      else if ((c >= 0x3040 && c <= 0x30ff)) kana++;
      // 谚文音节 AC00-D7A3 / 兼容字母 3130-318F / 字母组合 1100-11FF
      else if ((c >= 0xac00 && c <= 0xd7a3) ||
               (c >= 0x3130 && c <= 0x318f) ||
               (c >= 0x1100 && c <= 0x11ff)) hangul++;
    }
    if (kana > 0 || hangul > 0) return true;     // 日文/韩文 → 该翻成中文
    if (han === 0) return true;                  // 一点汉字没有 → 该翻
    if (latin === 0) return false;               // 全汉字 → 不翻
    return han / (han + latin) < 0.15;           // 混排：汉字占太少才翻
  }

  // ---- 可翻性闸门 ----
  function isTranslatable(s) {
    if (!s) return false;
    var t = s.replace(/\s+/g, ' ').trim();
    if (t.length < 2) return false;              // 单字/空白：翻没意义
    if (t.length > 2000) return false;           // 超长块：多半是误抓的大容器
    if (!needsTranslation(t)) return false;
    // 纯数字/符号/日期/邮箱/URL：没有任何可译语义。
    // 可译语义 = 拉丁字母 **或** 假名/谚文（纯汉字已在 needsTranslation 拦掉）。
    if (!/[A-Za-z\u3040-\u30ff\uac00-\ud7a3\u3130-\u318f\u1100-\u11ff]/.test(t)) {
      return false;
    }
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

  // ---- 内联标签掩码（2026-09-27）----
  //
  // 旧实现整段用 textContent 收、写回也用 textContent 贴：译文一上页面，
  // 段落里的 <a>/<b>/<code>/<span> 全被抹平——链接不能点、代码样式丢失。
  // 这里把内联元素序列化成结构标记 ⟦n⟧…⟦/n⟧（空元素/换行是 ⟦n/⟧），
  // 模型只看到带标记的文本并被要求原样保留标记；写回时（page_apply.js）
  // 按 map 把标记重建成真实元素。标记被模型弄坏 → 写回侧整段安全回退原文。
  //
  // 只保留"短语义"内联标签；块级/媒体/控件元素一律钻过去（dive through），
  // 与旧行为保持一致（它们在纯文本替换里本来也留不住，不引入新回归）。
  var KEEP_INLINE = {
    A: 1, B: 1, STRONG: 1, I: 1, EM: 1, CODE: 1, SPAN: 1, U: 1,
    SMALL: 1, SUB: 1, SUP: 1, MARK: 1, ABBR: 1, CITE: 1, Q: 1,
    KBD: 1, SAMP: 1, VAR: 1, TIME: 1, INS: 1, DEL: 1, BDI: 1,
    BDO: 1, DFN: 1
  };
  // 自闭合保留：只有 BR/WBR/IMG。INPUT 故意不加——重建会丢用户运行时已填的值。
  var KEEP_SELF = { BR: 1, WBR: 1, IMG: 1 };
  // 钻过去时也不收录其文字的标签（控件/媒体/脚本里的"文本"不是给人读的句子）
  var DIVE_SILENT = SKIP_TAGS;

  // 把元素内容序列化成 { text: 带标记文本, map: {id:{tag,a:[[名,值]...]}} }。
  // map 为空对象表示这段没有任何需要重建的内联标签，走纯文本路径即可。
  function maskElement(el) {
    var seq = 0;
    var map = {};
    var out = '';
    function record(el2) {
      var attrs = [];
      if (el2.attributes) {
        for (var ai = 0; ai < el2.attributes.length; ai++) {
          var at = el2.attributes[ai];
          attrs.push([at.name, at.value]);
        }
      }
      var id = String(seq++);
      map[id] = { tag: el2.tagName.toLowerCase(), a: attrs };
      return id;
    }
    function ser(node) {
      var kids = node.childNodes;
      for (var i = 0; i < kids.length; i++) {
        var k = kids[i];
        if (k.nodeType === 3) { out += k.nodeValue; continue; }
        if (k.nodeType !== 1) continue;
        var tag = k.tagName;
        // 注意顺序：CODE 等同时出现在 SKIP_TAGS 与 KEEP_INLINE 里——
        // 在"段落内"它是该保留的内联标签，保留判定必须在前。
        if (KEEP_SELF[tag]) {
          out += '⟦' + record(k) + '/⟧';
          continue;
        }
        if (KEEP_INLINE[tag]) {
          // 空的内联元素（典型：图标字体 <span class="icon"></span>）按自闭合
          // 处理：成对空标记 ⟦0⟧⟦/0⟧ 极容易被模型当废字符删掉，导致整段回退。
          if (!(k.textContent || '').trim()) {
            out += '⟦' + record(k) + '/⟧';
            continue;
          }
          var id = record(k);
          out += '⟦' + id + '⟧';
          ser(k);
          out += '⟦/' + id + '⟧';
          continue;
        }
        // 其余技术标签（SCRIPT/STYLE/PRE/INPUT/TEXTAREA/…）：不收其文字
        if (DIVE_SILENT[tag]) continue;
        ser(k); // 非保留容器：钻过去收里面的文字
      }
    }
    ser(el);
    return { text: norm(out), map: map };
  }

  function push(kind, text, node, markers) {
    var t = norm(text);
    var key = kind + ':' + t;
    if (seen[key]) { skipped++; return; }
    seen[key] = 1;
    // 视口内标记：当前屏幕上看得见的先翻（配合前端分批写回），
    // 用户抬眼就看到本屏被汉化，不用干等整页。总耗时不变，但体感差很多。
    var inView = false;
    if (node && node.getBoundingClientRect) {
      try {
        var rc = node.getBoundingClientRect();
        var vh = window.innerHeight || 800;
        inView = rc.height > 0 && rc.bottom > 0 && rc.top < vh;
      } catch (e) {}
    }
    segments.push({ kind: kind, text: t, node: node, inView: inView, markers: markers || null });
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
    var els = (document.body || document.documentElement).querySelectorAll('*');
    var restored = 0;
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      // 元素级原文备份（innerHTML）：还原时连内联标签一起回来，
      // 所以备份的必须是 HTML 而不是纯文本。
      if (el.__sbOrig !== undefined) {
        try { el.innerHTML = el.__sbOrig; restored++; } catch (e) {}
        delete el.__sbOrig;
      }
      delete el.__sbId;         // 只清标记；原文备份的生命周期由"还原"掌管
      delete el.__sbCovered;
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

  // ---- 1. 段落级收集（按块级元素，不按叶子文本节点）----
  //
  // 踩过：原来只收"没有元素子节点的纯文本节点"（父元素有 children 就跳过），
  // 结果正文段落里只要出现一个 <a>/<span>/<b>，整段主体文字全被跳过。RFC 9110
  // 实测 1708 段里 66% 是 <20 字碎片，Abstract 那种整段正文一个字没收到——
  // 用户看到的就是「汉化率很低、很多没翻」。段落才是语义单位，整段收、整段翻。
  //
  // 从外到内扫（文档顺序父在子前）：先收外层整段并给后代打 covered 标记，
  // 避免同一段文字被拆成父子两条重复收。
  var BLOCK_TAGS = {
    P: 1, H1: 1, H2: 1, H3: 1, H4: 1, H5: 1, H6: 1, LI: 1, TD: 1, TH: 1,
    DT: 1, DD: 1, CAPTION: 1, FIGCAPTION: 1, BLOCKQUOTE: 1, SUMMARY: 1,
    LABEL: 1, BUTTON: 1, ADDRESS: 1, LEGEND: 1, OPTION: 1
  };
  var BLOCK_SEL =
    'p,h1,h2,h3,h4,h5,h6,li,td,th,dt,dd,caption,figcaption,blockquote,' +
    'summary,label,button,address,legend,option';
  var blockEls = (document.body || document.documentElement).querySelectorAll(BLOCK_SEL);
  for (var bx = 0; bx < blockEls.length; bx++) {
    var bel = blockEls[bx];
    if (bel.__sbCovered) continue;
    var btext = norm(bel.textContent || '');
    if (!btext) continue;
    if (!isTranslatable(btext)) continue;
    // 掩码内联标签：map 非空时收集的文本带 ⟦n⟧ 标记，写回时据此重建链接/样式。
    var masked = maskElement(bel);
    if (!masked.text) continue;
    var hasMap = false;
    for (var mk in masked.map) { hasMap = true; break; }
    push('b', masked.text, bel, hasMap ? masked.map : null);
    var bdes = bel.querySelectorAll('*');
    for (var bd = 0; bd < bdes.length; bd++) bdes[bd].__sbCovered = 1;
  }

  // 兜底：块级标签之外的裸文本（<div>裸文本</div>、<span>文本</span>、
  // <a>单独链接</a> 等）。只看元素**直接**的文本子节点，元素子节点交给它们
  // 自己（或上面的块级收集）去争取，避免把同一句拆两遍。
  var anyEls = (document.body || document.documentElement).querySelectorAll('*');
  for (var ex = 0; ex < anyEls.length; ex++) {
    var eel = anyEls[ex];
    if (eel.__sbCovered) continue;
    // 块级元素归第一步管：这里再按"直接文本"扫一遍会把整段拆出碎片
    // （<p>See the <a>manual</a> for details.</p> 会多出一条 "See the for details."，
    //  既重复又丢了链接里的词，还白占一个翻译名额）。
    if (BLOCK_TAGS[eel.tagName]) continue;
    if (SKIP_TAGS[eel.tagName] || SKIP_TAGS_ATTR[eel.tagName]) continue;
    var direct = '';
    for (var cn = 0; cn < eel.childNodes.length; cn++) {
      if (eel.childNodes[cn].nodeType === 3) direct += eel.childNodes[cn].nodeValue;
    }
    var dtext = norm(direct);
    if (!dtext) continue;
    if (!isTranslatable(dtext)) { skipped++; continue; }
    push('b', dtext, eel);
    var edes = eel.querySelectorAll('*');
    for (var ed = 0; ed < edes.length; ed++) edes[ed].__sbCovered = 1;
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

  // 排序：**视口内的段排最前**，同组内短段在前。
  // 视口优先是为了配合前端"每批翻完立刻写回"——用户马上看到本屏变中文；
  // 短段优先是因为短段出结果快、先看到效果（原文这里只按长度排，没考虑视口）。
  // 注意：**排序后必须同步刷新节点上的标记**，否则写回时按 id 查节点会张冠李戴。
  var order = [];
  for (var oi = 0; oi < segments.length; oi++) order.push(segments[oi]);
  order.sort(function (x, y) {
    if (!!x.inView !== !!y.inView) return x.inView ? -1 : 1;
    return x.text.length - y.text.length;
  });
  segments = [];
  for (var s = 0; s < order.length; s++) {
    var o = order[s];
    var id = 't' + s;
    // 重新打标记：写回阶段就靠这个 id 找节点
    if (o.node) o.node.__sbId = id;
    // markers 只有存在内联标签时才有；字段恒存在（null 或对象），形状稳定
    segments.push({
      id: id,
      kind: o.kind,
      text: o.text,
      chars: o.text.length,
      markers: o.markers || null
    });
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

// 选中文本采集脚本（编译期嵌入，include_str! 于 api/handlers/content.rs）
//
// 用途：P1「网页随手问 AI」上下文采集（docs/网页随手问AI-全方位规划_2026-09-26_v1.md）
// - **纯只读**：只读 window.getSelection / DOM 文本，不改页面、不派发事件、不聚焦；
// - 固定模板：调用方只能选 expand / max_chars 两个参数，无注入面；
// - 段落扩取：选区碎片扩到所属段落（p/li/td/blockquote/pre/标题等），
//   问"这段说的啥"需要段落语境，碎片不够用；
// - 密码区域拒绝：选区落在密码框/常见密文容器内一律拒绝并说明理由（铁律 1.4）；
// - 只取主文档选区：跨源 iframe 内的 selection 取不到（浏览器同源策略），
//   如实由 has_selection=false 表达，不谎报；
// - 脚本自身不抛异常。

(function () {
  var EXPAND = __EXPAND__ === 1;
  var MAX_CHARS = __MAX_CHARS__;

  function finish(v) { return JSON.stringify(v); }
  function fail(m) { return JSON.stringify({ error: m }); }

  // 原生引用，防页面 monkey-patch
  var _qsa = Document.prototype.querySelectorAll;
  var _getSel = window.getSelection;

  // 兜底：在**读取侧**也注册一次选区缓存监听（幂等）。
  //
  // 起因：用户"选中文字 → 点侧栏按钮"时，点击让页面失焦、浏览器立刻清空选区，
  // 按钮 handler 再来读就啥都没有（真机实测：「翻译选中」永远报"没读到"）。
  // 页面加载后注入 page_selwatch.js 的方案在本机没生效（on_page_load 和
  // WebviewBuilder::initialization_script 都试过），所以在读取侧补注册——
  // 这里和下面读 getSelection 是同一个执行世界，必然捕获得到。
  // 代价：本次调用才装上监听器，所以"第一次点会失败"，此后正常工作。
  (function ensureSelWatch() {
    if (window.__sbSelWatch) return;
    window.__sbSelWatch = 1;
    try {
      document.addEventListener(
        'selectionchange',
        function () {
          try {
            var s = window.getSelection && window.getSelection();
            if (!s || s.rangeCount === 0) return;
            var t = String(s);
            if (!t || !t.trim()) return;
            window.__sbLastRange = s.getRangeAt(0).cloneRange();
            window.__sbLastSelAt = Date.now();
          } catch (e) {
            /* 选区不可克隆就放弃本次缓存 */
          }
        },
        true,
      );
    } catch (e) {
      /* 注册失败不影响只读读取 */
    }
  })();

  // 密码 / 敏感区域：命中即拒绝
  var SECRET_SEL = 'input[type=password],[type=password],' +
    '[autocomplete=current-password],[autocomplete=new-password],' +
    '[data-secret],[data-private],[data-sensitive],' +
    '.password,.password-box,.pwd,.pwd-box,.secret,.secret-box';

  function chars(s) { return (s || '').length; }

  function trimTo(s, n) {
    s = (s || '').trim();
    if (s.length <= n) return { text: s, truncated: false };
    return { text: s.slice(0, n), truncated: true };
  }

  // 归一化：压缩空白，便于后续文本比较与提示词组装
  function compact(s) {
    return (s || '').replace(/\s+/g, ' ').trim();
  }

  function elText(el) {
    if (!el) return '';
    var t = null;
    try { t = el.innerText; } catch (e) { t = null; }
    if (t == null) { try { t = el.textContent; } catch (e) { t = null; } }
    return (t || '').trim();
  }

  // 选区所在元素：anchorNode 可能是文本节点
  function anchorEl(sel) {
    var n = null;
    try { n = sel.anchorNode; } catch (e) { n = null; }
    if (!n) return null;
    if (n.nodeType === 3) return n.parentElement || null;
    if (n.nodeType === 1) return n;
    return null;
  }

  // 段落级容器：优先语义块，其次父元素
  var BLOCK_SEL = 'p,li,td,th,blockquote,pre,figcaption,dd,dt,summary,' +
    'h1,h2,h3,h4,h5,h6,article,section,main';

  function blockOf(el) {
    if (!el) return null;
    var b = null;
    try { b = el.closest ? el.closest(BLOCK_SEL) : null; } catch (e) { b = null; }
    return b || el;
  }

  function inSecret(el) {
    if (!el) return false;
    try {
      if (el.matches && el.matches(SECRET_SEL)) return true;
      var hit = _qsa.call(document, SECRET_SEL);
      for (var i = 0; i < hit.length; i++) {
        if (hit[i] === el || (hit[i].contains && hit[i].contains(el))) return true;
      }
    } catch (e) { /* 匹配异常按未命中处理，不误拒 */ }
    return false;
  }

  try {
    var sel = null;
    try { sel = _getSel.call(window); } catch (e) { sel = null; }

    var base = {
      has_selection: false,
      text: '',
      text_chars: 0,
      block_text: '',
      block_chars: 0,
      tag: '',
      truncated: false,
      expanded: EXPAND,
      rejected: false,
      reject_reason: '',
      url: '',
      title: '',
      in_iframe: false,
      // 休眠标签无 webview，Rust 侧另行回填 true 并附 hints（见 content.rs::selection）。
      // 此处置 false，保证"未休眠"时该字段恒存在，调用方形状稳定。
      hibernated: false,
      // 诊断用：选区缓存是否已挂上（排查"选中就丢"时看这个）
      selwatch: !!window.__sbSelWatch
    };

    try { base.url = location.href || ''; } catch (e) { base.url = ''; }
    try { base.title = document.title || ''; } catch (e) { base.title = ''; }

    // 选区被清空时回退到缓存：最典型的是"用户选中 → 点侧栏按钮"，
    // 点击让页面失焦、浏览器立刻清空选区，等 handler 来读就啥都没有了
    // （真机实测：永远报"没读到选中内容"）。页面侧常驻监听器
    // （page_selwatch.js）在选中那一刻克隆过 Range，这里恢复它。
    // 恢复的是用户自己的选区，视觉上会重新高亮——正是用户期望的"我选的还在"。
    if ((!sel || sel.isCollapsed || sel.rangeCount === 0) && window.__sbLastRange) {
      try {
        var kept = window.__sbLastRange;
        if (kept && !kept.collapsed) {
          var gs = _getSel.call(window);
          gs.removeAllRanges();
          gs.addRange(kept);
          sel = gs;
        }
      } catch (e) {
        /* Range 已失效（页面重渲染）：当作没有选区，如实返回 */
      }
    }

    // 无选区 / 折叠选区：如实返回"没有"，不报错（调用方常态路径）
    if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
      return finish(base);
    }

    var raw = '';
    try { raw = sel.toString() || ''; } catch (e) { raw = ''; }
    if (!raw.trim()) {
      return finish(base);
    }

    var el = anchorEl(sel);
    base.tag = el && el.tagName ? el.tagName.toLowerCase() : '';

    // 密码区域：拒绝（不返回任何内容）
    if (inSecret(el) || inSecret(blockOf(el))) {
      base.rejected = true;
      base.reject_reason = '选区位于密码或敏感输入区域，已拒绝读取（铁律 1.4）';
      return finish(base);
    }

    base.has_selection = true;
    var t = trimTo(raw, MAX_CHARS);
    base.text = t.text;
    base.text_chars = chars(t.text);
    base.truncated = t.truncated;

    // 段落扩取：给"这段说的啥"提供语境
    if (EXPAND) {
      var blk = blockOf(el);
      var bt = elText(blk);
      if (bt && bt.length > base.text_chars) {
        var b = trimTo(compact(bt), MAX_CHARS);
        base.block_text = b.text;
        base.block_chars = chars(b.text);
        if (b.truncated) base.truncated = true;
        if (blk && blk.tagName) base.tag = blk.tagName.toLowerCase();
      }
    }

    return finish(base);
  } catch (e) {
    return fail(e && e.message ? e.message : '选区采集异常');
  }
})();

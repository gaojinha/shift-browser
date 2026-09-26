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
      hibernated: false
    };

    try { base.url = location.href || ''; } catch (e) { base.url = ''; }
    try { base.title = document.title || ''; } catch (e) { base.title = ''; }

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

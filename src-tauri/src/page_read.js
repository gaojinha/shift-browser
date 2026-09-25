// read_page 固定采集脚本（编译期嵌入，include_str! 于 api.rs）
//
// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md
// - 脚本固定，调用方只能选 mode + selector + 截断上限（不能注入任意 JS）；
// - 占位符由 Rust 侧 replace 注入：__MODE__ / __SELECTOR__ / __MAX_CHARS__ / __MAX_ITEMS__
//   （__SELECTOR__ 经 js_escape 转义为 JS 字符串字面量内容；两个 MAX 为裸数字）；
// - 8 种模式：text / elements / links / images / tables / forms / article / html；
// - 脱敏：password / hidden 的 value 一律不返回；非 http(s) 图片 URL 只留
//   scheme+长度；html 模式剥离 script/style 内容、注释、password/hidden 的 value 属性；
// - 脚本自身不抛异常：任何错误返回 JSON.stringify({ error: ... })，由 Rust 侧转 ok:false。

(function () {
  var MODE = '__MODE__';
  var SELECTOR = '__SELECTOR__';
  var MAX_CHARS = __MAX_CHARS__;
  var MAX_ITEMS = __MAX_ITEMS__;
  // P1b 开关（Rust 侧注入 0/1）
  var EXPAND = __EXPAND__ === 1;
  var SHADOW = __SHADOW__ === 1;
  var IFRAME = __IFRAME__ === 1;

  function finish(v) { return JSON.stringify(v); }
  function fail(m) { return JSON.stringify({ error: m }); }

  // F4：保存原生 DOM API 引用，防页面 monkey-patch 干扰（页面可改写原型方法）
  var _docQs = Document.prototype.querySelector;
  var _docQsa = Document.prototype.querySelectorAll;
  var _elQs = Element.prototype.querySelector;
  var _elQsa = Element.prototype.querySelectorAll;
  var _gcs = window.getComputedStyle;
  function qs(rootEl, sel) {
    try { return (rootEl.nodeType === 9 ? _docQs : _elQs).call(rootEl, sel); }
    catch (e) { return null; }
  }
  function qsa(rootEl, sel) {
    return (rootEl.nodeType === 9 ? _docQsa : _elQsa).call(rootEl, sel);
  }

  var doc = document;
  var base = { mode: MODE, url: location.href || '', title: doc.title || '' };
  base.ts = Date.now();

  // 非 http(s) URL（data: / blob: 等）只留 scheme + 字节长度，不返回主体
  function safeUrl(u) {
    if (!u) return '';
    if (/^https?:/i.test(u)) return u;
    var m = u.match(/^([a-z][a-z0-9+.-]*):/i);
    return (m ? m[1] : 'data') + ':' + u.length;
  }

  // C6：页面元信息（title / description / og / lang）
  function collectMeta() {
    function content(sel) {
      var e = qs(doc, sel);
      return e ? (e.getAttribute('content') || '') : '';
    }
    return {
      title: doc.title || '',
      description: content('meta[name="description"]'),
      lang: doc.documentElement ? (doc.documentElement.getAttribute('lang') || '') : '',
      og: {
        title: content('meta[property="og:title"]'),
        description: content('meta[property="og:description"]'),
        image: safeUrl(content('meta[property="og:image"]')),
        url: content('meta[property="og:url"]'),
        site_name: content('meta[property="og:site_name"]'),
        type: content('meta[property="og:type"]'),
      },
    };
  }
  base.meta = collectMeta();

  var root = doc.body || doc.documentElement;
  if (!root) return fail('页面无 body');
  if (SELECTOR) {
    try {
      var picked = qs(root, SELECTOR);
      if (picked) root = picked;
    } catch (e) {
      return fail('选择器非法: ' + e.message);
    }
  }

  // B6/B9/B10：读不全的提示（canvas/svg 图形 / 登录框 / 验证码），供调用方决定是否截图或交互
  function collectHints(rootEl) {
    var hints = [];
    try {
      if (qsa(rootEl, 'canvas, svg').length > 0) {
        hints.push('页面含 canvas/svg 图形，文本可能读不全，建议截图');
      }
    } catch (e) { /* 忽略 */ }
    try {
      if (qsa(rootEl, 'input[type="password"]').length > 0) {
        hints.push('页面含密码输入框，可能需登录');
      }
    } catch (e) { /* 忽略 */ }
    try {
      if (qsa(rootEl, '[class*="captcha" i], [id*="captcha" i], iframe[src*="captcha" i]').length > 0) {
        hints.push('页面可能含验证码 / 人机验证');
      }
    } catch (e) { /* 忽略 */ }
    return hints;
  }
  base.hints = collectHints(root);

  // B7：图片真实地址（懒加载占位 src 优先让位于 currentSrc / data-src / data-original）
  function imgUrl(im) {
    return safeUrl(
      im.currentSrc ||
      im.getAttribute('data-src') ||
      im.getAttribute('data-original') ||
      im.src ||
      ''
    );
  }

  function visible(el) {
    try {
      var cs = _gcs.call(window, el);
      return cs.display !== 'none' && cs.visibility !== 'hidden';
    } catch (e) { return true; }
  }

  function rawText(el) {
    var t = el.innerText;
    if (t == null) t = el.textContent;
    return (t || '').trim();
  }

  function clip(s, n) {
    s = (s || '').trim();
    return s.length > n ? s.slice(0, n) : s;
  }

  // 压缩空白：行内多空白并 1，行尾空白去，连续 3+ 换行并 2
  function compact(s) {
    return (s || '')
      .replace(/[ \t]+/g, ' ')
      .replace(/\n[ \t]+/g, '\n')
      .replace(/\n{3,}/g, '\n\n')
      .trim();
  }

  // B3：展开原生 <details>（只动原生可折叠，不点按钮，零副作用）
  function expandDetails(rootEl) {
    try {
      var ds = qsa(rootEl, 'details');
      for (var i = 0; i < ds.length; i++) {
        if (!ds[i].open) ds[i].open = true;
      }
    } catch (e) { /* 忽略 */ }
  }

  // B5：尽力穿透开放 shadow root（closed shadow 读不到，按提示处理）
  function shadowText(rootEl) {
    var out = [];
    try {
      var all = qsa(rootEl, '*');
      var cap = Math.min(all.length, 5000);
      var budget = 0;
      for (var i = 0; i < cap && budget < MAX_CHARS; i++) {
        var sr = all[i] && all[i].shadowRoot;
        if (!sr) continue;
        var t = compact(sr.textContent || '');
        if (t) {
          out.push(t);
          budget += t.length;
        }
      }
    } catch (e) { /* 忽略 */ }
    return out.join('\n');
  }

  // B4：同源 iframe 文本并入；跨源记数（读不到，给提示）
  function iframeText(rootEl) {
    var out = [];
    var cross = 0;
    try {
      var fs = qsa(rootEl, 'iframe');
      var cap = Math.min(fs.length, 20);
      for (var i = 0; i < cap; i++) {
        try {
          var d = fs[i].contentDocument;
          if (!d || !d.body) { cross++; continue; }
          var b = d.body;
          var t = compact(b.innerText != null ? b.innerText : (b.textContent || ''));
          if (t) out.push(t);
        } catch (e) {
          cross++; // 跨源访问抛异常
        }
      }
    } catch (e) { /* 忽略 */ }
    return { text: out.join('\n'), cross: cross };
  }

  // 列表类模式公共收尾
  function finishList(key, items, total) {
    base[key] = items;
    base.total = total;
    base.returned = items.length;
    base.truncated = total > items.length;
    return finish(base);
  }

  if (MODE === 'text') {
    if (EXPAND) expandDetails(root);
    var parts = [rawText(root)];
    if (SHADOW) {
      var st = shadowText(root);
      if (st) parts.push(st);
    }
    if (IFRAME) {
      var ifr = iframeText(root);
      if (ifr.text) parts.push(ifr.text);
      if (ifr.cross > 0) {
        base.hints.push('存在 ' + ifr.cross + ' 个跨源 iframe，内容无法读取');
      }
    }
    var joined = parts.join('\n');
    var t = compact(joined);
    var tTrunc = t.length > MAX_CHARS;
    if (tTrunc) t = t.slice(0, MAX_CHARS);
    base.text = t;
    base.total = joined.length;
    base.returned = t.length;
    base.truncated = tTrunc;
    base.empty = t.length === 0;
    return finish(base);
  }

  if (MODE === 'elements') {
    var sel = 'a, img, button, input, select, textarea, label, h1, h2, h3, h4, h5, h6, table, form, ' +
              '[role="button"], [role="link"], [role="tab"], [role="menuitem"]';
    var els = qsa(root, sel);
    var eItems = [];
    for (var i = 0; i < els.length && eItems.length < MAX_ITEMS; i++) {
      var el = els[i];
      if (!visible(el)) continue;
      var it = { tag: el.tagName.toLowerCase() };
      var role = el.getAttribute('role');
      if (role) it.role = role;
      var txt = clip(rawText(el), 200);
      if (txt) it.text = txt;
      if (el.tagName === 'A') {
        it.href = el.href || '';
      } else if (el.tagName === 'IMG') {
        it.src = imgUrl(el);
        it.alt = el.alt || '';
      } else if (el.tagName === 'INPUT' || el.tagName === 'SELECT' || el.tagName === 'TEXTAREA') {
        it.type = el.tagName === 'INPUT'
          ? (el.type || 'text')
          : (el.tagName === 'SELECT' ? 'select' : 'textarea');
        if (el.name) it.name = el.name;
        if (el.id) it.id = el.id;
        // 脱敏：password / hidden 的 value 不返回
        var inpType = (el.type || '').toLowerCase();
        if (inpType !== 'password' && inpType !== 'hidden') {
          var v = el.value;
          if (v != null && String(v) !== '') it.value = clip(String(v), 100);
        }
      }
      eItems.push(it);
    }
    return finishList('elements', eItems, els.length);
  }

  if (MODE === 'links') {
    var as = qsa(root, 'a[href]');
    var lItems = [];
    for (var i = 0; i < as.length && lItems.length < MAX_ITEMS; i++) {
      var a = as[i];
      if (!visible(a)) continue;
      lItems.push({ text: clip(rawText(a), 200), href: a.href || '' });
    }
    return finishList('links', lItems, as.length);
  }

  if (MODE === 'images') {
    var imgs = qsa(root, 'img');
    var iItems = [];
    for (var i = 0; i < imgs.length && iItems.length < MAX_ITEMS; i++) {
      var im = imgs[i];
      if (!visible(im)) continue;
      iItems.push({
        src: imgUrl(im),
        alt: im.alt || '',
        w: im.naturalWidth || 0,
        h: im.naturalHeight || 0,
      });
    }
    return finishList('images', iItems, imgs.length);
  }

  if (MODE === 'tables') {
    var tables = qsa(root, 'table');
    var tItems = [];
    var anyMerged = false; // C1：检测 rowspan/colspan（合并单元格会打乱行列对应）
    // 表格本身限 50 张（单表行数再限 100），max_items 取两者较小
    var tableCap = Math.min(MAX_ITEMS, 50);
    for (var ti = 0; ti < tables.length && tItems.length < tableCap; ti++) {
      var tb = tables[ti];
      if (!visible(tb)) continue;
      try {
        if (qsa(tb, '[rowspan], [colspan]').length > 0) anyMerged = true;
      } catch (e) { /* 忽略 */ }
      var rows = [];
      var trs = qsa(tb, 'tr');
      var rowCap = Math.min(trs.length, 100);
      for (var r = 0; r < rowCap; r++) {
        var cells = [];
        var tds = qsa(trs[r], 'th, td');
        for (var c = 0; c < tds.length; c++) {
          cells.push(clip(rawText(tds[c]), 200));
        }
        rows.push(cells);
      }
      tItems.push(rows);
    }
    base.merged = anyMerged;
    return finishList('tables', tItems, tables.length);
  }

  if (MODE === 'forms') {
    var fs = qsa(root, 'input, select, textarea');
    var fItems = [];
    for (var fi = 0; fi < fs.length && fItems.length < MAX_ITEMS; fi++) {
      var el = fs[fi];
      if (!visible(el)) continue;
      // 脱敏：表单控件一律不返回 value（防密码 / CSRF token 泄露）
      var item = {
        type: el.tagName === 'INPUT'
          ? (el.type || 'text')
          : (el.tagName === 'SELECT' ? 'select' : 'textarea'),
      };
      if (el.name) item.name = el.name;
      if (el.id) item.id = el.id;
      if (el.placeholder) item.placeholder = clip(el.placeholder, 100);
      var label = '';
      if (el.id) {
        try {
          var l = qs(doc, 'label[for="' + el.id.replace(/"/g, '') + '"]');
          if (l) label = clip(rawText(l), 100);
        } catch (e) { /* 忽略 */ }
      }
      if (!label) {
        try {
          var cl = el.closest('label');
          if (cl) label = clip(rawText(cl), 100);
        } catch (e) { /* 忽略 */ }
      }
      if (label) item.label = label;
      fItems.push(item);
    }
    return finishList('forms', fItems, fs.length);
  }

  if (MODE === 'article') {
    // 只回传 HTML，readability 解析在 Rust 侧 spawn_blocking 执行（CPU 密集）
    var aHtml = root.outerHTML;
    if (aHtml.length > 4000000) {
      return fail('页面过大（outerHTML>4MB），请缩小 selector 范围');
    }
    base.html = aHtml;
    return finish(base);
  }

  if (MODE === 'html') {
    var html = root.outerHTML;
    var hTotal = html.length;
    // 脱敏：注释 / script、style 内容 / password、hidden 的 value 属性
    html = html.replace(/<!--[\s\S]*?-->/g, '');
    html = html.replace(/<script\b[\s\S]*?<\/script>/gi, '<script></script>');
    html = html.replace(/<style\b[\s\S]*?<\/style>/gi, '<style></style>');
    html = html.replace(/<input\b[^>]*>/gi, function (tag) {
      if (/type\s*=\s*["']?(password|hidden)/i.test(tag)) {
        tag = tag.replace(/\s+value\s*=\s*("[^"]*"|'[^']*'|[^\s>]+)/gi, '');
      }
      return tag;
    });
    var hTrunc = html.length > MAX_CHARS;
    if (hTrunc) html = html.slice(0, MAX_CHARS);
    base.html = html;
    base.total = hTotal;
    base.returned = html.length;
    base.truncated = hTrunc;
    return finish(base);
  }

  return fail('未知模式: ' + MODE);
})();

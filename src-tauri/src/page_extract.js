// extract 固定提取脚本（编译期嵌入，include_str! 于 api/handlers/content.rs）
//
// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.2 extract）
// - 固定模板：调用方只能选 selector + fields 白名单 + limit，不能注入任意 JS；
// - fields 白名单：text / href / src / alt / title / data-*（Rust 侧已校验）；
// - href/src 脱敏（非 http(s) 只留 scheme+长度）；脚本自身不抛异常。

(function () {
  var SELECTOR = '__SELECTOR__';
  var FIELDS = __FIELDS__;
  var LIMIT = __LIMIT__;

  function finish(v) { return JSON.stringify(v); }
  function fail(m) { return JSON.stringify({ error: m }); }

  // F4：原生引用，防页面 monkey-patch
  var _docQsa = Document.prototype.querySelectorAll;

  function safeUrl(u) {
    if (!u) return '';
    if (/^https?:/i.test(u)) return u;
    var m = u.match(/^([a-z][a-z0-9+.-]*):/i);
    return (m ? m[1] : 'data') + ':' + u.length;
  }

  function rawText(el) {
    if (!el) return '';
    var t = el.innerText;
    if (t == null) t = el.textContent;
    return (t || '').trim().slice(0, 500);
  }

  try {
    var els = _docQsa.call(document, SELECTOR);
    var items = [];
    var n = Math.min(els.length, LIMIT);
    for (var i = 0; i < n; i++) {
      var el = els[i];
      var it = {};
      for (var f = 0; f < FIELDS.length; f++) {
        var field = FIELDS[f];
        var v = '';
        if (field === 'text') v = rawText(el);
        else if (field === 'href') v = safeUrl(el.href || el.getAttribute('href') || '');
        else if (field === 'src') {
          v = safeUrl(el.currentSrc || el.getAttribute('src') || el.getAttribute('data-src') || '');
        } else if (field === 'alt') v = el.getAttribute('alt') || '';
        else if (field === 'title') v = el.getAttribute('title') || '';
        else if (field.indexOf('data-') === 0) v = el.getAttribute(field) || '';
        it[field] = v;
      }
      items.push(it);
    }
    return finish({
      selector: SELECTOR,
      total: els.length,
      returned: items.length,
      truncated: els.length > items.length,
      items: items
    });
  } catch (e) {
    return fail('提取失败: ' + e.message);
  }
})();

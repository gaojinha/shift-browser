// ShiftBrowser 右键翻译菜单（注入到所有 http/https 页面）
//
// 背景：翻译引擎（translate.rs）+ 整页翻译脚本（whole_page_translate.js）
// + shift://__translate__ 协议端点全都齐了，但一直**没有 UI 入口**。
// 本脚本补上入口：
//   - 选中文字右键 → 弹出菜单「翻译选中文字 / 翻译整个页面 / 复制」
//   - 未选中文字右键 → 放行系统原生菜单（不干扰复制粘贴等日常操作）
//   - 整页翻译：调 /whole-page，由 Rust 注入 whole_page_translate.js 回填 DOM
//   - 划词翻译：调 /text，结果在选中文字附近的浮层里展示
//
// 与后端约定（见 src-tauri/src/lib.rs 的 shift://__translate__ 处理器）：
//   请求：shift://__translate__/{text|whole-page}?callback=NAME[&text=...]
//   回写：后端 eval `window.NAME = {text} | {error}`，前端轮询取值
//   触发：Image().src（shift handler 同步返回 1x1 gif，不阻塞页面）
(function () {
  if (window.__shiftTranslateMenu) return;
  window.__shiftTranslateMenu = true;
  if (location.protocol !== 'http:' && location.protocol !== 'https:') return;

  var seq = 0;

  /** 调后端翻译，轮询回调变量拿结果（等待时长 = 设置里的翻译超时秒数 × 10 次轮询，默认 15 秒） */
  function callTranslate(path, params, cb) {
    var name = '__sb_tr_' + Date.now() + '_' + (++seq);
    window[name] = null;
    var q = 'callback=' + name;
    for (var k in params) {
      if (Object.prototype.hasOwnProperty.call(params, k)) {
        q += '&' + k + '=' + encodeURIComponent(params[k]);
      }
    }
    try {
      var img = new Image();
      img.src = 'shift://__translate__/' + path + '?' + q;
    } catch (e) {
      cb({ error: '无法发起翻译请求: ' + e });
      return;
    }
    var T = (typeof window.__SHIFT_TRANSLATE_TIMEOUT === 'number' && window.__SHIFT_TRANSLATE_TIMEOUT >= 5)
      ? window.__SHIFT_TRANSLATE_TIMEOUT : 15;
    var tries = 0;
    var timer = setInterval(function () {
      var r = window[name];
      if (r || ++tries > T * 10) {
        clearInterval(timer);
        try { delete window[name]; } catch (e) { window[name] = null; }
        cb(r || { error: '翻译超时' });
      }
    }, 100);
  }

  // ---------- 样式（一次性注入，用 Shadow DOM 隔离，避免污染页面） ----------
  function ensureHost() {
    var host = document.getElementById('__sb_tr_host');
    if (host) return host.shadowRoot;
    host = document.createElement('div');
    host.id = '__sb_tr_host';
    host.style.cssText = 'all:initial;position:fixed;z-index:2147483647;left:0;top:0;';
    var shadow = host.attachShadow({ mode: 'open' });
    var style = document.createElement('style');
    style.textContent = [
      '.menu{position:fixed;min-width:170px;background:#2b2b31;color:#eee;border:1px solid #45454d;',
      'border-radius:8px;box-shadow:0 6px 24px rgba(0,0,0,.45);padding:4px;font:13px/1.5 -apple-system,',
      '"PingFang SC","Microsoft YaHei",sans-serif;}',
      '.item{padding:7px 10px;border-radius:5px;cursor:pointer;white-space:nowrap;}',
      '.item:hover{background:#3d3d46;}',
      '.item.dim{opacity:.45;cursor:default;}',
      '.sep{height:1px;background:#45454d;margin:4px 6px;}',
      '.tip{position:fixed;max-width:420px;background:#2b2b31;color:#eee;border:1px solid #45454d;',
      'border-radius:8px;box-shadow:0 6px 24px rgba(0,0,0,.45);padding:10px 12px;',
      'font:13px/1.6 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif;}',
      '.tip .src{color:#9a9aa5;font-size:12px;margin-bottom:6px;max-height:72px;overflow:auto;}',
      '.tip .dst{color:#fff;white-space:pre-wrap;word-break:break-word;}',
      '.tip .err{color:#ff8a80;}',
      '.tip .bar{margin-top:8px;display:flex;gap:8px;}',
      '.tip button{background:#3d3d46;color:#eee;border:none;border-radius:5px;padding:4px 10px;',
      'cursor:pointer;font-size:12px;}',
      '.tip button:hover{background:#4a4a55;}',
    ].join('');
    shadow.appendChild(style);
    (document.body || document.documentElement).appendChild(host);
    return shadow;
  }

  var menuEl = null;
  function hideMenu() {
    if (menuEl && menuEl.parentNode) menuEl.parentNode.removeChild(menuEl);
    menuEl = null;
  }

  function showMenu(shadow, x, y, selectedText) {
    hideMenu();
    var menu = document.createElement('div');
    menu.className = 'menu';
    menu.style.left = Math.min(x, window.innerWidth - 190) + 'px';
    menu.style.top = Math.min(y, window.innerHeight - 200) + 'px';

    function addItem(label, fn, dim) {
      var it = document.createElement('div');
      it.className = 'item' + (dim ? ' dim' : '');
      it.textContent = label;
      if (!dim) {
        it.addEventListener('click', function (ev) {
          ev.stopPropagation();
          hideMenu();
          fn();
        });
      }
      menu.appendChild(it);
    }
    function addSep() {
      var s = document.createElement('div');
      s.className = 'sep';
      menu.appendChild(s);
    }

    // 翻译入口：整页翻译始终可用；选中翻译仅在选中文字时可点
    addItem('🌐 翻译选中文字', function () {
      translateSelection(selectedText, x, y);
    }, !selectedText);
    addItem('📄 翻译整个页面', function () {
      callTranslate('whole-page', {}, function (r) {
        if (r && r.error) showTip(shadow, x, y, '', r.error, true);
      });
    });
    if (selectedText) {
      addSep();
      addItem('📋 复制', function () {
        try { navigator.clipboard.writeText(selectedText); } catch (e) {}
      });
    }
    addSep();
    // 补齐系统原生菜单的常用项（自定义菜单会覆盖原生菜单，不能丢这些功能）
    addItem('🔄 刷新页面', function () {
      try { location.reload(); } catch (e) {}
    });
    addItem('◀ 后退', function () {
      try { history.back(); } catch (e) {}
    }, history.length <= 1);
    addItem('▶ 前进', function () {
      try { history.forward(); } catch (e) {}
    }, history.length <= 1);

    shadow.appendChild(menu);
    menuEl = menu;
    // 点击别处关菜单
    setTimeout(function () {
      document.addEventListener('mousedown', onDocDown, true);
    }, 0);
  }

  function onDocDown(ev) {
    if (!menuEl) return;
    var host = document.getElementById('__sb_tr_host');
    if (host && ev.composedPath && ev.composedPath().indexOf(host) !== -1) return;
    hideMenu();
    document.removeEventListener('mousedown', onDocDown, true);
  }

  /** 划词翻译：结果浮层展示 */
  function translateSelection(text, x, y) {
    var shadow = ensureHost();
    showTip(shadow, x, y, text, '翻译中…', false);
    callTranslate('text', { text: text }, function (r) {
      if (!r) return showTip(shadow, x, y, text, '翻译失败：无响应', true);
      if (r.error) return showTip(shadow, x, y, text, r.error, true);
      showTip(shadow, x, y, text, r.text || '', false);
    });
  }

  var tipEl = null;
  function showTip(shadow, x, y, srcText, dstText, isErr) {
    if (tipEl && tipEl.parentNode) tipEl.parentNode.removeChild(tipEl);
    var tip = document.createElement('div');
    tip.className = 'tip';
    tip.style.left = Math.min(x, Math.max(10, window.innerWidth - 440)) + 'px';
    tip.style.top = Math.min(y + 6, Math.max(10, window.innerHeight - 200)) + 'px';
    if (srcText) {
      var s = document.createElement('div');
      s.className = 'src';
      s.textContent = srcText.length > 160 ? srcText.slice(0, 160) + '…' : srcText;
      tip.appendChild(s);
    }
    var d = document.createElement('div');
    d.className = 'dst' + (isErr ? ' err' : '');
    d.textContent = dstText;
    tip.appendChild(d);
    var bar = document.createElement('div');
    bar.className = 'bar';
    if (!isErr && dstText && dstText !== '翻译中…') {
      var copyBtn = document.createElement('button');
      copyBtn.textContent = '复制译文';
      copyBtn.addEventListener('click', function () {
        try { navigator.clipboard.writeText(dstText); } catch (e) {}
      });
      bar.appendChild(copyBtn);
    }
    var closeBtn = document.createElement('button');
    closeBtn.textContent = '关闭';
    closeBtn.addEventListener('click', function () {
      if (tip.parentNode) tip.parentNode.removeChild(tip);
      tipEl = null;
    });
    bar.appendChild(closeBtn);
    tip.appendChild(bar);
    shadow.appendChild(tip);
    tipEl = tip;
  }

  // ---------- 右键拦截 ----------
  // 无论是否选中文字都弹自定义菜单：保证"翻译整个页面"入口永远可用。
  // 自定义菜单里补齐了复制/刷新/后退/前进等原生常用项，不丢失功能。
  document.addEventListener(
    'contextmenu',
    function (ev) {
      var sel = String(window.getSelection ? window.getSelection().toString() : '').trim();
      ev.preventDefault();
      ev.stopPropagation();
      var shadow = ensureHost();
      showMenu(shadow, ev.clientX, ev.clientY, sel.length > 5000 ? sel.slice(0, 5000) : sel);
    },
    true,
  );

  // 滚动/ESC 时收菜单
  window.addEventListener('scroll', hideMenu, true);
  document.addEventListener('keydown', function (ev) {
    if (ev.key === 'Escape') hideMenu();
  });
})();

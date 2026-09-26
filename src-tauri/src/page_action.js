// action/* 固定交互脚本（编译期嵌入，include_str! 于 api/handlers/actions.rs）
//
// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.4 P2 交互）
// - 固定模板：调用方只能选 action + selector + 少量参数，不能注入任意 JS；
// - 交互预检（I2）：先判 not_found / not_visible / disabled；
// - fill 只落字符串（不执行）；click 每次现查 DOM（I10）；
// - 脚本自身不抛异常：错误返回 { error, action_error }。

(function () {
  var ACTION = '__ACTION__';
  var SELECTOR = '__SELECTOR__';
  var INDEX = __INDEX__;
  var VALUE = '__VALUE__';
  var DIRECTION = '__DIRECTION__';
  var AMOUNT = __AMOUNT__;
  var CONDITION = '__CONDITION__';
  var KEY = '__KEY__';
  // 扩展参数（缺省一律"关闭/空"，旧调用行为不变）
  var PIERCE = __PIERCE__;         // 0/1：递归穿透开放 shadow root
  var FRAMES = __FRAMES__;         // JSON 数组：同源 iframe 逐层下钻路径（数字=第 N 个，字符串=选择器）
  var MODIFIERS = __MODIFIERS__;   // JSON 数组：Control/Shift/Alt/Meta
  var TEXT = '__TEXT__';           // find 的文本瞄准 / type 的输入内容
  var ROLE = '__ROLE__';           // find 的无障碍角色
  var NAME = '__NAME__';           // find 的无障碍名称
  var NTH = __NTH__;               // find 取第几个匹配（默认 0）
  var EXACT = __EXACT__;           // 0/1：find 精确匹配（缺省包含）
  var WITHIN = '__WITHIN__';       // find 限定容器选择器
  var VISIBLE_ONLY = __VISIBLE_ONLY__; // 0/1：find 只保留可见元素（缺省开）
  var EXPECT = __EXPECT__;         // JSON 对象：wait_for text 条件的 {op,value}
  var QUIET_MS = __QUIET_MS__;     // wait_for idle 的静默毫秒
  var CLEAR = __CLEAR__;           // 0/1：type 前清空

  var _docQsa = Document.prototype.querySelectorAll;
  var _elQsa = Element.prototype.querySelectorAll;
  var _dfQsa = DocumentFragment.prototype.querySelectorAll;

  function finish(v) { return JSON.stringify(v); }
  function actFail(code, msg, extra) {
    var v = { error: msg || code, action_error: code };
    if (extra) { for (var k in extra) v[k] = extra[k]; }
    return JSON.stringify(v);
  }

  // iframe 内元素样式必须在其自身窗口取（主文档 window.getComputedStyle 会取错）
  function visible(el) {
    try {
      var win = (el.ownerDocument && el.ownerDocument.defaultView) || window;
      var cs = win.getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      var r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    } catch (e) { return true; }
  }

  // 按节点类型取对应原型的 querySelectorAll（防页面 monkey-patch）
  function qsa1(root, sel) {
    if (root.nodeType === 9) return _docQsa.call(root, sel);
    if (root.nodeType === 11) return _dfQsa.call(root, sel);
    return _elQsa.call(root, sel);
  }

  // qsaDeep：在 root（Document/Element/ShadowRoot）内查 sel；pierce 时递归开放 shadow root。
  // pierce=false 时与原生 querySelectorAll 完全等价（顺序一致）。
  function qsaDeep(root, sel, pierce) {
    var out = [];
    var direct;
    try { direct = qsa1(root, sel); } catch (e) { return null; }
    for (var i = 0; i < direct.length; i++) out.push(direct[i]);
    if (pierce) {
      var all;
      try { all = qsa1(root, '*'); } catch (e) { return out; }
      for (var j = 0; j < all.length; j++) {
        var sr = all[j].shadowRoot;
        if (sr) {
          var sub = qsaDeep(sr, sel, true);
          if (sub) for (var k = 0; k < sub.length; k++) out.push(sub[k]);
        }
      }
    }
    return out;
  }

  // frames 数组逐层下钻（同源 contentDocument）；跨源显式报 cross，不谎报
  function descendFrames() {
    var doc = document;
    for (var i = 0; i < FRAMES.length; i++) {
      var step = FRAMES[i];
      var frameEl = null;
      if (typeof step === 'number') {
        var fl = qsa1(doc, 'iframe,frame');
        frameEl = fl[step] || null;
      } else if (typeof step === 'string') {
        var m;
        try { m = qsa1(doc, step); } catch (e) { return { err: 'bad_selector', msg: 'frames 第 ' + i + ' 层选择器非法: ' + e.message }; }
        for (var k = 0; k < m.length; k++) {
          var tn = m[k].tagName;
          if (tn === 'IFRAME' || tn === 'FRAME') { frameEl = m[k]; break; }
        }
      }
      if (!frameEl) return { err: 'not_found', msg: 'frames 路径第 ' + i + ' 层未命中 iframe' };
      var cd = null;
      try { cd = frameEl.contentDocument; } catch (e) { cd = null; }
      if (!cd) return { err: 'cross', index: i };
      doc = cd;
    }
    return { doc: doc, depth: FRAMES.length };
  }

  // 统一定位入口：先下钻 frames，再按 pierce 查 selector。
  // 返回 { els, doc } 或 { bad_selector: 消息 } 或 { cross, index } 或 { frame_gap, code, msg }
  function locate() {
    var base = descendFrames();
    if (base.err === 'cross') return { cross: true, index: base.index };
    if (base.err) return { frame_gap: true, code: base.err, msg: base.msg };
    if (PIERCE !== 1) {
      // 与原行为逐字节一致：直接 qsa，非法选择器带原生错误消息
      var els;
      try { els = qsa1(base.doc, SELECTOR); } catch (e) { return { bad_selector: e.message }; }
      var arr = [];
      for (var i = 0; i < els.length; i++) arr.push(els[i]);
      return { els: arr, doc: base.doc };
    }
    var deep = qsaDeep(base.doc, SELECTOR, true);
    if (!deep) {
      try { qsa1(base.doc, SELECTOR); } catch (e2) { return { bad_selector: e2.message }; }
      return { bad_selector: '选择器非法' };
    }
    return { els: deep, doc: base.doc };
  }

  function normText(s) {
    return (s || '').replace(/\s+/g, ' ').replace(/^\s+|\s+$/g, '');
  }

  // 无障碍角色：显式 role 优先，其次常见隐式映射
  var IMPLICIT_ROLES = {
    BUTTON: 'button', TEXTAREA: 'textbox', SELECT: 'combobox', IMG: 'img',
    NAV: 'navigation', MAIN: 'main', ASIDE: 'complementary', HEADER: 'banner',
    FOOTER: 'contentinfo', UL: 'list', OL: 'list', LI: 'listitem',
    TABLE: 'table', FORM: 'form', DIALOG: 'dialog',
    H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', H5: 'heading', H6: 'heading'
  };
  function roleOf(el) {
    var r = el.getAttribute && el.getAttribute('role');
    if (r) return r.toLowerCase();
    var tag = el.tagName;
    if (tag === 'A') return el.hasAttribute('href') ? 'link' : '';
    if (tag === 'INPUT') {
      var t = (el.getAttribute('type') || 'text').toLowerCase();
      if (t === 'button' || t === 'submit' || t === 'reset') return 'button';
      if (t === 'checkbox') return 'checkbox';
      if (t === 'radio') return 'radio';
      if (t === 'range') return 'slider';
      return 'textbox';
    }
    return IMPLICIT_ROLES[tag] || '';
  }

  // 无障碍名称：aria-label → labelledby → alt → title → 按钮型 input 的 value → 可见文本
  function accName(el) {
    var s = el.getAttribute('aria-label');
    if (s) return normText(s);
    var lb = el.getAttribute('aria-labelledby');
    if (lb) {
      var doc = el.ownerDocument;
      var parts = lb.split(/\s+/), out = [];
      for (var i = 0; i < parts.length; i++) {
        var t = doc.getElementById(parts[i]);
        if (t) out.push(t.innerText || t.textContent || '');
      }
      if (out.length) return normText(out.join(' '));
    }
    s = el.getAttribute('alt'); if (s) return normText(s);
    s = el.getAttribute('title'); if (s) return normText(s);
    if (el.tagName === 'INPUT' && /^(button|submit|reset)$/i.test(el.getAttribute('type') || '')) {
      return normText(el.value || '');
    }
    return normText(el.innerText || el.textContent || '');
  }

  // 稳定 selector：tag:nth-of-type 链（不引用 class，避开构建 hash）；跨 shadow 时标注 in_shadow
  function stablePath(el) {
    // 先于 id 短路检测：shadow 内元素即使带 id 也必须标注 in_shadow
    var inShadow = false;
    if (el.getRootNode) {
      var r0 = el.getRootNode();
      inShadow = !!(r0 && r0.nodeType === 11 && r0.host);
    }
    var parts = [];
    var cur = el;
    var esc = (window.CSS && window.CSS.escape) ? window.CSS.escape : function (s) { return s; };
    while (cur && cur.nodeType === 1) {
      if (cur.id) {
        parts.unshift('#' + esc(cur.id));
        return { selector: parts.join(' > '), in_shadow: inShadow };
      }
      var tag = cur.tagName.toLowerCase();
      var parent = cur.parentElement;
      if (!parent && cur.getRootNode) {
        var root = cur.getRootNode();
        if (root && root.host) { inShadow = true; parent = root.host; }
      }
      if (parent) {
        var sameTag = 0, nth = 0;
        var sibs = parent.children;
        for (var i = 0; i < sibs.length; i++) {
          if (sibs[i].tagName === cur.tagName) { sameTag++; if (sibs[i] === cur) nth = sameTag; }
        }
        parts.unshift(sameTag > 1 ? tag + ':nth-of-type(' + nth + ')' : tag);
      } else {
        parts.unshift(tag);
      }
      cur = parent;
    }
    return { selector: parts.join(' > '), in_shadow: inShadow };
  }

  // 坐标换算：同源 iframe 内元素要逐层加 frameElement 偏移，否则脑拿到局部坐标
  function absRect(el) {
    var r = el.getBoundingClientRect();
    var x = r.left, y = r.top;
    var win = el.ownerDocument ? el.ownerDocument.defaultView : null;
    var depth = 0;
    while (win && win !== window && depth < 8) {
      var fe = win.frameElement;
      if (!fe) break;
      var fr = fe.getBoundingClientRect();
      x += fr.left; y += fr.top;
      win = win.parent;
      depth++;
    }
    return { x: Math.round(x), y: Math.round(y), w: Math.round(r.width), h: Math.round(r.height) };
  }

  function isPassword(el) {
    return el.tagName === 'INPUT' && (el.getAttribute('type') || '').toLowerCase() === 'password';
  }

  // wait_for 条件检查（Rust 侧轮询，250ms 一跳）
  if (ACTION === 'check') {
    var exists = false;
    var hitEl = null;
    if (SELECTOR) {
      var loc = locate();
      if (loc.els && loc.els.length > 0) { exists = true; hitEl = loc.els[0]; }
    }
    var met;
    if (CONDITION === 'load') met = document.readyState === 'complete';
    else if (CONDITION === 'gone') met = !exists;
    else if (CONDITION === 'text') {
      // 元素可见文本满足 expect（contains/equals/regex；非法正则回 bad_selector 不崩）
      met = false;
      if (hitEl && EXPECT) {
        var t = normText(hitEl.innerText || hitEl.textContent || '');
        if (EXPECT.op === 'contains') met = t.indexOf(EXPECT.value) >= 0;
        else if (EXPECT.op === 'equals') met = t === EXPECT.value;
        else if (EXPECT.op === 'regex') {
          try { met = new RegExp(EXPECT.value).test(t); }
          catch (e) { return actFail('bad_selector', 'expect.value 非法正则: ' + e.message); }
        }
      }
    }
    else if (CONDITION === 'enabled') met = !!(hitEl && !hitEl.disabled && visible(hitEl));
    else if (CONDITION === 'idle' || CONDITION === 'idle_cleanup') {
      // idle：计数器挂 window.__shiftIdle 页面级持久对象（每次轮询重新 eval，局部变量会丢）。
      // 首次 eval 安装 fetch/XHR 包装 + MutationObserver；后续 eval 只读计数；
      // 超时/命中后 Rust 侧补发 idle_cleanup 恢复原函数，不污染页面。
      var IDLE_KEY = '__shiftIdle';
      if (CONDITION === 'idle_cleanup') {
        var st0 = window[IDLE_KEY];
        if (st0) {
          try { if (st0.origFetch) window.fetch = st0.origFetch; } catch (e) {}
          try {
            if (st0.origOpen) window.XMLHttpRequest.prototype.open = st0.origOpen;
            if (st0.origSend) window.XMLHttpRequest.prototype.send = st0.origSend;
          } catch (e) {}
          try { if (st0.obs) st0.obs.disconnect(); } catch (e) {}
          try { delete window[IDLE_KEY]; } catch (e) { window[IDLE_KEY] = undefined; }
        }
        return finish({ met: true, cleaned: true });
      }
      var st = window[IDLE_KEY];
      if (!st) {
        st = { inflight: 0, lastMut: Date.now(), origFetch: null, origOpen: null, origSend: null, obs: null };
        if (typeof window.fetch === 'function') {
          st.origFetch = window.fetch;
          window.fetch = function () {
            st.inflight++;
            var done = function () { st.inflight--; };
            try {
              var p = st.origFetch.apply(this, arguments);
              p.then(done, done);
              return p;
            } catch (e) { done(); throw e; }
          };
        }
        if (window.XMLHttpRequest) {
          var XP = window.XMLHttpRequest.prototype;
          st.origOpen = XP.open;
          st.origSend = XP.send;
          XP.open = function () { this.__shiftCounted = false; return st.origOpen.apply(this, arguments); };
          XP.send = function () {
            if (!this.__shiftCounted) {
              this.__shiftCounted = true;
              st.inflight++;
              var dec = function () { st.inflight--; };
              this.addEventListener('loadend', dec);
              this.addEventListener('error', dec);
              this.addEventListener('abort', dec);
            }
            return st.origSend.apply(this, arguments);
          };
        }
        try {
          st.obs = new MutationObserver(function () { st.lastMut = Date.now(); });
          st.obs.observe(document.documentElement || document, { childList: true, subtree: true, attributes: true, characterData: true });
        } catch (e) {}
        window[IDLE_KEY] = st;
      }
      met = st.inflight <= 0 && (Date.now() - st.lastMut) >= QUIET_MS;
      return finish({ met: met, inflight: st.inflight, quiet_ms: QUIET_MS });
    }
    else met = exists; // appear（默认）
    return finish({ met: met, exists: exists, ready_state: document.readyState || '' });
  }

  // action/find（只读定位，缺口 2/6）：text / role+name / selector 三种瞄准。
  // 必须在下方共用预检之前——find 允许"找不到"是合法结果（附 diagnose），不走 not_found 早退。
  if (ACTION === 'find') {
    var fBase = descendFrames();
    if (fBase.err === 'cross') {
      return finish({
        found: 0, returned: 0, frame_cross_origin: true, frame_index: fBase.index,
        hint: 'frames 路径第 ' + fBase.index + ' 层为跨源 iframe，无法穿透（只能用键盘/坐标绕）'
      });
    }
    if (fBase.err) return finish({ found: 0, returned: 0, reason: fBase.err, error: fBase.msg });
    var fDoc = fBase.doc;

    var roots = [fDoc];
    if (WITHIN) {
      var conts = qsaDeep(fDoc, WITHIN, PIERCE === 1);
      if (!conts) return finish({ found: 0, returned: 0, reason: 'bad_selector', error: 'within 选择器非法' });
      roots = conts;
    }

    var matches = [];
    if (SELECTOR) {
      for (var ri = 0; ri < roots.length; ri++) {
        var got = qsaDeep(roots[ri], SELECTOR, PIERCE === 1);
        if (!got) return finish({ found: 0, returned: 0, reason: 'bad_selector', error: 'selector 非法' });
        for (var gi = 0; gi < got.length; gi++) if (matches.indexOf(got[gi]) < 0) matches.push(got[gi]);
      }
    } else {
      var wantRole = ROLE ? ROLE.toLowerCase() : '';
      var TEXT_N = normText(TEXT); // find 侧归一化（type 用的是原始 TEXT，首尾空格保留）
      var wantText = TEXT_N ? TEXT_N.toLowerCase() : '';
      for (var rj = 0; rj < roots.length; rj++) {
        var cands = qsaDeep(roots[rj], '*', PIERCE === 1) || [];
        for (var cj = 0; cj < cands.length; cj++) {
          var el = cands[cj];
          if (isPassword(el)) continue; // 诊断/定位一律不碰密码框
          if (wantRole) {
            if (roleOf(el) !== wantRole) continue;
            if (NAME) {
              var nm = accName(el);
              if (!(EXACT === 1 ? nm === NAME : nm.toLowerCase().indexOf(NAME.toLowerCase()) >= 0)) continue;
            }
          } else {
            var t = normText(el.innerText || el.textContent || '');
            if (!t) continue;
            if (!(EXACT === 1 ? t === TEXT_N : t.toLowerCase().indexOf(wantText) >= 0)) continue;
          }
          if (matches.indexOf(el) < 0) matches.push(el);
          if (matches.length > 200) break; // 防御：异常大页面
        }
      }
      // 纯文本瞄准只留最内层命中（父容器文本天然包含子元素文本）
      if (!wantRole) {
        var inner = [];
        for (var ii = 0; ii < matches.length; ii++) {
          var keep = true;
          for (var jj = 0; jj < matches.length; jj++) {
            if (ii !== jj && matches[ii].contains(matches[jj])) { keep = false; break; }
          }
          if (keep) inner.push(matches[ii]);
        }
        matches = inner;
      }
    }
    if (VISIBLE_ONLY === 1) {
      var vm = [];
      for (var vi = 0; vi < matches.length; vi++) if (visible(matches[vi])) vm.push(matches[vi]);
      matches = vm;
    }
    var found = matches.length;
    if (found === 0) return finish({ found: 0, returned: 0, diagnose: diagnose(fDoc) });
    var nth = NTH < 0 ? 0 : NTH;
    if (nth >= found) {
      return finish({ found: found, returned: 0, reason: 'nth_out_of_range', error: 'nth 越界（共 ' + found + ' 个）' });
    }
    var hit = matches[nth];
    var sp = stablePath(hit);
    var out = {
      found: found, returned: 1, nth: nth,
      tag: hit.tagName.toLowerCase(), role: roleOf(hit) || null,
      text: normText(hit.innerText || hit.textContent || '').slice(0, 40),
      enabled: !hit.disabled, visible: visible(hit),
      rect: absRect(hit), selector: sp.selector
    };
    if (sp.in_shadow) out.in_shadow = true;
    if (fBase.depth > 0) out.in_frame = true;
    return finish(out);
  }

  // 缺口 6 失败诊断（只读、限量：nearby≤10 / same_text≤5 / in_iframe≤10，文本截 40 字）
  function diagnose(fDoc) {
    var dg = { reason: 'not_found' };
    try {
      // nearby：页面可见可交互元素样本（密码框除外），让脑知道"页面上有什么"
      var INTERACTIVE = 'a,button,input,select,textarea,[role],[onclick],[contenteditable]';
      var sample = qsaDeep(fDoc, INTERACTIVE, false) || [];
      var nearby = [];
      for (var i = 0; i < sample.length && nearby.length < 10; i++) {
        var el = sample[i];
        if (isPassword(el) || !visible(el)) continue;
        var cls = (typeof el.className === 'string' ? el.className : '').split(/\s+/).slice(0, 2).join(' ');
        nearby.push({ tag: el.tagName.toLowerCase(), text: normText(el.innerText || el.textContent || '').slice(0, 40), 'class': cls });
      }
      dg.nearby = nearby;
      // same_text：有 text 瞄准时，用前缀放宽搜一遍（可能只差空格/大小写/截断）
      if (TEXT) {
        var key = TEXT.slice(0, 8).toLowerCase();
        var same = [];
        if (key) {
          var all = qsaDeep(fDoc, '*', PIERCE === 1) || [];
          for (var j = 0; j < all.length && same.length < 5; j++) {
            var e2 = all[j];
            if (isPassword(e2)) continue;
            var t2 = normText(e2.innerText || e2.textContent || '').toLowerCase();
            if (t2 && t2.indexOf(key) >= 0) {
              var hasChildHit = false;
              for (var k2 = 0; k2 < all.length; k2++) {
                if (k2 !== j && e2.contains(all[k2])) {
                  var t3 = normText(all[k2].innerText || all[k2].textContent || '').toLowerCase();
                  if (t3 && t3.indexOf(key) >= 0) { hasChildHit = true; break; }
                }
              }
              if (!hasChildHit) same.push({ selector: stablePath(e2).selector, rect: absRect(e2) });
            }
          }
        }
        dg.same_text = same;
      }
      // in_iframe：扫主文档 iframe；跨源只报存在与 same_origin:false，不吐内容（不谎报可点）
      var frames = qsa1(document, 'iframe,frame');
      var infos = [];
      var hitFrames = [];
      for (var fi = 0; fi < frames.length && fi < 10; fi++) {
        var rec = { frame: fi, same_origin: false };
        var cd = null;
        try { cd = frames[fi].contentDocument; } catch (e) { cd = null; }
        if (cd) {
          rec.same_origin = true;
          if (TEXT) {
            var bt = cd.body ? normText(cd.body.innerText || cd.body.textContent || '') : '';
            if (bt.toLowerCase().indexOf(TEXT.toLowerCase()) >= 0) {
              rec.has_text = TEXT.slice(0, 40);
              hitFrames.push(fi);
            }
          }
        }
        infos.push(rec);
      }
      if (infos.length) dg.in_iframe = infos;
      if (hitFrames.length) dg.hint = '第 ' + hitFrames.join('/') + ' 个 iframe 内含目标文本，可试 frames:[' + hitFrames[0] + ']';
    } catch (e) { /* 诊断是尽力而为，不覆盖 not_found 主结论 */ }
    return dg;
  }

  if (ACTION === 'back') {
    try { window.history.back(); return finish({ ok_action: 'back' }); }
    catch (e) { return actFail('page_changed', e.message); }
  }

  if (ACTION === 'scroll') {
    try {
      var h = document.documentElement ? document.documentElement.scrollHeight : 0;
      var step = AMOUNT > 0 ? AMOUNT : 600;
      var to;
      if (DIRECTION === 'top') to = 0;
      else if (DIRECTION === 'bottom') to = h;
      else if (DIRECTION === 'up') to = window.scrollY - step;
      else to = window.scrollY + step;
      window.scrollTo(0, Math.max(0, to));
      return finish({ ok_action: 'scroll', scrollY: Math.round(window.scrollY), height: h });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  // action/key（I8）：派发键盘事件（可选先聚焦 selector 命中元素，否则作用于当前焦点）
  // modifiers（缺口 5）：组合键，仅白名单 Control/Shift/Alt/Meta；只对页面 JS 监听器有效。
  if (ACTION === 'key') {
    if (!KEY) return actFail('bad_action', '缺少 key');
    try {
      var targetEl = null;
      if (SELECTOR) {
        var kl = locate();
        if (kl.cross) return actFail('not_found', 'frames 路径第 ' + kl.index + ' 层为跨源 iframe，无法穿透', { frame_cross_origin: true, frame_index: kl.index });
        if (kl.frame_gap) return actFail(kl.code === 'bad_selector' ? 'bad_selector' : 'not_found', kl.msg);
        if (kl.bad_selector) return actFail('bad_selector', '选择器非法: ' + kl.bad_selector);
        if (!kl.els || kl.els.length === 0) return actFail('not_found', '未找到元素: ' + SELECTOR);
        targetEl = kl.els[INDEX < 0 ? 0 : INDEX];
        if (targetEl.focus) targetEl.focus();
      }
      var el2 = targetEl || document.activeElement || document.body;
      var opts = { key: KEY, bubbles: true, cancelable: true };
      for (var mi = 0; mi < MODIFIERS.length; mi++) {
        if (MODIFIERS[mi] === 'Control') opts.ctrlKey = true;
        else if (MODIFIERS[mi] === 'Shift') opts.shiftKey = true;
        else if (MODIFIERS[mi] === 'Alt') opts.altKey = true;
        else if (MODIFIERS[mi] === 'Meta') opts.metaKey = true;
      }
      el2.dispatchEvent(new KeyboardEvent('keydown', opts));
      el2.dispatchEvent(new KeyboardEvent('keypress', opts));
      el2.dispatchEvent(new KeyboardEvent('keyup', opts));
      return finish({ ok_action: 'key', key: KEY, modifiers: MODIFIERS, tag: el2.tagName ? el2.tagName.toLowerCase() : '' });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  // action/type（缺口 5）：向当前焦点逐字符输入（纯字符串，不执行）。
  // 合成事件 isTrusted=false 不写值，必须：input/textarea 走 native setter + InputEvent；
  // contenteditable 走 Selection/Range 插入文本节点。逐字符走完整流程（受控组件防覆盖）。
  if (ACTION === 'type') {
    if (!TEXT && CLEAR !== 1) return actFail('bad_action', '缺少 text');
    try {
      var tEl;
      if (SELECTOR) {
        // 可选 selector：定位+聚焦+输入在同一脚本同步完成（跨 eval 焦点可能被 WebKit 清掉）
        var tl = locate();
        if (tl.cross) return actFail('not_found', 'frames 路径第 ' + tl.index + ' 层为跨源 iframe，无法穿透', { frame_cross_origin: true, frame_index: tl.index });
        if (tl.frame_gap) return actFail(tl.code === 'bad_selector' ? 'bad_selector' : 'not_found', tl.msg);
        if (tl.bad_selector) return actFail('bad_selector', '选择器非法: ' + tl.bad_selector);
        if (!tl.els || tl.els.length === 0) return actFail('not_found', '未找到元素: ' + SELECTOR);
        var tidx = INDEX < 0 ? 0 : INDEX;
        if (tidx >= tl.els.length) return actFail('not_found', 'index 越界（共 ' + tl.els.length + ' 个）');
        tEl = tl.els[tidx];
        if (!visible(tEl)) return actFail('not_visible', '元素不可见');
        if (tEl.disabled) return actFail('disabled', '元素已禁用');
      } else {
        tEl = document.activeElement || document.body;
      }
      if (isPassword(tEl)) return actFail('password_refused', '拒绝向密码框输入');
      var typed = 0;
      var chars = [];
      for (var chi = 0; chi < TEXT.length; chi++) chars.push(TEXT[chi]);
      if (tEl.tagName === 'INPUT' || tEl.tagName === 'TEXTAREA') {
        var proto = tEl.tagName === 'INPUT' ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
        var desc = Object.getOwnPropertyDescriptor(proto, 'value');
        if (!desc || !desc.set) return actFail('bad_action', '无法取得原生 value setter');
        if (tEl.focus) tEl.focus();
        if (CLEAR === 1) {
          desc.set.call(tEl, '');
          tEl.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'deleteContentBackward' }));
        }
        for (var ci = 0; ci < chars.length; ci++) {
          var cur = desc.get.call(tEl);
          var s0 = tEl.selectionStart, s1 = tEl.selectionEnd;
          var nv = (typeof s0 === 'number' && typeof s1 === 'number')
            ? cur.slice(0, s0) + chars[ci] + cur.slice(s1)
            : cur + chars[ci];
          desc.set.call(tEl, nv);
          var pos = (typeof s0 === 'number') ? s0 + 1 : nv.length;
          try { tEl.setSelectionRange(pos, pos); } catch (e) {}
          tEl.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: chars[ci] }));
          typed++;
        }
      } else if (tEl.isContentEditable) {
        if (tEl.focus) tEl.focus();
        var tDoc = tEl.ownerDocument;
        var sel0 = tDoc.getSelection();
        if (CLEAR === 1) {
          sel0.selectAllChildren(tEl);
          sel0.deleteFromDocument();
        }
        for (var cj = 0; cj < chars.length; cj++) {
          var sel = tDoc.getSelection();
          if (!sel.rangeCount) {
            var r0 = tDoc.createRange();
            r0.selectNodeContents(tEl);
            r0.collapse(false);
            sel.addRange(r0);
          }
          var range = sel.getRangeAt(0);
          range.deleteContents();
          var node = tDoc.createTextNode(chars[cj]);
          range.insertNode(node);
          range.setStartAfter(node);
          range.setEndAfter(node);
          sel.removeAllRanges();
          sel.addRange(range);
          tEl.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: chars[cj] }));
          typed++;
        }
      } else {
        return actFail('not_editable', '当前焦点不是输入框/可编辑元素（tag: ' + (tEl.tagName || '').toLowerCase() + '）');
      }
      return finish({ ok_action: 'type', typed: typed, cleared: CLEAR === 1, tag: tEl.tagName.toLowerCase() });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  // 交互预检（I2）：统一定位（frames 下钻 + pierce 穿透）；跨源显式报 frame_cross_origin 不谎报
  var L = locate();
  if (L.cross) return actFail('not_found', 'frames 路径第 ' + L.index + ' 层为跨源 iframe，无法穿透（只能用键盘/坐标绕）', { frame_cross_origin: true, frame_index: L.index });
  if (L.frame_gap) return actFail(L.code === 'bad_selector' ? 'bad_selector' : 'not_found', L.msg);
  if (L.bad_selector) return actFail('bad_selector', '选择器非法: ' + L.bad_selector);
  var els = L.els;
  if (!els || els.length === 0) return actFail('not_found', '未找到元素: ' + SELECTOR);
  var idx = INDEX < 0 ? 0 : INDEX;
  if (idx >= els.length) return actFail('not_found', 'index 越界（共 ' + els.length + ' 个）');
  var el = els[idx];
  if (!visible(el)) return actFail('not_visible', '元素不可见');
  if (el.disabled) return actFail('disabled', '元素已禁用');

  if (ACTION === 'click') {
    var before = location.href;
    try {
      // 显式 focus：真实点击的默认行为含聚焦；合成 click() 在非活动标签上不转移焦点，
      // 显式 focus 保证后续 action/type（无 selector 时）拿得到焦点
      if (el.focus) el.focus();
      el.click();
    } catch (e) { return actFail('page_changed', e.message); }
    return finish({
      ok_action: 'click',
      url_before: before,
      url_after: location.href,
      navigated: location.href !== before
    });
  }

  if (ACTION === 'fill') {
    try {
      if (el.focus) el.focus();
      el.value = VALUE; // 只落字符串，不执行
      el.dispatchEvent(new Event('input', { bubbles: true }));
      el.dispatchEvent(new Event('change', { bubbles: true }));
      return finish({ ok_action: 'fill', tag: el.tagName.toLowerCase() });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  if (ACTION === 'hover') {
    try {
      el.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
      el.dispatchEvent(new MouseEvent('mouseenter', { bubbles: false }));
      el.dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
      return finish({ ok_action: 'hover', tag: el.tagName.toLowerCase() });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  return actFail('bad_action', '未知动作: ' + ACTION);
})();

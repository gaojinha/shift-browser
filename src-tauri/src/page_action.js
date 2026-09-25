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

  var _docQsa = Document.prototype.querySelectorAll;
  var _gcs = window.getComputedStyle;

  function finish(v) { return JSON.stringify(v); }
  function actFail(code, msg) {
    return JSON.stringify({ error: msg || code, action_error: code });
  }

  function visible(el) {
    try {
      var cs = _gcs.call(window, el);
      if (cs.display === 'none' || cs.visibility === 'hidden') return false;
      var r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    } catch (e) { return true; }
  }

  // wait_for 条件检查（Rust 侧轮询）
  if (ACTION === 'check') {
    var exists = false;
    try { exists = _docQsa.call(document, SELECTOR).length > 0; } catch (e) { exists = false; }
    var met;
    if (CONDITION === 'load') met = document.readyState === 'complete';
    else if (CONDITION === 'gone') met = !exists;
    else met = exists; // appear（默认）
    return finish({ met: met, exists: exists, ready_state: document.readyState || '' });
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
  if (ACTION === 'key') {
    if (!KEY) return actFail('bad_action', '缺少 key');
    try {
      var targetEl = null;
      if (SELECTOR) {
        var ks = _docQsa.call(document, SELECTOR);
        if (!ks || ks.length === 0) return actFail('not_found', '未找到元素: ' + SELECTOR);
        targetEl = ks[INDEX < 0 ? 0 : INDEX];
        if (targetEl.focus) targetEl.focus();
      }
      var el2 = targetEl || document.activeElement || document.body;
      var opts = { key: KEY, bubbles: true, cancelable: true };
      el2.dispatchEvent(new KeyboardEvent('keydown', opts));
      el2.dispatchEvent(new KeyboardEvent('keypress', opts));
      el2.dispatchEvent(new KeyboardEvent('keyup', opts));
      return finish({ ok_action: 'key', key: KEY, tag: el2.tagName ? el2.tagName.toLowerCase() : '' });
    } catch (e) { return actFail('page_changed', e.message); }
  }

  // 交互预检（I2）
  var els;
  try { els = _docQsa.call(document, SELECTOR); }
  catch (e) { return actFail('bad_selector', '选择器非法: ' + e.message); }
  if (!els || els.length === 0) return actFail('not_found', '未找到元素: ' + SELECTOR);
  var idx = INDEX < 0 ? 0 : INDEX;
  if (idx >= els.length) return actFail('not_found', 'index 越界（共 ' + els.length + ' 个）');
  var el = els[idx];
  if (!visible(el)) return actFail('not_visible', '元素不可见');
  if (el.disabled) return actFail('disabled', '元素已禁用');

  if (ACTION === 'click') {
    var before = location.href;
    try { el.click(); } catch (e) { return actFail('page_changed', e.message); }
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

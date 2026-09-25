// action/dismiss 固定弹窗关闭脚本（编译期嵌入，include_str! 于 api/handlers/page.rs）
//
// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（§4.3 action/dismiss · B8/H3）
// 原则：**只点可判定的浮层**（弹窗/遮罩/浮层内的关闭按钮、或带 close 语义的 aria-label），
//       识别不了就返回提示、绝不盲点。脚本自身不抛异常。

(function () {
  function finish(v) { return JSON.stringify(v); }
  var _docQsa = Document.prototype.querySelectorAll;
  var _gcs = window.getComputedStyle;

  function visible(el) {
    try {
      var cs = _gcs.call(window, el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false;
      var r = el.getBoundingClientRect();
      return r.width > 0 && r.height > 0;
    } catch (e) { return true; }
  }

  function textOf(el) {
    var t = el.innerText;
    if (t == null) t = el.textContent;
    return (t || '').trim().replace(/\s+/g, ' ');
  }

  // 弹窗/浮层容器判定（限定点击范围，避免误点页面主按钮）
  function inOverlay(el) {
    try {
      return !!(el.closest && el.closest(
        '[role="dialog"], [aria-modal="true"], [role="alertdialog"],' +
        '[class*="modal" i], [class*="dialog" i], [class*="popup" i],' +
        '[class*="overlay" i], [class*="cookie" i], [class*="consent" i], [class*="mask" i]'
      ));
    } catch (e) { return false; }
  }

  // 关闭语义文案白名单（精确短文本，避免长文案误命中）
  var TEXTS = ['同意', '接受', '我同意', '我接受', '允许', '确定', '好的', '知道了',
    '我知道了', '关闭', '跳过', '不再提示', '以后再说',
    'accept', 'accept all', 'agree', 'close', 'ok', 'got it', 'allow', 'dismiss'];

  try {
    var cands = _docQsa.call(document,
      'button, [role="button"], a[href="javascript:void(0)"], input[type="button"], input[type="submit"]');

    // ① 明确 close 语义（aria-label / title 含 close/关闭）——最高优先
    for (var i = 0; i < cands.length; i++) {
      var el = cands[i];
      if (!visible(el)) continue;
      var aria = (el.getAttribute('aria-label') || '') + ' ' + (el.getAttribute('title') || '');
      if (/close|关闭|dismiss/i.test(aria) && inOverlay(el)) {
        el.click();
        return finish({ dismissed: true, by: 'aria-close', text: textOf(el).slice(0, 30) });
      }
    }

    // ② 浮层内的白名单文案按钮
    for (var j = 0; j < cands.length; j++) {
      var b = cands[j];
      if (!visible(b) || !inOverlay(b)) continue;
      var t = textOf(b).slice(0, 20);
      if (!t) continue;
      var low = t.toLowerCase();
      for (var k = 0; k < TEXTS.length; k++) {
        if (low === TEXTS[k] || low === TEXTS[k] + '。' || low === TEXTS[k] + '！') {
          b.click();
          return finish({ dismissed: true, by: 'text', text: t });
        }
      }
    }

    return finish({ dismissed: false, hint: '未找到可识别的弹窗（不盲点）；如有需要可自行 action/click' });
  } catch (e) {
    return finish({ dismissed: false, hint: '弹窗识别失败: ' + e.message });
  }
})();

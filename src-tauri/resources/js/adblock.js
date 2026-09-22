
// ShiftBrowser 广告拦截：文本特征规则 + 动态监听
(function () {
  if (window.__shiftAdblockInstalled) return;
  window.__shiftAdblockInstalled = true;
  if (location.protocol !== 'http:' && location.protocol !== 'https:') return;
  var keywords = __SB_KEYWORDS__;
  var HIDDEN = 'data-sb-hidden';
  function hideByText() {
    var nodes = document.querySelectorAll('div, section, aside');
    // 性能保护：超大 DOM（几十万节点的重型 SPA）全扫会卡主线程，超过 3000 节点直接放弃文本扫描
    // （CSS [class*=ad]/[id*=ad] 规则仍会生效，只是文本特征词兜底失效，可接受）
    if (nodes.length > 3000) return;
    for (var i = 0; i < nodes.length; i++) {
      var el = nodes[i];
      if (el.getAttribute(HIDDEN)) continue;
      if (el.offsetWidth === 0 && el.offsetHeight === 0) continue;
      if (el.offsetHeight > 520) continue; // 跳过大块页面区域，避免误杀正文
      var text = el.textContent || '';
      if (text.length < 4 || text.length > 400) continue;
      for (var k = 0; k < keywords.length; k++) {
        if (keywords[k] && text.indexOf(keywords[k]) !== -1) {
          el.style.display = 'none';
          el.setAttribute(HIDDEN, '1');
          break;
        }
      }
    }
  }
  hideByText();
  var timer = null;
  var observer = new MutationObserver(function () {
    if (timer) return;
    timer = setTimeout(function () {
      timer = null;
      hideByText();
    }, 500);
  });
  if (document.body) {
    observer.observe(document.body, { childList: true, subtree: true });
  }
})();

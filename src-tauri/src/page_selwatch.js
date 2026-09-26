// 选区缓存监听（页面加载前注入，对所有标签页生效）
//
// 为什么需要它：用户"选中文字 → 点侧栏按钮"时，点击会让页面 webview 失焦，
// 浏览器会立刻清空选区。等按钮的 handler 去读 window.getSelection() 时，
// 选区早就没了，于是永远报"没读到选中内容"（真机实测踩到）。
//
// 做法：选中的一瞬间（selectionchange）把 Range 克隆一份存到 window 上，
// 读取端（page_selection.js）在没有当前选区时回退到这份缓存。
// 只存 Range 快照，**不改页面、不派发事件、不聚焦**；页面重渲染导致 Range
// 失效时读取端会捕获异常并如实报"没读到"。
//
// 幂等：__sbSelWatch 防重复注入（UserScript 与本注入可能都跑一遍）。
(function () {
  if (window.__sbSelWatch) return;
  window.__sbSelWatch = 1;
  try {
    document.addEventListener(
      'selectionchange',
      function () {
        try {
          var s = window.getSelection && window.getSelection();
          if (!s || s.rangeCount === 0) return;
          var text = String(s);
          if (!text || !text.trim()) return;
          window.__sbLastRange = s.getRangeAt(0).cloneRange();
          window.__sbLastSelAt = Date.now();
        } catch (e) {
          /* 选区不可克隆（跨 shadow / 已失效）就放弃本次缓存，不抛 */
        }
      },
      true,
    );
  } catch (e) {
    /* 注入环境异常不影响页面 */
  }
})();

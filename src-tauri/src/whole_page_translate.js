// 整页翻译注入脚本（child webview 内执行）
//
// 由 Rust 侧 shift://__translate__/whole-page 处理器 eval 注入本脚本。
// 流程：
//   1. 遍历文本节点（跳过 script/style/code/隐藏元素/纯数字标点）
//   2. 分批（每批 15 段）通过 shift://__translate__/batch 调后端翻译
//   3. 轮询 window.__shift_translate_result_<cb> 拿译文，回填 nodeValue
//   4. 右上角浮层显示进度
// 注意：本脚本被 eval 到任意第三方页面，变量全部挂 window.__shift_wpt_* 避免污染，
// 且用 IIFE 包裹；不依赖任何页面全局。
(function () {
  if (window.__shift_wpt_active) return;
  window.__shift_wpt_active = true;

  var BATCH = 15;
  var POLL_MS = 120;
  var POLL_MAX = 400; // 单批最多等 48s

  // 跳过这些标签内的文本
  var SKIP_TAGS = {
    SCRIPT: 1, STYLE: 1, NOSCRIPT: 1, TEXTAREA: 1, CODE: 1, PRE: 1,
    IFRAME: 1, CANVAS: 1, SVG: 1, MATH: 1, SELECT: 1, OPTION: 1, KBD: 1, SAMP: 1,
  };

  // ---- 进度浮层 ----
  var box = document.createElement('div');
  box.setAttribute('data-shift-wpt', '1');
  box.style.cssText =
    'position:fixed;top:14px;right:14px;z-index:2147483647;background:rgba(20,20,24,.92);' +
    'color:#eee;font:13px/1.5 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif;' +
    'padding:10px 14px;border-radius:8px;box-shadow:0 4px 18px rgba(0,0,0,.4);min-width:150px;';
  box.textContent = '正在收集页面文本…';
  (document.body || document.documentElement).appendChild(box);

  function setProgress(done, total, msg) {
    if (msg) {
      box.textContent = msg;
      return;
    }
    box.textContent = '整页翻译中 ' + done + '/' + total;
  }

  function finish(msg) {
    box.textContent = msg;
    setTimeout(function () {
      try { box.remove(); } catch (e) {}
      window.__shift_wpt_active = false;
    }, 2500);
  }

  // ---- 收集文本节点 ----
  function collect() {
    var out = [];
    if (!document.body) return out;
    var walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT, {
      acceptNode: function (n) {
        var v = n.nodeValue;
        if (!v || !v.trim()) return NodeFilter.FILTER_REJECT;
        var p = n.parentNode;
        if (!p) return NodeFilter.FILTER_REJECT;
        var tag = p.nodeName ? p.nodeName.toUpperCase() : '';
        if (SKIP_TAGS[tag]) return NodeFilter.FILTER_REJECT;
        if (p.isContentEditable) return NodeFilter.FILTER_REJECT;
        // 纯数字/空白/标点（无可译内容）
        if (!/[A-Za-z\u00c0-\u024f\u0400-\u04ff\u3040-\u30ff\u4e00-\u9fff\uac00-\ud7af]/.test(v)) {
          return NodeFilter.FILTER_REJECT;
        }
        // 隐藏元素跳过
        try {
          var st = window.getComputedStyle(p);
          if (st && (st.display === 'none' || st.visibility === 'hidden')) {
            return NodeFilter.FILTER_REJECT;
          }
        } catch (e) {}
        return NodeFilter.FILTER_ACCEPT;
      },
    });
    while (walker.nextNode()) out.push(walker.currentNode);
    return out;
  }

  var nodes = collect();
  if (nodes.length === 0) {
    finish('没有可翻译的文本');
    return;
  }

  // 去重：相同原文只翻一次（回填时统一写）
  var uniq = [];
  var seen = Object.create(null);
  var indexOf = []; // 每个 node 对应 uniq 下标
  for (var i = 0; i < nodes.length; i++) {
    var key = nodes[i].nodeValue.trim();
    if (seen[key] === undefined) {
      seen[key] = uniq.length;
      uniq.push(key);
    }
    indexOf.push(seen[key]);
  }

  var total = uniq.length;
  var translated = new Array(total);
  var batches = [];
  for (var s = 0; s < total; s += BATCH) batches.push([s, Math.min(s + BATCH, total)]);

  // 逐批翻译（顺序批处理，避免瞬时并发过高）
  var bi = 0;

  function nextBatch() {
    if (bi >= batches.length) {
      applyAll();
      return;
    }
    var range = batches[bi];
    var slice = uniq.slice(range[0], range[1]);
    var cb = 'wpt' + Date.now() + '_' + bi;
    var key = 'shift_cache_wpt_' + cb;
    try { window[key] = null; } catch (e) {}

    // 通过 Image().src 触发（shift handler 同步返回 1x1 gif）
    var payload = encodeURIComponent(JSON.stringify(slice));
    var img = new Image();
    img.src = 'shift://__translate__/batch?callback=' + cb + '&texts=' + payload;

    var waited = 0;
    var timer = setInterval(function () {
      waited += POLL_MS;
      var res = window['__shift_translate_result_' + cb];
      if (res) {
        clearInterval(timer);
        if (res.error) {
          finish('翻译失败：' + res.error);
          return;
        }
        var arr = res.texts || [];
        for (var k = 0; k < arr.length; k++) translated[range[0] + k] = arr[k];
        bi++;
        setProgress(Math.min(range[1], total), total);
        nextBatch();
      } else if (waited >= POLL_MS * POLL_MAX) {
        clearInterval(timer);
        finish('翻译超时（网络问题？）');
      }
    }, POLL_MS);
  }

  // ---- 回填 ----
  function applyAll() {
    var ok = 0;
    for (var i = 0; i < nodes.length; i++) {
      var t = translated[indexOf[i]];
      if (t) {
        try {
          nodes[i].nodeValue = t;
          ok++;
        } catch (e) {}
      }
    }
    finish('整页翻译完成（' + ok + ' 处）');
  }

  setProgress(0, total);
  nextBatch();
})();

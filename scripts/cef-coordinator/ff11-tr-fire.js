(() => {
  // F-CEF-11 翻译通道端到端·发起：真发一次划词翻译请求，验宿主通道在 CEF 下通不通
  // 背景：__sbTrSend 双通道——主 window.webkit.messageHandlers.shiftTranslate（WebKit 专属，
  //       为躲站点 CSP 于 2026-09-21 启用）；回退 shift://__translate__/ + Image().src
  //       （Tauri protocol handler，CEF 侧未见注册，且受 img-src CSP 拦）。
  // 判据：本脚本发起后调 read 脚本 —— 回写有值 = 通道通；恒 null = 通道死（静默失败）。
  window.__sb_tr_probe = null;
  const r = {
    href: location.href,
    typeof_send: typeof window.__sbTrSend,
    typeof_webkit: typeof window.webkit,
    has_native_bridge: !!(window.webkit && window.webkit.messageHandlers
      && window.webkit.messageHandlers.shiftTranslate),
  };
  if (typeof window.__sbTrSend === 'function') {
    try {
      // callback 名须匹配后端的 valid_callback（[A-Za-z0-9_]{1,64}）→ __sb_tr_probe 合规
      window.__sbTrSend('/text', { text: 'hello' }, '__sb_tr_probe');
      r.fired = true;
    } catch (e) { r.fired = false; r.err = String(e).slice(0, 140); }
  } else {
    r.fired = false;
    r.err = 'no __sbTrSend → 隐私项/CONTEXT_TRANSLATE_JS 未进包';
  }
  return r;
})()

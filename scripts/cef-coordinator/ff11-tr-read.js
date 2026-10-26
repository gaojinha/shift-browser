(() => {
  // F-CEF-11 翻译通道端到端·取结果（须在 ff11-tr-fire.js 之后 4~20s 内调）
  // 后端回写形式：eval `window.NAME = {text:...} | {error:...}`（见 translate.rs write_translate_result）
  let v;
  try { v = JSON.stringify(window.__sb_tr_probe); } catch (e) { v = 'ERR:' + e.message; }
  return {
    href: location.href,
    probe_type: typeof window.__sb_tr_probe,       // 'object'=回写过 / 'object 且 null'=没回写
    probe_is_null: window.__sb_tr_probe === null,
    probe_undefined: typeof window.__sb_tr_probe === 'undefined',
    probe_val: (v === undefined ? 'undefined' : String(v).slice(0, 240)),
    // 结论口径写死在这，免得下次又靠人脑回忆
    verdict: (typeof window.__sb_tr_probe === 'undefined')
      ? '发起脚本没跑（fire 未执行或页面被换）'
      : (window.__sb_tr_probe === null
          ? '通道死：请求发出但宿主从未回写 → CEF 下翻译静默不可用'
          : '通道通：宿主已回写（再看 probe_val 是 text 还是 error）'),
  };
})()

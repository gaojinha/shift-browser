(() => {
  // F-CEF-11 验收探针：验「扩展/隐私/广告拦截脚本是否真进了 CEF 页面」
  // 判据分三层：① 通道标记 ② 前缀变量（wl_prefix 复用没重写）③ 脚本自身留下的特征 + CSS 是否真挂上
  const has = (k) => Object.prototype.hasOwnProperty.call(window, k);
  const brief = (v) => {
    try {
      if (v === undefined) return null;
      if (v === null) return 'null';
      if (typeof v === 'object') return JSON.stringify(v).slice(0, 160);
      return String(v).slice(0, 160);
    } catch (e) { return 'ERR:' + e.message; }
  };
  const styles = Array.from(document.querySelectorAll('style'));
  return {
    href: location.href,
    readyState: document.readyState,
    body_now: !!document.body,
    doc_root_now: !!document.documentElement,

    // ① 注入通道本身（browser::inject_marker 的第一行）
    inject_ok: window.__SHIFT_INJECT_OK__ === true,

    // ② wl_prefix 的 7 个前缀变量（派单点名要求一起注入、别重写）
    prefix_vars: {
      PRIVACY_WHITELIST: has('__SHIFT_PRIVACY_WHITELIST__'),
      TRANSLATE_TIMEOUT: has('__SHIFT_TRANSLATE_TIMEOUT__'),
      FP_MODE: has('__SHIFT_FP_MODE__'),
      SESSION_KEY: has('__SHIFT_SESSION_KEY__'),
      UA_BRANDS: has('__SHIFT_UA_BRANDS__'),
      SITE_OVERRIDES: has('__SHIFT_SITE_OVERRIDES__'),
      GPC_ON: has('__SHIFT_GPC_ON__')
    },

    // ③ 隐私脚本（privacy.js）自己留的特征
    privacy_marks: {
      patched: has('__shiftPatched'),
      features: brief(window.__shiftPrivacyFeatures),
      installed: has('__shiftPrivacyInstalled'),
      noise_on: has('__shiftNoiseOn'),
      fp_mode_val: brief(window.__shiftFpMode),
      brands_val: brief(window.__shiftBrands),
      origin_seed: has('__shiftOriginSeed')
    },

    // 品牌表不能被 compat/隐私覆盖坏（F-CEF-08 的成果要守住）
    ua_brands: (navigator.userAgentData && navigator.userAgentData.brands) || null,
    ua_mobile: navigator.userAgentData ? navigator.userAgentData.mobile : null,

    // GPC：CEF 构建应为 undefined（F-CEF-07 已定的口径）
    gpc_prop: (navigator.globalPrivacyControl === undefined) ? 'undefined' : brief(navigator.globalPrivacyControl),

    // ★ 本轮最关键的取证：CSS 只能靠 JS 挂 head/documentElement，
    //   document_start 下若两者皆 null，guarded() 的空 catch 会把失败静默吞掉
    style_count: styles.length,
    style_len_head: styles.map(s => (s.textContent || '').length).slice(0, 12),
    style_snippets: styles.map(s => (s.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 70)).slice(0, 6),

    // WebMCP polyfill 与 compat 的痕迹（有则说明那两段也进来了）
    webmcp_marks: { model_context: !!(window.modelContext || navigator.modelContext), has_mc_prop: 'modelContext' in (navigator || {}) },

    // adblock 的 DOM 观察器：document_start 时 body 为 null 则不挂（已知，非 CEF 新增）
    adblock_observer_note: '见 adblock.js:48 if(document.body)',

    // 用户脚本 / 扩展在本机的样本量（决定这次验收覆盖到哪一层）
    note_sample_zero: '本机 extensions 表 0 行、userscripts 0 行 → 这两条链路本轮无法行为级取证'
  };
})()

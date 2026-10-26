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
      // ⚠️ 名字一律从代码原文抄，别手敲：12:43 那轮我把这项写成
      //    '__SHIFT_TRANSLATE_TIMEOUT__'（多两个尾下划线），害得误报"变量缺失"，
      //    还顺着假理由推出"wl_prefix 整段没执行"。真实名见 adblock.rs:157：
      //    window.__SHIFT_TRANSLATE_TIMEOUT = {};   ← 无尾下划线
      PRIVACY_WHITELIST: has('__SHIFT_PRIVACY_WHITELIST__'),
      TRANSLATE_TIMEOUT: has('__SHIFT_TRANSLATE_TIMEOUT'),
      // 本机 settings 里 translate_timeout=120（在 adblock.rs 的 5..=120 合法区间内）
      // → 真机该读到 120；读到 15 说明取的是默认值（设置没传下来），读不到说明前缀没执行
      TRANSLATE_TIMEOUT_VAL: brief(window.__SHIFT_TRANSLATE_TIMEOUT),
      FP_MODE: has('__SHIFT_FP_MODE__'),
      SESSION_KEY: has('__SHIFT_SESSION_KEY__'),
      UA_BRANDS: has('__SHIFT_UA_BRANDS__'),
      SITE_OVERRIDES: has('__SHIFT_SITE_OVERRIDES__'),
      GPC_ON: has('__SHIFT_GPC_ON__')
    },

    // ③ 隐私脚本（src-tauri/resources/js/privacy.js）自己留的特征
    //    ⚠️ 12:43 那轮本块有两项是【探针自己的错】，不是页面缺陷，别再照字面读：
    //    (a) 旧 `patched: has('__shiftPatched')` → false。**假阴性**：`__shiftPatched`
    //        从不挂 window，它是 WebRTC 防护的幂等哨兵，挂在构造函数上
    //        （privacy.js:233 `if (OrigRTC && !OrigRTC.__shiftPatched)`）。
    //        真判据在下面 rtc_webrtc_patched。
    //    (b) 旧 `brands_val: brief(window.__shiftBrands)` → null。**假阴性**：`__shiftBrands`
    //        是函数内局部变量（privacy.js:337 `var __shiftBrands = null;`），
    //        页面上永远读不到。品牌表真值在 __SHIFT_UA_BRANDS__（注入侧）
    //        和 navigator.userAgentData（运行时侧），拆成下面两项分别取。
    privacy_marks: {
      features: brief(window.__shiftPrivacyFeatures),
      installed: has('__shiftPrivacyInstalled'),      // privacy.js:4 window 上，有效
      noise_on: has('__shiftNoiseOn'),                // privacy.js:43/92，有效
      fp_mode_val: brief(window.__shiftFpMode),       // privacy.js:101，有效
      origin_seed: has('__shiftOriginSeed'),          // privacy.js:78，有效
      // 修正后的 WebRTC 判据：哨兵挂构造函数上
      rtc_webrtc_patched: !!(window.RTCPeerConnection && window.RTCPeerConnection.__shiftPatched),
      rtc_ctor_changed: window.RTCPeerConnection !== window.__shiftOrigRTC,
      // 品牌表：注入侧 vs 运行时侧必须分开看，两者不一致才是真问题
      brands_injected: brief(window.__SHIFT_UA_BRANDS__),
      brands_runtime: (function () {
        try { return brief(window.navigator.userAgentData && window.navigator.userAgentData.brands); }
        catch (e) { return 'ERR:' + e.message; }
      })()
    },

    // ★ 翻译桥（context_translate.js）。12:43 我拿错符号名（window.__shiftTranslate
    //   根本不存在）误判"桥没装"。真实标志与通道见 context_translate.js:13/17/46：
    //     window.__sbTrSend        —— 发送函数（双通道：webkit 原生桥 → 回退 shift:// Image）
    //     window.__shiftTranslateMenu —— 幂等标志（http/https 页才置位）
    //   主通道依赖 window.webkit.messageHandlers.shiftTranslate —— WebKit 专属，CEF 无；
    //   回退通道 shift://__translate__ 是 Tauri protocol handler（受站点 img-src CSP 拦，
    //   2026-09-21 正因如此才改走原生桥），且 CEF 侧未见注册 shift:// handler。
    //   判据：三个符号都在 + webkit 不存在 ⇒ 脚本装了、通道死了（不是"没注入"）。
    translate_bridge: {
      sb_tr_send: has('__sbTrSend'),
      menu_flag: has('__shiftTranslateMenu'),
      typeof_webkit: typeof window.webkit,                    // CEF 预期 'undefined'
      has_msg_handlers: !!(window.webkit && window.webkit.messageHandlers
        && window.webkit.messageHandlers.shiftTranslate),     // 主通道是否可用
      wpt_flag: has('__shift_wpt_active')                     // 整页翻译脚本的标记
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

    // 用户脚本 / 扩展样本量（决定覆盖到哪一层）。
    // 12:33 我查的是 app.db —— 那是 0 表的空壳，结论凑巧对但地基错。
    // 13:12 用真库复核：~/.local/share/com.shiftbrowser.app/shift-browser.db（24 表）
    //   → extensions 0 行、userscripts 0 行（settings 43 行）
    note_sample_zero: '真库 shift-browser.db 复核：extensions 0 行 / userscripts 0 行 → 这两条链路无样本，本轮不可行为级取证（需先造样本）',
  };
})()

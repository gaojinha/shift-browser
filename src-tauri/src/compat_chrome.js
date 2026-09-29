// Chrome 指纹补齐（2026-09-29 Turnstile 确诊缺口）
//
// 背景：特性矩阵对照（真 Chrome vs 本浏览器）缺三件 —— navigator.userAgentData、
// navigator.deviceMemory、window.scheduler。验证墙类站点拿它们打分，缺件直接
// 扣分。astes 值照桌面 Chrome抄（Linux/x86_64/142 系，与 UA 串一致）。
//
// 注入时机必须 document_start 且全帧（含验证 iframe），网站脚本 feature-detect
// 之前就要在。各值照桌面 Chrome 抄（Linux/x86_64/142 系，与 UA 串一致）。
// 与 privacy 加噪不冲突：这里只"补没有的"，不改已有的 canvas 指纹。
// 幂等：重复注入直接 return。开关见设置 compat_chrome_fp（默认开），关即回原生。
(function () {
  if (window.__sbCompatFP) return;
  window.__sbCompatFP = true;

  function def(obj, prop, value) {
    try {
      Object.defineProperty(obj, prop, { value: value, configurable: true, writable: false });
      return true;
    } catch (e) {
      return false;
    }
  }

  try {
    var nav = window.navigator;
    // ---- userAgentData（Chromium 系标准形状，desktop Linux）----
    if (nav && !nav.userAgentData) {
      var brands = [
        { brand: 'Chromium', version: '142' },
        { brand: 'Google Chrome', version: '142' },
        { brand: 'Not-A.Brand', version: '99' },
      ];
      var fullList = [
        { brand: 'Chromium', version: '142.0.0.0' },
        { brand: 'Google Chrome', version: '142.0.0.0' },
        { brand: 'Not-A.Brand', version: '99.0.0.0' },
      ];
      var uad = {
        brands: brands,
        mobile: false,
        platform: 'Linux',
        getHighEntropyValues: function (hints) {
          var out = {
            brands: brands,
            mobile: false,
            platform: 'Linux',
            architecture: 'x86',
            bitness: '64',
            model: '',
            uaFullVersion: '142.0.0.0',
            fullVersionList: fullList,
          };
          // 只回要的子集也合法；这里全量回，省得站点二次追问
          void hints;
          return Promise.resolve(out);
        },
      };
      // 桌面端 toJSON 按规范返回 brands/mobile/platform
      try {
        uad.toJSON = function () {
          return { brands: brands, mobile: false, platform: 'Linux' };
        };
      } catch (e) {}
      def(nav, 'userAgentData', uad);
    }
    // ---- deviceMemory（桌面 Chrome 上限 8GB，按规范取值为 8）----
    if (nav && nav.deviceMemory === undefined) {
      def(nav, 'deviceMemory', 8);
    }
  } catch (e) {}

  try {
    // ---- scheduler（最小可用桩：yield/postTask 走微任务，语义不骗人）----
    if (!window.scheduler) {
      var scheduler = {
        yield: function () {
          return Promise.resolve();
        },
        postTask: function (cb) {
          return Promise.resolve().then(function () {
            return typeof cb === 'function' ? cb() : undefined;
          });
        },
      };
      def(window, 'scheduler', scheduler);
    }
  } catch (e) {}
})();

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
    // ---- 平台推导（2026-09-29）：按站点 UA 覆盖动态对齐 ----
    // 注入全局不分站点，JS 按 location.host 查 __SHIFT_SITE_OVERRIDES__ 表；
    // 有覆盖则按覆盖 UA 推平台（Windows/macOS），否则 Linux。
    // 否则"UA 说 Windows、指纹说 Linux"被风控识破（B站 412 类）。
    var __plat = 'Linux', __arch = 'x86';
    try {
      var __ov = window.__SHIFT_SITE_OVERRIDES__ || {};
      var __h = (location.host || '').toLowerCase();
      var __ua = __ov[__h] || __ov[__h.replace(/^www\./, '')] || '';
      if (/windows/i.test(__ua)) { __plat = 'Windows'; __arch = 'x86'; }
      else if (/macintosh|mac os/i.test(__ua)) { __plat = 'macOS'; __arch = 'arm'; }
    } catch (e) {}
    // ---- userAgentData（Chromium 系标准形状）----
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
        platform: __plat,
        getHighEntropyValues: function (hints) {
          var out = {
            brands: brands,
            mobile: false,
            platform: __plat,
            architecture: __arch,
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
          return { brands: brands, mobile: false, platform: __plat };
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
    // ---- scheduler（2026-09-29 禁用：最小桩语义不完整，GitHub React 重度依赖
    // 任务调度，桩导致主线程卡死 ANR。userAgentData/deviceMemory 保留。
    // 如需恢复，需实现完整 TaskController/signal/优先级语义后再开。----
    void 0;
  } catch (e) {}
})();

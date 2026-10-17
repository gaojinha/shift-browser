
(function () {
  if (window.__shiftPrivacyInstalled) return;
  window.__shiftPrivacyInstalled = true;

  // ---- 0) 跨域子框架一律零修改（风控/验证码 iframe 兼容，2026-09-26 Upwork 事故）----
  // 本脚本经 AllFrames 注入，会跑进第三方跨域 iframe；而 Cloudflare Turnstile /
  // reCAPTCHA / hCaptcha 的挑战代码恰恰运行在这种 iframe 里，专门检测原生 API
  // 有没有被钩（Function.prototype.toString、原型链、跨帧一致性）。任何
  // defineProperty / 包装构造函数都会让安全检查直接判负——Upwork 的 Cloudflare
  // managed challenge（cf-mitigated: challenge，挑战跑在 challenges.cloudflare.com
  // 跨域 iframe）就因此永远过不去。跨域帧读 top.location 会抛 SecurityError，
  // 用它识别"跨域子框架"；同源子框架仍照常防护。
  var crossSubframe = (function () {
    try {
      if (window.top === window.self) return false; // 顶层框架
      void window.top.location;                    // 跨域时抛 SecurityError
      return false;                                // 同源子框架
    } catch (e) {
      return true;
    }
  })();
  if (crossSubframe) return;

  // ---- 0.5) 白名单 = 完全兼容模式（不再只是"跳过加噪"）----
  // 命中白名单的顶层/同源帧：不安装任何指纹伪装，连 GPC 也不设，整帧保持
  // 浏览器原生环境——风控站（Upwork 等 Cloudflare 站点）、在线设计/地图类加这里。
  // 注意：第三方挑战 iframe 由上面的跨域判定自动豁免，**不要**把
  // challenges.cloudflare.com 之类加进白名单（iframe 里 hostname 是对方域名，
  // 且挑战域名会变；真正要加的是 upwork.com 这种顶层站点）。
  var WL = window.__SHIFT_PRIVACY_WHITELIST__ || [];
  var WL_HIT = (function () {
    var h = String(location.hostname || '').toLowerCase();
    for (var i = 0; i < WL.length; i++) {
      var d = String(WL[i] || '').toLowerCase().replace(/^\*\./, '').replace(/^https?:\/\//, '').replace(/\/.*$/, '').trim();
      if (!d) continue;
      if (h === d || (h.length > d.length && h.slice(-(d.length + 1)) === '.' + d)) return true;
    }
    return false;
  })();
  if (WL_HIT) {
    try {
      window.__shiftNoiseOn = false;
      window.__shiftPrivacyBypass = true; // STRICT_PRIVACY_JS 见此标记也整体跳过
    } catch (e) {}
    return;
  }

  // ---- 1) GPC：全球隐私控制（W3C 规范信号，2027 起加州强制）----
  try {
    Object.defineProperty(navigator, 'globalPrivacyControl', {
      get: function () { return true; },
      configurable: true
    });
  } catch (e) {}

  // ---- 2) per-session + per-origin 确定性种子（FNV-1a）----
  // 种子 = f(会话随机 key, origin)：同会话同源稳定（不破坏功能）、
  // 跨会话（进程重启后 key 换新）或跨源即变（追踪失效）。对标 Brave farbling。
  // 会话 key 由 Rust 注入侧生成并在脚本前拼 window.__SHIFT_SESSION_KEY__。
  function originSeed() {
    var o = String(location.origin || location.href || '');
    var k = String(window.__SHIFT_SESSION_KEY__ || '');
    var s = k + '|' + o;
    var h = 2166136261;
    for (var i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return h >>> 0;
  }
  var SEED = originSeed();
  try { window.__shiftOriginSeed = SEED; } catch (e) {}
  // xorshift32 确定性伪随机：同源同会话稳定，换站点即变
  function rnd(i) {
    var x = (SEED + Math.imul(i, 2654435761)) >>> 0;
    x ^= x << 13; x >>>= 0;
    x ^= x >> 17;
    x ^= x << 5; x >>>= 0;
    return (x % 1000) / 1000;
  }

  // ---- 1.5) 加噪开关 ----
  // 走到这里说明既不是跨域 iframe（0 已 return）、也没命中白名单（0.5 已 return），
  // 故加噪恒开；保留 __shiftNoiseOn 标记供隐私自测读取。
  var NOISE_ON = true;
  try { window.__shiftNoiseOn = true; } catch (e) {}

  // ---- 1.6) 反指纹模式（阶段 2 · 反指纹 2.0）----
  // noise   = 加噪（默认）：同源稳定、跨源不同的确定性噪声
  // uniform = 统一化（Tor/Mullvad 派）：Canvas/WebGL/Audio 返回**固定值**，
  //           让该模式下所有使用者呈现一致指纹，而非各自"独一无二的噪声"
  // 注入时由 Rust 侧在脚本前拼 window.__SHIFT_FP_MODE__
  var FP_MODE = (window.__SHIFT_FP_MODE__ === 'uniform') ? 'uniform' : 'noise';
  var UNIFORM = (FP_MODE === 'uniform');
  try { window.__shiftFpMode = FP_MODE; } catch (e) {}

  // ---- 3) Canvas 加噪 / 统一化（读回路径 toDataURL / getImageData）----
  try {
    var origToDataURL = HTMLCanvasElement.prototype.toDataURL;
    HTMLCanvasElement.prototype.toDataURL = function () {
      if (!NOISE_ON) return origToDataURL.apply(this, arguments);
      // 统一化：返回一张固定白图（所有人一致），不暴露真实画布内容
      if (UNIFORM) {
        try {
          var fc = document.createElement('canvas');
          fc.width = this.width || 1;
          fc.height = this.height || 1;
          var fx = fc.getContext && fc.getContext('2d');
          if (fx) { fx.fillStyle = '#ffffff'; fx.fillRect(0, 0, fc.width, fc.height); }
          return origToDataURL.apply(fc, arguments);
        } catch (e) {}
        return origToDataURL.apply(this, arguments);
      }
      try {
        var ctx = this.getContext && this.getContext('2d');
        if (ctx && this.width > 0 && this.height > 0) {
          var w = Math.min(this.width, 12), h = Math.min(this.height, 12);
          var d = ctx.getImageData(0, 0, w, h);
          for (var i = 0; i < d.data.length; i += 4) {
            d.data[i] = (d.data[i] + (rnd(i) < 0.5 ? 0 : 1)) & 255;
            d.data[i + 1] = (d.data[i + 1] + (rnd(i + 1) < 0.5 ? 0 : 1)) & 255;
          }
          ctx.putImageData(d, 0, 0);
        }
      } catch (e) {}
      return origToDataURL.apply(this, arguments);
    };
    var origGetImageData = CanvasRenderingContext2D.prototype.getImageData;
    CanvasRenderingContext2D.prototype.getImageData = function () {
      var d = origGetImageData.apply(this, arguments);
      if (!NOISE_ON) return d;
      try {
        if (UNIFORM) {
          // 统一化：固定为不透明白色（所有人一致）
          for (var j = 0; j < d.data.length; j += 4) {
            d.data[j] = 255; d.data[j + 1] = 255; d.data[j + 2] = 255; d.data[j + 3] = 255;
          }
        } else {
          for (var i = 0; i < d.data.length; i += 4) {
            d.data[i] = (d.data[i] + (rnd(i) < 0.5 ? 0 : 1)) & 255;
          }
        }
      } catch (e) {}
      return d;
    };
  } catch (e) {}

  // ---- 4) WebGL 参数归一化（抹掉 GPU 型号/驱动指纹）----
  try {
    var gp = WebGLRenderingContext.prototype.getParameter;
    WebGLRenderingContext.prototype.getParameter = function (p) {
      // 白名单站点：不加噪、不归一化 GPU 信息（保留站点自己的渲染判断）
      if (!NOISE_ON) return gp.apply(this, arguments);
      // 37445 = UNMASKED_VENDOR_WEBGL, 37446 = UNMASKED_RENDERER_WEBGL
      if (p === 37445) return 'Google Inc.';
      if (p === 37446) return 'ANGLE (Generic Renderer)';
      return gp.apply(this, arguments);
    };
    if (window.WebGL2RenderingContext) {
      var gp2 = WebGL2RenderingContext.prototype.getParameter;
      WebGL2RenderingContext.prototype.getParameter = function (p) {
        if (!NOISE_ON) return gp2.apply(this, arguments);
        if (p === 37445) return 'Google Inc.';
        if (p === 37446) return 'ANGLE (Generic Renderer)';
        return gp2.apply(this, arguments);
      };
    }
  } catch (e) {}

  // ---- 5) AudioContext 加噪 / 统一化 ----
  try {
    if (window.AnalyserNode && AnalyserNode.prototype.getFloatFrequencyData) {
      var ofd = AnalyserNode.prototype.getFloatFrequencyData;
      AnalyserNode.prototype.getFloatFrequencyData = function (arr) {
        ofd.apply(this, arguments);
        if (!NOISE_ON) return;
        try {
          if (UNIFORM) {
            for (var u = 0; u < arr.length; u++) arr[u] = -100;
          } else {
            for (var i = 0; i < arr.length; i += 31) {
              arr[i] = arr[i] + (rnd(i) - 0.5) * 0.001;
            }
          }
        } catch (e) {}
      };
    }
    if (window.AudioBuffer && AudioBuffer.prototype.getChannelData) {
      var ocd = AudioBuffer.prototype.getChannelData;
      AudioBuffer.prototype.getChannelData = function (ch) {
        var d = ocd.apply(this, arguments);
        if (!NOISE_ON) return d;
        try {
          if (UNIFORM) {
            for (var u = 0; u < d.length; u++) d[u] = 0;
          } else {
            for (var i = 0; i < d.length; i += 997) {
              d[i] = d[i] + (rnd(i) - 0.5) * 1e-6;
            }
          }
        } catch (e) {}
        return d;
      };
    }
  } catch (e) {}

  // ---- 6) 硬件信息归一化（收敛到常见配置，避免成为稀有指纹）----
  try {
    Object.defineProperty(navigator, 'hardwareConcurrency', {
      get: function () { return 8; },
      configurable: true
    });
  } catch (e) {}
  try {
    if ('deviceMemory' in navigator) {
      Object.defineProperty(navigator, 'deviceMemory', {
        get: function () { return 8; },
        configurable: true
      });
    }
  } catch (e) {}

  // ---- 7) WebRTC 防泄露：丢弃 host 候选（内网 IP），保留公网候选 ----
  try {
    var OrigRTC = window.RTCPeerConnection || window.webkitRTCPeerConnection;
    if (OrigRTC && !OrigRTC.__shiftPatched) {
      var filterCandidate = function (ev) {
        var c = ev && ev.candidate && ev.candidate.candidate;
        // "typ host" = 本机/内网地址；丢弃即可阻止内网 IP 泄露
        return !(c && / typ host/.test(c));
      };
      var Patched = function (cfg, cons) {
        var pc = new OrigRTC(cfg, cons);
        var origAdd = pc.addEventListener.bind(pc);
        pc.addEventListener = function (type, fn, opts) {
          if (type === 'icecandidate' && typeof fn === 'function') {
            var wrapped = function (ev) {
              if (!filterCandidate(ev)) return; // 内网候选不交给页面
              return fn.apply(this, arguments);
            };
            return origAdd(type, wrapped, opts);
          }
          return origAdd(type, fn, opts);
        };
        try {
          Object.defineProperty(pc, 'onicecandidate', {
            configurable: true,
            get: function () { return this.__shiftOnIce || null; },
            set: function (fn) {
              var self = this;
              this.__shiftOnIce = fn;
              origAdd('icecandidate', function (ev) {
                if (!filterCandidate(ev)) return;
                if (typeof self.__shiftOnIce === 'function') self.__shiftOnIce(ev);
              });
            }
          });
        } catch (e) {}
        return pc;
      };
      Patched.prototype = OrigRTC.prototype;
      Patched.__shiftPatched = true;
      window.RTCPeerConnection = Patched;
    }
  } catch (e) {}

  // ---- 8) 字体探针防护：已移至严格档 ----
  // 事故记录（2026-09-18）：本项在标准档曾导致邮箱正文空白 —— 部分站点用
  // document.fonts.check 判断"字体是否就绪"来决定是否渲染内容，强制返回 false
  // 会被误判为字体未加载而卡住渲染。字体探针改为仅在严格档启用。
  // （实现见 STRICT_PRIVACY_JS 的字体探针段）

  // ---- 9) WebGPU 指纹防护（2026 新兴高熵信号）----
  // 引擎标记：WebKitGTK 无 WebGPU 实现（navigator.gpu 为空）时无补丁可装，
  // 自测凭 webgpuEngine 区分"引擎不支持"（skip）与"补丁未挂上"（fail）。
  var webgpuEngine = !!(navigator.gpu && navigator.gpu.requestAdapter);
  var webgpuPatched = false;
  try {
    if (webgpuEngine) {
      var oreq = navigator.gpu.requestAdapter.bind(navigator.gpu);
      navigator.gpu.requestAdapter = function () {
        return oreq.apply(null, arguments).then(function (adapter) {
          try {
            if (adapter && adapter.info) {
              return new Proxy(adapter, {
                get: function (t, k) {
                  if (k === 'info') {
                    return { vendor: 'google', architecture: 'generic',
                             device: 'generic', description: 'Generic GPU' };
                  }
                  var v = t[k];
                  return typeof v === 'function' ? v.bind(t) : v;
                }
              });
            }
            if (adapter && adapter.requestAdapterInfo) {
              var oinfo = adapter.requestAdapterInfo.bind(adapter);
              adapter.requestAdapterInfo = function () {
                return Promise.resolve({ vendor: 'google', architecture: 'generic',
                                         device: 'generic', description: 'Generic GPU' });
              };
            }
          } catch (e) {}
          return adapter;
        });
      };
      webgpuPatched = true;
    }
  } catch (e) {}

  // ---- 10) navigator 杂项归一化（plugins/mimeTypes/Client Hints/平台）----
  try {
    // 现代网站不再依赖 plugins/mimeTypes，但指纹脚本仍会读取
    Object.defineProperty(navigator, 'plugins', {
      get: function () { return { length: 0, item: function () { return null; },
                                  namedItem: function () { return null; },
                                  refresh: function () {} }; },
      configurable: true
    });
    Object.defineProperty(navigator, 'mimeTypes', {
      get: function () { return { length: 0, item: function () { return null; },
                                  namedItem: function () { return null; } }; },
      configurable: true
    });
  } catch (e) {}
  try {
    // Client Hints JS API：抹掉精确的平台版本/架构
    // 平台/brands 与站点 UA 对齐（2026-09-29）：按 location.host 查覆盖表，
    // 有覆盖则按覆盖 UA 推平台与 Chrome 主版本，否则 fallback 全局值/Linux
    var __shiftPlat = 'Linux';
    var __shiftBrands = null;
    try {
      var __ov = window.__SHIFT_SITE_OVERRIDES__ || {};
      var __h = (location.host || '').toLowerCase();
      var __ua = __ov[__h] || __ov[__h.replace(/^www\./, '')] || '';
      if (/windows/i.test(__ua)) { __shiftPlat = 'Windows'; }
      else if (/macintosh|mac os/i.test(__ua)) { __shiftPlat = 'macOS'; }
      var __m = /Chrome\/(\d+)/.exec(__ua);
      if (__m) {
        __shiftBrands = [{ brand: 'Not_A Brand', version: '24' },
                         { brand: 'Chromium', version: __m[1] },
                         { brand: 'Google Chrome', version: __m[1] }];
      }
    } catch (e) {}
    if (navigator.userAgentData) {
      var uad = navigator.userAgentData;
      try {
        Object.defineProperty(navigator, 'userAgentData', {
          get: function () {
            return {
              brands: __shiftBrands || window.__SHIFT_UA_BRANDS__ || uad.brands,
              mobile: false,
              platform: __shiftPlat,
              getHighEntropyValues: function () {
                return Promise.resolve({ platform: __shiftPlat, platformVersion: '',
                                         architecture: '', model: '', uaFullVersion: '' });
              },
              toJSON: function () { return { brands: __shiftBrands || window.__SHIFT_UA_BRANDS__ || uad.brands, mobile: false, platform: __shiftPlat }; }
            };
          },
          configurable: true
        });
      } catch (e) {}
    }
  } catch (e) {}

  // ---- 11) 屏幕色深归一化（不碰 width/height，避免影响响应式布局）----
  try {
    Object.defineProperty(screen, 'colorDepth', { get: function () { return 24; }, configurable: true });
    Object.defineProperty(screen, 'pixelDepth', { get: function () { return 24; }, configurable: true });
  } catch (e) {}

  // ---- 12) 电池 / 网络信息 / 设备枚举 归一化 ----
  try {
    if (navigator.getBattery) {
      navigator.getBattery = function () {
        return Promise.resolve({
          charging: true, chargingTime: 0, dischargingTime: Infinity, level: 1,
          onchargingchange: null, onchargingtimechange: null,
          ondischargingtimechange: null, onlevelchange: null,
          addEventListener: function () {}, removeEventListener: function () {}
        });
      };
    }
  } catch (e) {}
  try {
    if (navigator.connection) {
      Object.defineProperty(navigator.connection, 'type', { get: function () { return 'wifi'; }, configurable: true });
      Object.defineProperty(navigator.connection, 'effectiveType', { get: function () { return '4g'; }, configurable: true });
      Object.defineProperty(navigator.connection, 'downlink', { get: function () { return 10; }, configurable: true });
      Object.defineProperty(navigator.connection, 'rtt', { get: function () { return 50; }, configurable: true });
    }
  } catch (e) {}
  try {
    // 设备枚举：抹掉 label（真实设备名是强指纹），只保留 kind
    if (navigator.mediaDevices && navigator.mediaDevices.enumerateDevices) {
      var oenum = navigator.mediaDevices.enumerateDevices.bind(navigator.mediaDevices);
      navigator.mediaDevices.enumerateDevices = function () {
        return oenum.apply(null, arguments).then(function (list) {
          return list.map(function (d) {
            return { deviceId: '', groupId: '', kind: d.kind, label: '' };
          });
        });
      };
    }
  } catch (e) {}

  // ---- 13) 自测特性标记（供"隐私防护自测"核对实际生效项）----
  // 说明：canvas/webgl/audio 受白名单（NOISE_ON）控制；其余为常开归一化项。
  try {
    window.__shiftPrivacyFeatures = {
      gpc: true,
      canvas: NOISE_ON,
      webgl: NOISE_ON,
      audio: NOISE_ON,
      hardware: true,
      webrtc: true,
      // webgpu = 补丁真挂上；webgpu_engine = 引擎有无该接口（自测区分 skip/fail 用）
      webgpu: webgpuPatched,
      webgpu_engine: webgpuEngine,
      misc: true,
      fp_mode: FP_MODE
    };
  } catch (e) {}
})();

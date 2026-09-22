
(function () {
  if (window.__shiftPrivacyStrict) return;
  window.__shiftPrivacyStrict = true;

  // ---- 1) 移除高危设备 API（既是强指纹源，也是攻击面）----
  var risky = ['usb', 'bluetooth', 'serial', 'hid', 'credentials'];
  for (var i = 0; i < risky.length; i++) {
    var k = risky[i];
    if (k === 'credentials') continue; // 密码管理依赖 Credential Management，保留
    try {
      Object.defineProperty(navigator, k, {
        get: function () { return undefined; },
        configurable: true
      });
    } catch (e) {}
  }

  // ---- 2) Gamepad：枚举结果恒为空 ----
  try {
    if (navigator.getGamepads) {
      navigator.getGamepads = function () { return []; };
    }
  } catch (e) {}

  // ---- 3) 传感器 API：直接抹掉构造函数 ----
  try {
    ['Accelerometer', 'Gyroscope', 'Magnetometer', 'AmbientLightSensor',
     'AbsoluteOrientationSensor', 'RelativeOrientationSensor', 'LinearAccelerationSensor']
      .forEach(function (name) {
        try {
          if (window[name]) {
            Object.defineProperty(window, name, {
              get: function () { return undefined; },
              configurable: true
            });
          }
        } catch (e) {}
      });
  } catch (e) {}

  // ---- 4) 语音合成音色列表归一化（voices 列表是稳定指纹）----
  try {
    if (window.speechSynthesis) {
      var origGet = speechSynthesis.getVoices.bind(speechSynthesis);
      speechSynthesis.getVoices = function () {
        return [{
          name: 'Generic Voice', lang: 'zh-CN', localService: true,
          default: true, voiceURI: 'generic'
        }];
      };
    }
  } catch (e) {}

  // ---- 4.5) 字体探针防护（仅严格档；标准档曾导致邮箱正文空白，见事故记录）----
  try {
    if (document.fonts && document.fonts.check) {
      var ocheck = document.fonts.check.bind(document.fonts);
      var COMMON = ['Arial', 'Times New Roman', 'Courier New', 'Verdana', 'Helvetica',
                    'Georgia', 'Tahoma', 'Trebuchet MS', 'Segoe UI', 'Roboto',
                    'Microsoft YaHei', 'SimSun', 'PingFang SC', 'Noto Sans CJK SC'];
      document.fonts.check = function (font, text) {
        try {
          var m = /(\d+(?:\.\d+)?)(px|pt|em|rem)?\s+(.+)/.exec(String(font || ''));
          var fam = m ? m[3] : String(font || '');
          fam = fam.replace(/["']/g, '').split(',')[0].trim();
          if (fam && COMMON.indexOf(fam) === -1) return false;
        } catch (e) {}
        return ocheck.apply(null, arguments);
      };
    }
  } catch (e) {}

  // ---- 5) 高精度计时降级（抗时序侧信道指纹）----
  // 只把精度降到 2ms 级别：对绝大多数网页无感，但能削弱计时类指纹/攻击
  try {
    var origNow = performance.now.bind(performance);
    var last = 0;
    performance.now = function () {
      var t = origNow();
      var q = Math.floor(t / 2) * 2;
      if (q <= last) q = last + 2;
      last = q;
      return q;
    };
  } catch (e) {}

  // ---- 6) 时区/区域保持真实（中文用户体验优先），仅暴露受限的 locale 列表 ----
  try {
    if (navigator.languages && navigator.languages.length > 2) {
      Object.defineProperty(navigator, 'languages', {
        get: function () { return navigator.language ? [navigator.language, 'zh'] : ['zh-CN', 'zh']; },
        configurable: true
      });
    }
  } catch (e) {}

  // ---- 7) 严格档自测标记 ----
  try {
    window.__shiftPrivacyFeatures = window.__shiftPrivacyFeatures || {};
    window.__shiftPrivacyFeatures.strict = true;
  } catch (e) {}
})();

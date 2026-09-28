
(function () {
  try {
    var f = window.__shiftPrivacyFeatures || {};
    var gpc = false;
    try { gpc = (navigator.globalPrivacyControl === true); } catch (e) {}
    var rtcPatched = false;
    try { rtcPatched = !!(window.RTCPeerConnection && window.RTCPeerConnection.__shiftPatched); } catch (e) {}
    var hc = null;
    try { hc = navigator.hardwareConcurrency; } catch (e) {}
    return JSON.stringify({
      installed: !!window.__shiftPrivacyInstalled,
      strict: !!f.strict,
      noiseOn: !!window.__shiftNoiseOn,
      features: {
        gpc: gpc,
        canvas: !!f.canvas,
        webgl: !!f.webgl,
        audio: !!f.audio,
        hardware: !!f.hardware,
        webrtc: !!(f.webrtc || rtcPatched),
        webgpu: !!f.webgpu,
        // 引擎有无 WebGPU 接口（后端区分"引擎不支持→skip"与"补丁未挂上→fail"）
        webgpu_engine: !!f.webgpu_engine,
        misc: !!f.misc
      },
      hardwareConcurrency: hc,
      hostname: String(location.hostname || ''),
      protocol: String(location.protocol || '')
    });
  } catch (e) {
    return JSON.stringify({ error: String(e) });
  }
})()

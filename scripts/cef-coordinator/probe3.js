(async () => {
  const out = {};
  const c = document.createElement('canvas');
  const gl = c.getContext('webgl');
  const gl2 = c.getContext('webgl2');
  out.webgl = !!gl;
  out.webgl2 = !!gl2;
  if (gl) {
    const dbg = gl.getExtension('WEBGL_debug_renderer_info');
    out.UNMASKED_VENDOR = dbg ? gl.getParameter(dbg.UNMASKED_VENDOR_WEBGL) : null;
    out.UNMASKED_RENDERER = dbg ? gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL) : null;
    out.GL_VERSION = gl.getParameter(gl.VERSION);
    out.MAX_TEXTURE = gl.getParameter(gl.MAX_TEXTURE_SIZE);
    out.EXT_COUNT = (gl.getSupportedExtensions() || []).length;
  }
  out.nav_gpu = 'gpu' in navigator;
  out.userAgent = navigator.userAgent;
  const uad = navigator.userAgentData || null;
  out.uaData = uad ? { brands: uad.brands, mobile: uad.mobile, platform: uad.platform } : null;
  try {
    out.uad_high = uad ? await uad.getHighEntropyValues(
      ['architecture', 'bitness', 'model', 'platformVersion', 'fullVersionList']) : null;
  } catch (e) { out.uad_high = 'ERR:' + String(e).slice(0, 80); }
  out.gpc_defined = ('globalPrivacyControl' in navigator);
  out.gpc_value = out.gpc_defined ? navigator.globalPrivacyControl : '__NOT_DEFINED__';
  out.webdriver = navigator.webdriver;
  out.doNotTrack = navigator.doNotTrack;
  out.plugins = navigator.plugins.length;
  out.languages = navigator.languages;
  out.platform = navigator.platform;
  out.hardwareConcurrency = navigator.hardwareConcurrency;
  out.deviceMemory = navigator.deviceMemory;
  out.pdfViewerEnabled = navigator.pdfViewerEnabled;
  out.touchMax = navigator.maxTouchPoints;
  out.screen = { w: screen.width, h: screen.height, availW: screen.availWidth, availH: screen.availHeight, dpr: window.devicePixelRatio, depth: screen.colorDepth };
  out.chrome_type = typeof window.chrome;
  out.timezone = (() => { try { return new Date().getTimezoneOffset(); } catch (e) { return null; } })();
  return out;
})()

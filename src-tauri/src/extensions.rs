// 扩展管理（WebKitGTK UserContentManager 用户脚本/样式注入）
// WebKitGTK 不支持真正的 Chrome 扩展 API，MVP 方案：
// 扩展 = 目录（含 manifest.json + content.js/content.css），
// 注入到所有 http/https 页面（allow_list 限定，不污染 tauri://localhost 的 Vue UI）
use crate::db::extensions as ext_db;
use crate::db::Db;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

#[cfg(target_os = "linux")]
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
    UserStyleLevel, UserStyleSheet, WebViewExt,
};

/// 返回给前端的数据（含注入内容摘要，便于显示）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtensionInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub description: Option<String>,
    pub js_count: usize,
    pub css_count: usize,
    pub created_at: String,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content_scripts: Vec<ContentScript>,
}

#[derive(Deserialize)]
struct ContentScript {
    #[serde(default)]
    js: Vec<String>,
    #[serde(default)]
    css: Vec<String>,
}

/// 扩展目录解析结果：(name, version, description, 合并后的 js, 合并后的 css, js 文件数, css 文件数)
type ExtensionDirResult = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    usize,
    usize,
);

/// 读取扩展目录内容
fn read_extension_dir(path: &str) -> Result<ExtensionDirResult, String> {
    // path 可以是目录（含 manifest.json）或 manifest.json 文件本身
    let manifest_path = if path.ends_with("manifest.json") {
        path.to_string()
    } else {
        format!("{}/manifest.json", path)
    };
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 manifest.json 失败: {}", e))?;
    let manifest: Manifest = serde_json::from_str(&manifest_str)
        .map_err(|e| format!("解析 manifest.json 失败: {}", e))?;

    // 扩展目录 = manifest.json 所在目录
    let base_dir = match manifest_path.rfind('/') {
        Some(idx) => manifest_path[..idx].to_string(),
        None => ".".to_string(),
    };

    let mut js_sources: Vec<String> = Vec::new();
    let mut css_sources: Vec<String> = Vec::new();
    // 安全：manifest 里的脚本/样式路径必须解析后仍落在扩展目录内，
    // 防止恶意/被篡改的 manifest 声明 "../../../.ssh/id_rsa" 之类路径越界读任意文件。
    //
    // 两道防线：
    //  1) 词法：拒绝含 ".."（ParentDir）组件的路径；Path::starts_with 是纯词法比较、
    //     不解析 ".."，若只靠 starts_with，声明 "../../../home/gao/.ssh/id_rsa" 可绕过越界读取。
    //  2) 物理（symlink 兜底）：文件真实存在时做 canonicalize，解析符号链接后
    //     再校验「真实路径」仍落在「真实 base」内——防止扩展包在目录里埋一个
    //     symlink 指向 /home/gao/.ssh，再让 manifest 引用该链接名间接越界。
    //     文件不存在（如首次注入尚未生成）时退回到已通过词法校验的相对路径，
    //     因为相对路径 + 无 ".." + 以 base 开头，词法上已确定无法越界。
    let base = std::path::Path::new(&base_dir);
    // 真实 base：解析 base 自身可能的 symlink（防御扩展目录本身是链接指向外部的场景）
    let base_real = base.canonicalize().unwrap_or_else(|_| base.to_owned());
    for cs in &manifest.content_scripts {
        for js in &cs.js {
            let p = base.join(js);
            // 拒绝含 ".." 组件的路径
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("脚本路径含 ..（拒绝）: {}", js));
            }
            // 词法越界
            if !p.starts_with(base) {
                return Err(format!("脚本路径越界（拒绝）: {}", js));
            }
            // symlink 兜底：解析真实路径后再校验一次
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("脚本路径经符号链接越界（拒绝）: {}", js));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取脚本 {} 失败: {}", js, e))?;
            js_sources.push(content);
        }
        for css in &cs.css {
            let p = base.join(css);
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("样式路径含 ..（拒绝）: {}", css));
            }
            if !p.starts_with(base) {
                return Err(format!("样式路径越界（拒绝）: {}", css));
            }
            // symlink 兜底（与 js 分支保持一致）：解析真实路径后校验仍落在真实 base 内
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("样式路径经符号链接越界（拒绝）: {}", css));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取样式 {} 失败: {}", css, e))?;
            css_sources.push(content);
        }
    }

    let js = if js_sources.is_empty() {
        None
    } else {
        Some(js_sources.join("\n;\n"))
    };
    let css = if css_sources.is_empty() {
        None
    } else {
        Some(css_sources.join("\n"))
    };

    Ok((
        manifest.name,
        manifest.version,
        manifest.description,
        js,
        css,
        js_sources.len(),
        css_sources.len(),
    ))
}

/// 内置隐私防护脚本（P0：GPC + 反指纹随机化 + WebRTC 防泄露）
///
/// 设计要点（依据 Brave/Tor 的公开实践）：
/// - **随机化路线**：Canvas/WebGL/Audio 加入"同源稳定、跨源不同"的确定性噪声，
///   避免出现"每源都变"这种可被识别的加噪特征（UCSD IMC25 论文指出该识别手法）。
/// - **GPC**：W3C 规范除 Sec-GPC 请求头外，还有 navigator.globalPrivacyControl 属性；
///   WebKitGTK 未暴露请求头改写 API，故用 JS 属性表达（主流合规网站可识别）。
/// - **WebRTC**：过滤 icecandidate 中的 host 候选（内网 IP 泄露主因），保留公网候选，
///   不整体禁用 WebRTC（避免破坏视频会议类网站）。
const PRIVACY_JS: &str = r#"
(function () {
  if (window.__shiftPrivacyInstalled) return;
  window.__shiftPrivacyInstalled = true;

  // ---- 1) GPC：全球隐私控制（W3C 规范信号，2027 起加州强制）----
  try {
    Object.defineProperty(navigator, 'globalPrivacyControl', {
      get: function () { return true; },
      configurable: true
    });
  } catch (e) {}

  // ---- 2) per-origin 确定性种子（FNV-1a 哈希 origin）----
  function originSeed() {
    var o = String(location.origin || location.href || '');
    var h = 2166136261;
    for (var i = 0; i < o.length; i++) {
      h ^= o.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return h >>> 0;
  }
  var SEED = originSeed();
  // xorshift32 确定性伪随机：同源同会话稳定，换站点即变
  function rnd(i) {
    var x = (SEED + Math.imul(i, 2654435761)) >>> 0;
    x ^= x << 13; x >>>= 0;
    x ^= x >> 17;
    x ^= x << 5; x >>>= 0;
    return (x % 1000) / 1000;
  }

  // ---- 3) Canvas 加噪（读回路径 toDataURL / toBlob / getImageData）----
  try {
    var origToDataURL = HTMLCanvasElement.prototype.toDataURL;
    HTMLCanvasElement.prototype.toDataURL = function () {
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
      try {
        for (var i = 0; i < d.data.length; i += 4) {
          d.data[i] = (d.data[i] + (rnd(i) < 0.5 ? 0 : 1)) & 255;
        }
      } catch (e) {}
      return d;
    };
  } catch (e) {}

  // ---- 4) WebGL 参数归一化（抹掉 GPU 型号/驱动指纹）----
  try {
    var gp = WebGLRenderingContext.prototype.getParameter;
    WebGLRenderingContext.prototype.getParameter = function (p) {
      // 37445 = UNMASKED_VENDOR_WEBGL, 37446 = UNMASKED_RENDERER_WEBGL
      if (p === 37445) return 'Google Inc.';
      if (p === 37446) return 'ANGLE (Generic Renderer)';
      return gp.apply(this, arguments);
    };
    if (window.WebGL2RenderingContext) {
      var gp2 = WebGL2RenderingContext.prototype.getParameter;
      WebGL2RenderingContext.prototype.getParameter = function (p) {
        if (p === 37445) return 'Google Inc.';
        if (p === 37446) return 'ANGLE (Generic Renderer)';
        return gp2.apply(this, arguments);
      };
    }
  } catch (e) {}

  // ---- 5) AudioContext 加噪 ----
  try {
    if (window.AnalyserNode && AnalyserNode.prototype.getFloatFrequencyData) {
      var ofd = AnalyserNode.prototype.getFloatFrequencyData;
      AnalyserNode.prototype.getFloatFrequencyData = function (arr) {
        ofd.apply(this, arguments);
        try {
          for (var i = 0; i < arr.length; i += 31) {
            arr[i] = arr[i] + (rnd(i) - 0.5) * 0.001;
          }
        } catch (e) {}
      };
    }
    if (window.AudioBuffer && AudioBuffer.prototype.getChannelData) {
      var ocd = AudioBuffer.prototype.getChannelData;
      AudioBuffer.prototype.getChannelData = function (ch) {
        var d = ocd.apply(this, arguments);
        try {
          for (var i = 0; i < d.length; i += 997) {
            d[i] = d[i] + (rnd(i) - 0.5) * 1e-6;
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
})();
"#;

/// 在主线程把所有 child webview 的 UserContentManager 清空并注入扩展
/// UserContentManager 是 webview 级（非 WebContext 级），必须逐个 webview 注入
fn inject_extensions(app: &AppHandle, items: Vec<(Option<String>, Option<String>)>) {
    // 注意：主窗口是 Window 类型（tabs.rs 用 get_window("main")），
    // get_webview_window("main") 会返回 None 导致注入被跳过
    let Some(window) = app.get_window("main") else {
        log::info!("[extensions] 主窗口不可用，跳过注入");
        return;
    };
    let webviews = window.webviews();
    log::info!("[extensions] 注入到 {} 个 webview", webviews.len());
    let items_owned = items.clone();
    let _ = app.run_on_main_thread(move || {
        // 只注入 child 标签页（已跳过主 webview），child 都是 http/https 外部网页，
        // allow_list 留空（全部注入）避免 WebKitGTK 对 "http://*" 通配符格式不识别导致不注入
        let allow_list: &[&str] = &[];
        for wv in &webviews {
            // 跳过主 webview（label="main"，是 Vue UI），只注入 child 标签页（label="tab-*"）
            // remove_all_scripts 会误删 Tauri 主 webview 的 UserScript（Vue UI 桥接），导致界面空白
            if wv.label() == "main" {
                continue;
            }
            let items_clone = items_owned.clone();
            let _ = wv.with_webview(move |pwv| {
                let webview = pwv.inner();
                let Some(manager) = webview.user_content_manager() else {
                    return;
                };
                manager.remove_all_scripts();
                manager.remove_all_style_sheets();
                for (js, css) in &items_clone {
                    if let Some(js) = js {
                        let script = UserScript::new(
                            js,
                            UserContentInjectedFrames::AllFrames,
                            UserScriptInjectionTime::End,
                            allow_list,
                            &[],
                        );
                        manager.add_script(&script);
                    }
                    if let Some(css) = css {
                        let sheet = UserStyleSheet::new(
                            css,
                            UserContentInjectedFrames::AllFrames,
                            UserStyleLevel::User,
                            allow_list,
                            &[],
                        );
                        manager.add_style_sheet(&sheet);
                    }
                }
            });
        }
        log::info!("[extensions] 注入完成: {} 个扩展", items.len());
    });
}

/// 内置广告拦截 CSS 规则（精简版，常见广告选择器）
/// 设置 adblock_enabled=true 时自动注入，优先级低于用户扩展（扩展后注入）
const ADBLOCK_CSS: &str = r#"
/* ShiftBrowser 内置广告拦截（精简规则） */
#ad, #ads, .ad, .ads, .advert, .advertisement, .ad-banner, .ad-container,
.ad-slot, .ad-wrapper, .adsbygoogle, .banner-ad, .sponsored, .sponsor,
.promo, .promotion, .google-ads, .ggads, .adv, .advertise, .ad-box,
[class*="ad-banner"], [class*="ad-container"], [id*="ad-banner"],
iframe[src*="ad."], iframe[src*="doubleclick"], iframe[src*="googlead"],
img[src*="ad."], img[src*="advert"], img[src*="doubleclick"],
ins.adsbygoogle, .ad-placeholder, .advertisement-banner,
[class*="popup-ad"], [id*="popup-ad"], .ad-modal, .ad-overlay,
/* 门户/新浪类广告 */
#ad_right, .ad-left, .ad-top, .ad-bottom, .adbox, .ad-box, .ad_box,
.adsbox, .ggbox, #sinaads, .sinaads, .ads_global, .ad_global,
[class*="advert"], [id*="advert"], [class*="sinaads"], [id*="sinaads"],
[class*="gg_"], [class*="_gg"], [class*="banner"], [id*="banner"],
iframe[src*="ad"], iframe[src*="ads"], iframe[src*="advert"], iframe[src*="sinaads"],
iframe[src*="googlesyndication"], iframe[src*="amazon-adsystem"],
/* 对联/悬浮广告（页面左右两侧） */
#leftCouplet, #rightCouplet, .leftCouplet, .rightCouplet, .couplet, #couplet,
.floatAd, .float-ad, .floatAdLeft, .floatAdRight, .leftFloat, .rightFloat,
#floatAd, .sideAd, .side-ad, .sidebar-ad, .leftAd, .rightAd, .left-ad, .right-ad,
.left_float, .right_float, .float_left, .float_right, .floatL, .floatR,
[class*="couplet"], [id*="couplet"], [class*="floatAd"], [class*="float-ad"],
[class*="float_"], [id*="float_"], [class*="sideAd"], [class*="side-ad"],
/* 常见广告尺寸规格 */
.ad-300x250, .ad-728x90, .ad-160x600, .ad-970x90, .ad-970x250,
.ad-336x280, .ad-300x600, .ad-320x50, .ad-250x250, .ad-468x60,
/* 广告容器/单元 */
.adsense, .ad-sense, .advert-slot, .ad-slot-1, .ad-slot-2, .ad-slot,
.ad-unit, .ad-unit-1, .ad-unit-2, .ad-inner, .ad-outer, .ad-wrap,
.ad-content, .ad-item, .ad-list, .ad-grid, .ad-banner-container,
/* 内容推荐广告 */
.taboola, .outbrain, .revcontent, .content-ad, .recommend-ad,
/* 电商联盟/点击追踪（不拦 tmall.com 本体链接，避免购物页全灭） */
a[href*="uland.taobao"], a[href*="click.tb"], a[href*="s.click.taobao"],
a[href*="section=ad"], a[href*="ad_id="],
.tb-ads, .tm-ads, .shop-recommend-ad, .trade-ad, .tb-shop-ad, .shop-ad,
.jd-ad, .jd-ads, .deal-ad, .pinduoduo-ad, .pdd-ad, .pdd-ads, .vip-ad,
/* 内容平台广告（知乎/哔哩/豆瓣/小红书/微博） */
.Pc-word-ad, .Pc-mission-ad, .QuestionAd, .AnswerAd, .Topstory-ad,
.bili-rich-ad, .video-card-ad, .bili-video-card__info--ad, .feed-ad,
.douban-ad, .douban-advert, .xiaohongshu-ad, .xhs-ad,
.weibo-ad, .weibo-ads, .topic-ad, .card-ad, .recommend-card-ad,
/* 视频平台广告（优酷/爱奇艺/腾讯视频/芒果TV） */
.yk-player-ad, .yk-vip-ad, .yk-poster-ad, .youku-ad, .youku-ads,
.iqiyi-player-ad, .iqiyi-vip-ad, .iqiyi-poster-ad, .iqiyi-ad, .iqiyi-ads,
.tenvideo-player-ad, .qq-video-ad, .video-vip-ad,
.mgtv-player-ad, .mgtv-ad, .mgtv-vip-ad, .player-vip-ad, .preroll-ad, .postroll-ad,
/* 弹窗二跳/下载引导 */
.download-guide, .app-guide, .install-guide, .guide-modal, .guide-overlay,
.wx-follow, .wechat-guide, .qr-modal, .qr-overlay,
/* 工具栏/浏览器伴侣 */
.toolbar-ad, .companion-ad, .assistant-ad, .helper-ad,
/* 超高 z-index 遮罩（广告弹层常用 2147483647，不拦普通 fixed） */
div[style*="z-index:2147483647"], div[style*="z-index: 2147483647"],
/* 通用电商品类页推广 */
.quick-bar-ad, .flash-ad, .flashbuy-ad, .countdown-ad,
/* 音频/音乐 */
.kg-ad, .kugou-ad, .qqmusic-ad, .wangyiyun-ad, .music-ad,
/* 弹窗/遮罩 */
.modal-ad, .pop-ad, .layer-ad, .mask-ad, .dialog-ad, .overlay-ad,
/* 底部浮条 */
.bottom-ad, .footer-ad, .float-bottom, .bottom-banner, .footer-banner,
.bottom-bar-ad, .floating-ad, .sticky-ad,
/* 移动端/应用推广 */
.mobile-ad, .app-ad, .app-download, .download-ad, .app-promo,
/* 各门户广告 */
.qq_ad, .tx_ad, .netease_ad, .sohu_ad, .sogou_ad, .youku_ad, .tudou_ad,
#content_right, .ec_ads, .result-op.c-container, .c-container[data-click*="ad"]
{ display: none !important; }
"#;

/// 内置关键词（用户自定义关键词会追加在后面）
const BUILTIN_KEYWORDS: &[&str] = &[
    "关闭广告",
    "关闭推广",
    "收起广告",
    "不再显示",
    "跳过广告",
    "跳过推广",
    "头条报",
    "推广",
    "赞助",
    "下载APP",
    "下载 App",
    "下载客户端",
    "广告投放",
    "广告合作",
    "广告位",
    "广告服务",
];

/// 广告拦截 JS 模板。`__SB_KEYWORDS__` 在注入前替换成完整数组（内置 + 用户自定义）
const ADBLOCK_JS_TEMPLATE: &str = r#"
// ShiftBrowser 广告拦截：文本特征规则 + 动态监听
(function () {
  if (location.protocol !== 'http:' && location.protocol !== 'https:') return;
  var keywords = __SB_KEYWORDS__;
  var HIDDEN = 'data-sb-hidden';
  function hideByText() {
    var nodes = document.querySelectorAll('div, section, aside');
    // 性能保护：超大 DOM（几十万节点的重型 SPA）全扫会卡主线程，超过 3000 节点直接放弃文本扫描
    // （CSS [class*=ad]/[id*=ad] 规则仍会生效，只是文本特征词兜底失效，可接受）
    if (nodes.length > 3000) return;
    for (var i = 0; i < nodes.length; i++) {
      var el = nodes[i];
      if (el.getAttribute(HIDDEN)) continue;
      if (el.offsetWidth === 0 && el.offsetHeight === 0) continue;
      if (el.offsetHeight > 520) continue; // 跳过大块页面区域，避免误杀正文
      var text = el.textContent || '';
      if (text.length < 4 || text.length > 400) continue;
      for (var k = 0; k < keywords.length; k++) {
        if (keywords[k] && text.indexOf(keywords[k]) !== -1) {
          el.style.display = 'none';
          el.setAttribute(HIDDEN, '1');
          break;
        }
      }
    }
  }
  hideByText();
  var timer = null;
  var observer = new MutationObserver(function () {
    if (timer) return;
    timer = setTimeout(function () {
      timer = null;
      hideByText();
    }, 500);
  });
  if (document.body) {
    observer.observe(document.body, { childList: true, subtree: true });
  }
})();
"#;

fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// 拼装广告拦截 JS：内置关键词 + 用户自定义关键词
fn build_adblock_js(custom: &[String]) -> String {
    let mut all: Vec<String> = BUILTIN_KEYWORDS.iter().map(|s| (*s).to_string()).collect();
    for k in custom {
        let t = k.trim();
        if t.is_empty() {
            continue;
        }
        if !all.iter().any(|x| x == t) {
            all.push(t.to_string());
        }
    }
    let arr = all
        .iter()
        .map(|k| format!("'{}'", js_escape(k)))
        .collect::<Vec<_>>()
        .join(", ");
    ADBLOCK_JS_TEMPLATE.replace("__SB_KEYWORDS__", &format!("[{}]", arr))
}

async fn load_custom_keywords(db: &Db) -> Vec<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_keywords'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    match row {
        Some((v,)) => serde_json::from_str(&v).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// 读取所有启用扩展的注入内容 + 广告拦截（如开启）
async fn collect_enabled_items(db: &Db) -> Result<Vec<(Option<String>, Option<String>)>, String> {
    let exts = ext_db::list(db).await.map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for ext in exts {
        if !ext.enabled {
            continue;
        }
        match read_extension_dir(&ext.path) {
            Ok((_, _, _, js, css, _, _)) => items.push((js, css)),
            Err(e) => log::warn!("[extensions] 读取扩展 {} 失败: {}", ext.name, e),
        }
    }
    // 广告拦截（内置系统注入，追加在用户扩展后面）
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_enabled'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    let adblock_on = row.map(|r| r.0 == "true").unwrap_or(true); // 默认开启
    if adblock_on {
        let custom = load_custom_keywords(db).await;
        let js = build_adblock_js(&custom);
        log::info!(
            "[extensions] 广告拦截已开启（内置 {} 词 + 自定义 {} 词）",
            BUILTIN_KEYWORDS.len(),
            custom.len()
        );
        items.push((Some(js), Some(ADBLOCK_CSS.to_string())));
    }
    // 内置隐私防护（GPC + Canvas/WebGL/Audio 反指纹 + WebRTC 防泄露）——始终注入，
    // 放在最后确保不被用户扩展覆盖；注意 inject_extensions 会先 remove_all_scripts，
    // 所以脚本必须都在这份 items 里，不能单独注入。
    items.push((Some(PRIVACY_JS.to_string()), None));
    log::info!("[extensions] 隐私防护已注入（GPC + 反指纹 + WebRTC 候选过滤）");
    Ok(items)
}

/// 广告拦截开关（存设置 + 重新注入）
#[tauri::command]
pub async fn set_adblock_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_adblock_enabled: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_enabled', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 广告拦截当前状态（给前端初始化用）
#[tauri::command]
pub async fn get_adblock_enabled(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_enabled'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0 == "true").unwrap_or(true))
}

/// 读取用户自定义广告关键词（不含内置）
#[tauri::command]
pub async fn get_adblock_keywords(db: State<'_, Db>) -> Result<Vec<String>, String> {
    Ok(load_custom_keywords(&db).await)
}

/// 保存用户自定义广告关键词并重新注入（立刻对已开标签生效，刷新页面后看到效果）
#[tauri::command]
pub async fn set_adblock_keywords(
    db: State<'_, Db>,
    app: AppHandle,
    keywords: Vec<String>,
) -> Result<(), String> {
    let cleaned: Vec<String> = keywords
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] set_adblock_keywords: {} 个自定义词",
        cleaned.len()
    );
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_keywords', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 加载扩展（目录或 manifest.json 路径）
#[tauri::command]
pub async fn load_extension(
    db: State<'_, Db>,
    app: AppHandle,
    path: String,
) -> Result<ExtensionInfo, String> {
    log::info!("[extensions] load_extension: path={}", path);
    let (name, version, description, js, css, js_count, css_count) = read_extension_dir(&path)?;
    let id = ext_db::create(&db, &name, &path)
        .await
        .map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] 已加载扩展: id={} name={} js={} css={}",
        id,
        name,
        js.as_ref().map(|s| s.len()).unwrap_or(0),
        css.as_ref().map(|s| s.len()).unwrap_or(0)
    );
    // 注入（重新注入全部启用扩展，保持一致性）
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(ExtensionInfo {
        id,
        name,
        path,
        enabled: true,
        version,
        description,
        js_count,
        css_count,
        created_at: String::new(),
    })
}

/// 扩展列表
#[tauri::command]
pub async fn list_extensions(db: State<'_, Db>) -> Result<Vec<ExtensionInfo>, String> {
    let exts = ext_db::list(&db).await.map_err(|e| e.to_string())?;
    log::info!("[extensions] list_extensions: {} 条", exts.len());
    let mut infos = Vec::new();
    for ext in exts {
        let (version, description, js_count, css_count) = match read_extension_dir(&ext.path) {
            Ok((_, v, d, _, _, jc, cc)) => (v, d, jc, cc),
            Err(e) => {
                log::warn!("[extensions] 读取扩展 {} 失败: {}", ext.name, e);
                (None, None, 0, 0)
            }
        };
        infos.push(ExtensionInfo {
            id: ext.id,
            name: ext.name,
            path: ext.path,
            enabled: ext.enabled,
            version,
            description,
            js_count,
            css_count,
            created_at: ext.created_at,
        });
    }
    Ok(infos)
}

/// 启用/禁用扩展（重新注入）
#[tauri::command]
pub async fn set_extension_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[extensions] set_extension_enabled: id={} enabled={}",
        id,
        enabled
    );
    ext_db::set_enabled(&db, id, enabled)
        .await
        .map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 删除扩展（重新注入）
#[tauri::command]
pub async fn delete_extension(db: State<'_, Db>, app: AppHandle, id: i64) -> Result<(), String> {
    log::info!("[extensions] delete_extension: id={}", id);
    ext_db::delete(&db, id).await.map_err(|e| e.to_string())?;
    let items = collect_enabled_items(&db).await?;
    inject_extensions(&app, items);
    Ok(())
}

/// 启动时注入所有启用扩展（在 setup 阶段调用，db 已就绪）
pub fn inject_on_startup(app: &AppHandle, db: &Db) {
    let items = match tauri::async_runtime::block_on(collect_enabled_items(db)) {
        Ok(items) => items,
        Err(e) => {
            log::error!("[extensions] 启动注入失败: {}", e);
            return;
        }
    };
    if !items.is_empty() {
        log::info!("[extensions] 启动注入 {} 个启用扩展", items.len());
        inject_extensions(app, items);
    }
}

/// 重新注入全部启用扩展 + 广告拦截（新标签创建后调用，确保新 webview 也有扩展）
pub async fn re_inject(app: &AppHandle) {
    let db = app.state::<Db>();
    match collect_enabled_items(&db).await {
        Ok(items) => {
            log::info!("[extensions] re_inject: {} 个注入项", items.len());
            inject_extensions(app, items);
        }
        Err(e) => log::error!("[extensions] re_inject 失败: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_js_includes_builtin_and_custom() {
        let js = build_adblock_js(&["自定义词".to_string()]);
        assert!(js.contains("'关闭广告'"), "应包含内置词");
        assert!(js.contains("'自定义词'"), "应包含自定义词");
        assert!(!js.contains("__SB_KEYWORDS__"), "模板占位符应被替换");
    }

    #[test]
    fn build_js_escapes_quotes_and_dedups() {
        let js = build_adblock_js(&["it's".to_string(), "关闭广告".to_string()]);
        assert!(js.contains(r"'it\'s'"), "单引号应转义");
        assert_eq!(js.matches("'关闭广告'").count(), 1, "内置词不应重复");
    }
}

//! 广告拦截（P1-3 拆分 · 2026-09-22）
//!
//! 内置 CSS 规则 + 关键词 + JS 模板 + 构建函数。原在 extensions.rs。

use super::*;

/// 内置广告拦截 CSS 规则（精简版，常见广告选择器）
/// 设置 adblock_enabled=true 时自动注入，优先级低于用户扩展（扩展后注入）
const ADBLOCK_CSS: &str = r#"
/* ShiftBrowser 内置广告拦截（收紧版 2026-09-18）
   事故：宽泛选择器（[class*="banner"]、[id*="banner"]、iframe[src*="ad"]、
   img[src*="ad."]、[src*="advert"] 等）导致邮箱正文的 iframe 与图片被隐藏，
   表现为"邮件正文空白"。教训：src/class 的子串匹配会命中正常资源。
   原则：只保留高置信度广告标识，严禁 [src*="ad"] 这类会误伤正文的匹配。 */
ins.adsbygoogle, .adsbygoogle,
[id^="google_ads"], [id^="aswift"], [id^="div-gpt-ad"], [class^="div-gpt-ad"],
.google-ads, .ggads, #sinaads, .sinaads,
iframe[src*="doubleclick.net"], iframe[src*="googlesyndication.com"],
iframe[src*="amazon-adsystem.com"], iframe[src*="cpro.baidu.com"],
img[src*="doubleclick.net"],
.ad-banner, #ad-banner, .ad-container, .ad-slot,
[class*="popup-ad"], [id*="popup-ad"], .ad-modal, .ad-overlay,
[class*="popup-ad"], [id*="popup-ad"], .ad-modal, .ad-overlay,
/* 门户/新浪类广告 */
#ad_right, .ad-left, .ad-top, .ad-bottom, .adbox, .ad-box, .ad_box,
.adsbox, .ads_global, .ad_global,
[class*="sinaads"], [id*="sinaads"],
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
    // 2026-09-18 移除宽泛词「推广」：邮件正文含"产品推广"等字样时整块被隐藏，
    // 表现为"邮件正文空白"。文本关键词只保留明确的广告关闭按钮文案。
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
  if (window.__shiftAdblockInstalled) return;
  window.__shiftAdblockInstalled = true;
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
pub fn build_adblock_js(custom: &[String]) -> String {
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

pub async fn load_custom_keywords(db: &Db) -> Vec<String> {
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
pub async fn collect_enabled_items(
    db: &Db,
) -> Result<(Vec<(Option<String>, Option<String>)>, bool), String> {
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
    // 注入相关设置一次查询拿全（2026-09-19 优化：原先 adblock/privacy 三档/白名单/
    // 自定义关键词/翻译超时 6 次串行查询合并为 1 次）
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE key IN \
         ('adblock_enabled','privacy_shield_level','privacy_shield','privacy_whitelist',\
          'adblock_keywords','translate_timeout','privacy_fp_mode')",
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut kv: std::collections::HashMap<String, String> = rows.into_iter().collect();
    let mut take = |k: &str| kv.remove(k);

    // 广告拦截（内置系统注入，追加在用户扩展后面）
    let adblock_on = take("adblock_enabled").map(|v| v == "true").unwrap_or(true); // 默认开启
                                                                                   // 同步给引擎层内容过滤器（网络层拦截）：开关变化时立即生效
    crate::engine::content_filter::set_enabled(adblock_on);
    if adblock_on {
        let custom: Vec<String> = take("adblock_keywords")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let js = build_adblock_js(&custom);
        log::info!(
            "[extensions] 广告拦截已开启（内置 {} 词 + 自定义 {} 词）",
            BUILTIN_KEYWORDS.len(),
            custom.len()
        );
        items.push((Some(js), Some(ADBLOCK_CSS.to_string())));
        ADBLOCK_INJECTIONS.fetch_add(1, Ordering::Relaxed);
        // 引擎层网络拦截规则：内置规则 + 自定义关键词（规则变化时会触发重新编译）
        let rules = crate::engine::content_filter::build_rules_with_keywords(&custom);
        crate::engine::content_filter::remember_rules(&rules);
    }
    // 内置隐私防护（三档）：off / standard / strict
    // 兼容早期 privacy_shield='false'（等价 off）
    let level = match take("privacy_shield_level") {
        Some(l) => l,
        None => match take("privacy_shield").as_deref() {
            Some("false") => "off".to_string(),
            _ => "standard".to_string(), // 默认标准档
        },
    };
    // 加噪白名单：命中域名的站点跳过"加噪类"防护（保留 GPC/WebRTC/参数清洗等）
    let whitelist: Vec<String> = take("privacy_whitelist")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    // 翻译超时（与 translate::current_timeout 同规则）
    let timeout: u64 = take("translate_timeout")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 5 && *v <= 120)
        .unwrap_or(15);
    // 反指纹模式（阶段 2）：noise（加噪，默认）/ uniform（统一化）
    let fp_mode = normalize_fp_mode(take("privacy_fp_mode").as_deref());
    let wl_prefix = format!(
        "window.__SHIFT_PRIVACY_WHITELIST__ = {};\nwindow.__SHIFT_TRANSLATE_TIMEOUT = {};\nwindow.__SHIFT_FP_MODE__ = {};\nwindow.__SHIFT_UA_BRANDS__ = {};\n",
        serde_json::to_string(&whitelist).unwrap_or_else(|_| "[]".to_string()),
        timeout,
        serde_json::to_string(fp_mode).unwrap_or_else(|_| "\"noise\"".to_string()),
        // Client Hints brands 与 UA 版本对齐（方案 B）：否则"UA 说 Chrome、brands 无 Chrome"被识破
        crate::useragent::brands_json(&crate::useragent::ua_for(""))
    );
    match level.as_str() {
        "off" => log::info!("[extensions] 隐私防护已关闭（off）"),
        "strict" => {
            items.push((
                Some(format!(
                    "{}{}\n{}\n{}",
                    wl_prefix, PRIVACY_JS, STRICT_PRIVACY_JS, CONTEXT_TRANSLATE_JS
                )),
                None,
            ));
            PRIVACY_INJECTIONS.fetch_add(1, Ordering::Relaxed);
            log::info!(
                "[extensions] 隐私防护已注入（严格档 strict，白名单 {} 个）+ 右键翻译菜单",
                whitelist.len()
            );
        }
        _ => {
            items.push((
                Some(format!(
                    "{}{}\n{}",
                    wl_prefix, PRIVACY_JS, CONTEXT_TRANSLATE_JS
                )),
                None,
            ));
            PRIVACY_INJECTIONS.fetch_add(1, Ordering::Relaxed);
            log::info!(
                "[extensions] 隐私防护已注入（标准档 standard，白名单 {} 个）+ 右键翻译菜单",
                whitelist.len()
            );
        }
    }
    // WebMCP polyfill 试验开关（阶段 1.5，默认关）
    let webmcp_on = take("webmcp_polyfill").map(|v| v == "true").unwrap_or(false);
    if webmcp_on {
        log::info!("[extensions] WebMCP polyfill 试验已开启");
    }
    Ok((items, webmcp_on))
}
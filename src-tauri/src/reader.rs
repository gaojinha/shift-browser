// 阅读模式：用 readability 算法提取正文，生成干净的大字排版页，新标签打开
// 注意：readability::scrape 内部是 blocking reqwest，必须 spawn_blocking，
// 绝不能跑在 async 线程/主线程上
use tauri::{AppHandle, Manager};

/// 抓正文超时（readability 内部 reqwest 无超时配置，外层兜底）
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 抓取并解析正文（带超时 + 净化）
/// 2026-09-19 修复：不再用 readability::extractor::scrape（其内置 reqwest 0.11 blocking，
/// 无 UA 配置且兼容性差，对部分站点误报 UnexpectedError）。
/// 改为自己用 reqwest 0.12 抓 HTML（可设 UA/超时/重定向），再调 extract 纯解析。
async fn scrape_article(url: &str) -> Result<readability::extractor::Product, String> {
    // P2：只允许 http/https——fetch_article/enter_reader_mode 共用入口，
    // 显式拒绝 file:/javascript: 等 scheme（readability 内部 reqwest 不认其他
    // scheme，但入口拦截更清晰，防下游意外处理）
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持普通网页".to_string());
    }
    // SSRF 防护（2026-09-19 修复）：拒绝内网/环回/链路本地/保留地址。
    // 此前任意 URL 都能让本机 reqwest 去抓，可被用来探测内网/云元数据（169.254.169.254）。
    validate_public_url(url).await?;
    let url = url.to_string();
    let url_for_log = url.clone();
    let r = tokio::time::timeout(
        FETCH_TIMEOUT,
        tokio::task::spawn_blocking(move || fetch_and_extract(&url)),
    )
    .await;
    match r {
        Ok(Ok(Ok(product))) => Ok(product),
        Ok(Ok(Err(e))) => {
            // 记日志便于定位（此前静默返回，用户只看到"提取不出正文"）
            log::warn!("[reader] 抓取/提取失败: {} -> {}", url_for_log, e);
            Err(e)
        }
        Ok(Err(e)) => {
            log::warn!("[reader] 抓取线程异常: {} -> {}", url_for_log, e);
            Err("正文提取线程异常".to_string())
        }
        Err(_) => Err("抓取正文超时（20 秒），可能是慢站或网络问题".to_string()),
    }
}

/// 抓 HTML + readability 纯解析（blocking，必须在 spawn_blocking 里跑）
/// 2026-09-19：改用系统 curl 抓取——reqwest/rustls 的 TLS 指纹被部分站点 WAF
/// 识别为爬虫（understandingwar.org 等返回 403），curl/OpenSSL 指纹被主流站信任。
/// 手动跟随重定向（最多 5 跳），每跳做 SSRF 校验，防重定向绕过。
fn fetch_and_extract(url: &str) -> Result<readability::extractor::Product, String> {
    const BROWSER_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36";
    // UA 双保险：浏览器 UA 优先（多数站正常）；若被反爬拦（非 2xx），
    // 回退默认 UA（个别站反着来，如 understandingwar.org 拦浏览器 UA 放行默认 UA）
    let html = match curl_fetch_with_redirects(url, Some(BROWSER_UA)) {
        Ok(h) => h,
        Err(_) => curl_fetch_with_redirects(url, None)?,
    };
    let parsed = url::Url::parse(url).map_err(|e| format!("URL 解析失败: {}", e))?;
    readability::extractor::extract(&mut html.as_bytes(), &parsed)
        .map_err(|e| format!("正文提取失败: {}", e))
}

/// 用系统 curl 抓取，手动跟随重定向并逐跳 SSRF 校验
pub(crate) fn curl_fetch_with_redirects(url: &str, ua: Option<&str>) -> Result<String, String> {
    let mut cur = url.to_string();
    for _ in 0..5 {
        // 单跳探测：拿状态码 + 重定向目标（curl 不带 -L 不自动跟随）
        let mut cmd = std::process::Command::new("curl");
        cmd.args([
            "-s",
            "--compressed",
            "--max-time",
            "20",
            "-o",
            "/dev/null",
            "-w",
            "%{http_code} %{redirect_url}",
        ]);
        if let Some(u) = ua {
            cmd.args(["-A", u]);
        }
        cmd.arg(&cur);
        let out = cmd.output().map_err(|e| format!("调用 curl 失败: {}", e))?;
        let meta = String::from_utf8_lossy(&out.stdout);
        let mut parts = meta.trim().splitn(2, ' ');
        let code: u16 = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let loc = parts.next().unwrap_or("").trim().to_string();
        if (300..400).contains(&code) && !loc.is_empty() {
            // 重定向目标也必须是公网（防 SSRF 重定向绕过）
            if let Err(e) = validate_public_host_blocking(&loc) {
                return Err(e);
            }
            cur = loc;
            continue;
        }
        if !(200..300).contains(&code) {
            return Err(format!("抓取失败: HTTP {}", code));
        }
        // 2xx：拿正文
        let mut cmd2 = std::process::Command::new("curl");
        cmd2.args(["-s", "--compressed", "--max-time", "20"]);
        if let Some(u) = ua {
            cmd2.args(["-A", u]);
        }
        cmd2.arg(&cur);
        let out2 = cmd2
            .output()
            .map_err(|e| format!("调用 curl 失败: {}", e))?;
        if !out2.status.success() {
            return Err(format!("抓取失败: curl 退出码 {:?}", out2.status.code()));
        }
        let html = String::from_utf8_lossy(&out2.stdout).to_string();
        if html.is_empty() {
            return Err("抓取结果为空".to_string());
        }
        return Ok(html);
    }
    Err("重定向次数过多".to_string())
}

/// 同步版 SSRF 主机校验（重定向逐跳用；初始 URL 用 async 版 validate_public_url）
fn validate_public_host_blocking(url: &str) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    let parsed = url::Url::parse(url).map_err(|_| "重定向目标 URL 不合法".to_string())?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "重定向目标缺少主机名".to_string())?;
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return reject_private_ip(&ip);
    }
    let addrs = (host, 80)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败: {} ({})", host, e))?;
    for addr in addrs {
        reject_private_ip(&addr.ip())?;
    }
    Ok(())
}

/// SSRF 校验：目标主机必须是公网地址。
/// - IP 字面量（127.0.0.1 / 10.x / 192.168.x / 169.254.x / ::1 / fc00:: / fe80:: 等）直接拒绝
/// - 域名 host 做 DNS 解析后逐一校验，任一解析结果落在内网即拒绝
pub(crate) async fn validate_public_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| {
        log::warn!("[reader] URL 解析失败: {} -> {}", url, e);
        "URL 格式不合法".to_string()
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL 缺少主机名".to_string())?
        .to_string();
    // IP 字面量：直接判
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        let r = reject_private_ip(&ip);
        if r.is_err() {
            log::warn!("[reader] SSRF 拦截 IP 字面量: {} -> {}", host, ip);
        }
        return r;
    }
    // 域名：异步解析全部 A/AAAA，任一落在内网即拒绝
    let addrs = tokio::net::lookup_host((host.as_str(), 80))
        .await
        .map_err(|e| {
            log::warn!("[reader] DNS 解析失败: {} -> {}", host, e);
            format!("DNS 解析失败: {}", host)
        })?;
    for addr in addrs {
        if let Err(e) = reject_private_ip(&addr.ip()) {
            log::warn!(
                "[reader] SSRF 拦截域名解析: {} -> {} ({})",
                host,
                addr.ip(),
                e
            );
            return Err(e);
        }
    }
    Ok(())
}

fn reject_private_ip(ip: &std::net::IpAddr) -> Result<(), String> {
    let bad = match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || {
                // fc00::/7（ULA 内网）与 fe80::/10（链路本地）
                let segs = v6.segments();
                (segs[0] & 0xfe00) == 0xfc00 || (segs[0] & 0xffc0) == 0xfe80
            }
        }
    };
    if bad {
        Err("目标地址是内网/保留地址，已拦截（SSRF 防护）".to_string())
    } else {
        Ok(())
    }
}

/// 净化正文 HTML（防 XSS）：用 ammonia（html5ever 真解析器）白名单模式。
/// 正则方案可被 `<svg/onload=alert(1)>`（无闭合、/on* 前无空白）绕过，
/// ammonia 按 HTML5 规范解析后只保留白名单标签/属性，on* 事件与危险协议一律剥离。
fn sanitize_html(html: &str) -> String {
    use std::collections::{HashMap, HashSet};
    let mut builder = ammonia::Builder::default();
    builder
        .tags(HashSet::from([
            // 块级
            "p",
            "div",
            "h1",
            "h2",
            "h3",
            "h4",
            "h5",
            "h6",
            "ul",
            "ol",
            "li",
            "blockquote",
            "pre",
            "hr",
            "br",
            "table",
            "thead",
            "tbody",
            "tr",
            "td",
            "th",
            "figure",
            "figcaption",
            "section",
            "article",
            "header",
            "footer",
            "main",
            "aside",
            // 行内
            "span",
            "a",
            "img",
            "strong",
            "em",
            "b",
            "i",
            "u",
            "s",
            "small",
            "code",
            "sub",
            "sup",
            "mark",
            "abbr",
            "cite",
            "q",
            "time",
            // 媒体（src 仅 http/https，data: 一律不放行）
            "video",
            "audio",
            "source",
            "picture",
        ]))
        .tag_attributes(HashMap::from([
            ("a", HashSet::from(["href", "title"])),
            (
                "img",
                HashSet::from(["src", "alt", "title", "width", "height"]),
            ),
            ("td", HashSet::from(["colspan", "rowspan"])),
            ("th", HashSet::from(["colspan", "rowspan"])),
            ("video", HashSet::from(["src", "controls", "poster"])),
            ("audio", HashSet::from(["src", "controls"])),
            ("source", HashSet::from(["src", "type"])),
        ]))
        .add_generic_attributes(HashSet::from(["class", "title"]))
        // 只允许 http/https URL（剥 javascript:/vbscript:/data:text/html 等可执行协议）
        .url_schemes(HashSet::from(["http", "https"]))
        .clean(html)
        .to_string()
}

/// 抓正文返回 JSON（不写文件）——供资讯面板"📖 内联阅读"用
#[tauri::command]
pub async fn fetch_article(url: String) -> Result<serde_json::Value, String> {
    let product = scrape_article(&url).await?;
    if product.content.len() < 200 {
        return Err("内容太短，可能不是文章页".to_string());
    }
    let content = sanitize_html(&product.content);
    Ok(serde_json::json!({
        "title": product.title,
        "content": content,
        "text": product.text,
    }))
}

/// 公共包装：供页面理解层（content_extract）复用抓取+readability 管线。
/// 出网操作，调用方须确保用户已授权。
pub async fn scrape_article_public(url: &str) -> Result<readability::extractor::Product, String> {
    scrape_article(url).await
}

/// 进入阅读模式：提取 url 正文 → 写 file:// 页面 → 返回新页面 URL（前端 createTab 打开）
#[tauri::command]
pub async fn enter_reader_mode(app: AppHandle, url: String) -> Result<String, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持普通网页".to_string());
    }
    let product = scrape_article(&url).await?;

    if product.content.len() < 500 {
        return Err("提取内容太短，可能不是文章页".to_string());
    }

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("reader");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // 只保留最近 20 篇，旧的删掉
    let mut old: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    old.sort();
    while old.len() > 19 {
        let _ = std::fs::remove_file(&old[0]);
        old.remove(0);
    }

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = dir.join(format!("r{}.html", ts));
    let content = sanitize_html(&product.content);
    let html = render_reader_html(&product.title, &url, &content);
    std::fs::write(&path, html).map_err(|e| e.to_string())?;
    Ok(format!("file://{}", path.display()))
}

/// 阅读页模板（占位符用 replace 注入，正文 HTML 里可能有花括号，不能走 format!）
fn render_reader_html(title: &str, source: &str, content: &str) -> String {
    r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>__TITLE__</title>
<!-- 阅读页是 file:// 加载，tauri.conf.json 的全局 CSP 不生效，必须自带 meta CSP。
     正文是外部网页内容（已过 sanitize_html），此处纵深防御：禁 connect/frame/form/base。 -->
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src https: http: data:; media-src https: http: data:; connect-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'">
<style>
  :root { --fs: 19px; }
  body { background:#1c1c1e; color:#d8d8d8; margin:0; font:var(--fs)/1.9 -apple-system,"PingFang SC","Microsoft YaHei",serif; }
  .bar { position:sticky; top:0; display:flex; align-items:center; gap:10px; padding:10px 16px; background:#232326; border-bottom:1px solid #333; font-size:13px; z-index:10; }
  .bar .src { color:#888; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; flex:1; }
  .bar a { color:#4a9eff; text-decoration:none; white-space:nowrap; }
  .fs-btn { border:1px solid #444; background:#2a2a2e; color:#ddd; border-radius:6px; padding:3px 10px; cursor:pointer; font-size:13px; }
  .fs-btn:hover { border-color:#4a9eff; }
  article { max-width:720px; margin:0 auto; padding:28px 24px 80px; }
  h1 { font-size:1.5em; line-height:1.5; margin:0 0 24px; color:#eee; }
  article img { max-width:100%; height:auto; display:block; margin:18px auto; border-radius:6px; }
  article p { margin:0 0 1.2em; }
  article a { color:#4a9eff; }
  article pre, article code { background:#26262a; border-radius:4px; padding:2px 6px; font-size:0.85em; }
  article pre { padding:12px; overflow-x:auto; }
  article blockquote { border-left:3px solid #4a9eff; margin:1em 0; padding:0.3em 1em; color:#aaa; background:#222226; }
  article table { border-collapse:collapse; width:100%; font-size:0.9em; }
  article td, article th { border:1px solid #3a3a3e; padding:6px 10px; }
  video { max-width:100%; }
</style>
</head>
<body>
  <div class="bar">
    <span style="color:#4a9eff">📖 阅读模式</span>
    <button class="fs-btn" id="fsDown">A−</button>
    <button class="fs-btn" id="fsUp">A＋</button>
    <span class="src">__SOURCE__</span>
    <a href="__SOURCE_LINK__" target="_blank">看原文 ↗</a>
  </div>
  <article>
    <h1>__TITLE__</h1>
    <div id="content">__CONTENT__</div>
  </article>
<script>
(function() {
  var root = document.documentElement;
  var fs = parseInt(localStorage.getItem('sb_reader_fs') || '19', 10);
  function apply() { root.style.setProperty('--fs', fs + 'px'); }
  apply();
  document.getElementById('fsUp').addEventListener('click', function() { fs = Math.min(fs + 2, 30); apply(); localStorage.setItem('sb_reader_fs', fs); });
  document.getElementById('fsDown').addEventListener('click', function() { fs = Math.max(fs - 2, 14); apply(); localStorage.setItem('sb_reader_fs', fs); });

  // ---- 懒加载图片修复：很多新闻站用 data-src/data-original 占位，直接渲染不显示 ----
  // 提升 data-* 占位为真 src，剥掉 srcset（多尺寸会加载错图/拖慢），补协议头，再懒加载滚动出图
  var CONT = document.getElementById('content');
  var imgFix = (function() {
    var imgs = CONT.querySelectorAll('img');
    for (var i = 0; i < imgs.length; i++) {
      var im = imgs[i];
      var s = im.getAttribute('data-src') || im.getAttribute('data-original') || im.getAttribute('data-lazy-src') || im.getAttribute('data-echo') || im.getAttribute('src');
      if (!s) continue;
      if (s.charAt(0) === '/' && s.charAt(1) === '/') s = location.protocol + s;       // //cdn → //当页协议
      if (im.hasAttribute('srcset')) im.removeAttribute('srcset');                     // 剥多尺寸
      if (im.src === im.baseURI && s !== '') { im.removeAttribute('src'); }            // 占位图 src 去掉
      im.setAttribute('src', s);
      im.setAttribute('loading', 'lazy');                                             // 滚动到才加载
      im.setAttribute('decoding', 'async');
      im.style.maxWidth = '100%'; im.style.height = 'auto';
    }
  })();
})();
</script>
</body>
</html>"#
        .replace("__TITLE__", &esc_html(title))
        .replace("__SOURCE__", &esc_html(source))
        .replace("__SOURCE_LINK__", &esc_attr(source))
        .replace("__CONTENT__", content)
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn esc_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_blocks_script_and_svg() {
        // 正则方案被绕过的用例：无闭合 svg + onload
        let s = sanitize_html(r#"<p>hi</p><svg/onload=alert(1)>"#);
        assert!(!s.contains("svg"));
        assert!(!s.contains("onload"));
        assert!(s.contains("hi"));
        // 完整 script 块删除
        let s = sanitize_html(r#"<p>a</p><script>alert(1)</script><p>b</p>"#);
        assert!(!s.contains("script"));
        assert!(s.contains("a"));
        assert!(s.contains("b"));
    }

    #[test]
    fn sanitize_strips_events_and_danger_uris() {
        // onerror 事件剥掉
        let s = sanitize_html(r#"<img src="https://x.com/a.png" onerror="alert(1)">"#);
        assert!(!s.contains("onerror"));
        assert!(s.contains("https://x.com/a.png"));
        // javascript: href 剥掉
        let s = sanitize_html(r#"<a href="javascript:alert(1)">点我</a>"#);
        assert!(!s.contains("javascript:"));
        assert!(s.contains("点我"));
        // data:text/html src 剥掉（url_schemes 只放 http/https）
        let s = sanitize_html(r#"<img src="data:text/html,<script>alert(1)</script>">"#);
        assert!(!s.contains("data:"));
        // 正常 https 链接保留
        let s = sanitize_html(r#"<a href="https://ok.com/x">链接</a>"#);
        assert!(s.contains("https://ok.com/x"));
        // iframe/object/embed 删除
        let s = sanitize_html(
            r#"<p>a</p><iframe src="https://evil.com"></iframe><object data="x"></object><embed src="y">"#,
        );
        assert!(!s.contains("iframe"));
        assert!(!s.contains("object"));
        assert!(!s.contains("embed"));
        // base 删除（防相对链接劫持）
        let s = sanitize_html(r#"<base href="https://evil.com/"><p>a</p>"#);
        assert!(!s.contains("base"));
    }

    #[test]
    fn sanitize_keeps_normal_content() {
        let s = sanitize_html(
            r#"<h1>标题</h1><p>正文 <strong>粗体</strong> <a href="https://x.com">链接</a></p><ul><li>项</li></ul>"#,
        );
        assert!(s.contains("标题"));
        assert!(s.contains("粗体"));
        assert!(s.contains("链接"));
        assert!(s.contains("<li>"));
    }
}

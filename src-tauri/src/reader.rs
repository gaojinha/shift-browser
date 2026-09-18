// 阅读模式：用 readability 算法提取正文，生成干净的大字排版页，新标签打开
// 注意：readability::scrape 内部是 blocking reqwest，必须 spawn_blocking，
// 绝不能跑在 async 线程/主线程上
use tauri::{AppHandle, Manager};

/// 抓正文超时（readability 内部 reqwest 无超时配置，外层兜底）
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// 抓取并解析正文（带超时 + 净化）
async fn scrape_article(url: &str) -> Result<readability::extractor::Product, String> {
    // P2：只允许 http/https——fetch_article/enter_reader_mode 共用入口，
    // 显式拒绝 file:/javascript: 等 scheme（readability 内部 reqwest 不认其他
    // scheme，但入口拦截更清晰，防下游意外处理）
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("只支持普通网页".to_string());
    }
    let url = url.to_string();
    tokio::time::timeout(
        FETCH_TIMEOUT,
        tokio::task::spawn_blocking(move || readability::extractor::scrape(&url)),
    )
    .await
    .map_err(|_| "抓取正文超时（20 秒），可能是慢站或网络问题".to_string())?
    .map_err(|e| e.to_string())?
    .map_err(|_| "提取不出正文（可能是动态页或内容太短）".to_string())
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

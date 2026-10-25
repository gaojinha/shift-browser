//! CEF 专用启动页：不依赖 Tauri IPC 的最小可用版本。
//!
//! 现有的 `newtab/index.html` 重度依赖 `window.__TAURI_INTERNALS__`（23 处 invoke），
//! 在 CEF 里跑会整屏报错。这里给一个自包含的替代页：搜索框 + 常用站点，
//! 交互全靠页面自己（`location.href`），不回传宿主。

use std::path::Path;

/// 写入启动页 HTML 并返回 `file://` URL（幂等：文件已存在则直接复用）。
pub fn ensure_newtab_html(shell_dir: &Path) -> Result<String, String> {
    let dir = shell_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("建启动页目录失败: {e}"))?;
    let path = dir.join("cef-newtab.html");
    std::fs::write(&path, NEWTAB_HTML).map_err(|e| format!("写启动页失败: {e}"))?;
    Ok(format!("file://{}", path.display()))
}

const NEWTAB_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>新标签页</title>
<style>
  * { box-sizing: border-box; }
  html, body {
    margin: 0; padding: 0; height: 100%;
    background: radial-gradient(circle at 50% 0%, #1e2532 0%, #12151b 60%);
    color: #e6e8eb; font-family: -apple-system, "Noto Sans CJK SC", "Microsoft YaHei", sans-serif;
    display: flex; align-items: center; justify-content: center;
    user-select: none;
  }
  .wrap { width: 620px; max-width: 86vw; text-align: center; }
  .brand {
    font-size: 34px; font-weight: 600; letter-spacing: 2px;
    color: #8ab4f8; margin-bottom: 6px;
  }
  .sub { font-size: 13px; color: #7c8593; margin-bottom: 28px; }
  .search {
    display: flex; align-items: center; height: 46px;
    background: #171b22; border: 1px solid #2f3742; border-radius: 23px;
    padding: 0 18px; transition: border-color .15s, box-shadow .15s;
  }
  .search:focus-within {
    border-color: #5b8def; box-shadow: 0 0 0 3px rgba(91,141,239,.18);
  }
  .search svg { width: 17px; height: 17px; stroke: #6b7482; fill: none;
    stroke-width: 2; stroke-linecap: round; margin-right: 10px; flex: 0 0 auto; }
  #q {
    flex: 1; height: 100%; border: 0; outline: 0; background: transparent;
    color: #e6e8eb; font-size: 15px;
  }
  #q::placeholder { color: #5d6674; }
  .links {
    display: flex; flex-wrap: wrap; justify-content: center; gap: 10px;
    margin-top: 30px;
  }
  .link {
    display: flex; align-items: center; gap: 7px;
    height: 34px; padding: 0 14px; border-radius: 10px;
    background: #171b22; border: 1px solid #262d38;
    color: #b9c0cb; font-size: 13px; text-decoration: none;
    transition: background .15s, border-color .15s, color .15s;
  }
  .link:hover { background: #1f2530; border-color: #3a4453; color: #e6e8eb; }
  .link i { font-style: normal; font-size: 14px; }
  .hint { margin-top: 26px; font-size: 12px; color: #4d5563; }
</style>
</head>
<body>
<div class="wrap">
  <div class="brand">Shift</div>
  <div class="sub">输入网址或搜索词，回车打开</div>
  <div class="search">
    <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>
    <input id="q" type="text" spellcheck="false" placeholder="搜索或输入网址" autofocus>
  </div>
  <div class="links">
    <a class="link" href="https://www.baidu.com"><i>🔍</i>百度</a>
    <a class="link" href="https://www.zhihu.com"><i>📖</i>知乎</a>
    <a class="link" href="https://www.bilibili.com"><i>📺</i>B站</a>
    <a class="link" href="https://github.com"><i>🐙</i>GitHub</a>
    <a class="link" href="https://news.ycombinator.com"><i>📰</i>HN</a>
  </div>
  <div class="hint">Ctrl+T 新标签 · Ctrl+W 关闭 · Ctrl+Tab 切换</div>
</div>
<script>
  const q = document.getElementById('q');

  function go() {
    const v = q.value.trim();
    if (!v) return;
    // 有 scheme 或像域名 → 直接访问；否则当搜索词
    let url;
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(v) || v.startsWith('about:') || v.startsWith('data:')) {
      url = v;
    } else if (v.includes('.') && !v.includes(' ')) {
      url = 'https://' + v;
    } else {
      url = 'https://www.google.com/search?q=' + encodeURIComponent(v);
    }
    location.href = url;
  }

  q.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') go();
    else if (e.key === 'Escape') q.blur();
  });

  q.focus();
  document.addEventListener('dragstart', (e) => e.preventDefault());
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::ensure_newtab_html;

    #[test]
    fn newtab_html_written_and_self_contained() {
        let dir = std::env::temp_dir().join(format!("cef-newtab-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let url = ensure_newtab_html(&dir).expect("写启动页失败");
        assert!(url.ends_with("cef-newtab.html"));
        let disk = std::path::Path::new(url.strip_prefix("file://").expect("前缀"));
        assert!(disk.exists(), "启动页未落盘: {url}");
        let body = std::fs::read_to_string(disk).unwrap();
        assert!(body.contains("id=\"q\""), "必须有搜索框");
        assert!(!body.contains("__TAURI_INTERNALS__"), "不得依赖 Tauri IPC");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

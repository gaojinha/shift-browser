//! 工具栏 WebUI：本地 HTML 文件生成
//!
//! 工具栏本身就是一个网页，跑在 CEF 的独立 BrowserView 里——跟内容页同一套
//! 渲染/输入栈。里面用 `console.log("SHIFT_ACT:...")` 把动作传回浏览器进程
//! （MVP 桥，后续换 MessageRouter）。

use std::path::Path;

/// 生成（覆盖写）工具栏 HTML，返回可直接加载的 `file://` URL
pub fn ensure_toolbar_html(cache_dir: &Path) -> Result<String, String> {
    let dir = cache_dir.join("shell");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建工具栏目录失败: {e}"))?;

    let path = dir.join("toolbar.html");
    std::fs::write(&path, TOOLBAR_HTML).map_err(|e| format!("写工具栏页失败: {e}"))?;

    // 用绝对路径拼 file:// URL（路径里不能有空格，缓存目录一般没有）
    let url = format!("file://{}", path.display());
    Ok(url)
}

const TOOLBAR_HTML: &str = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<style>
  * { box-sizing: border-box; }
  html, body {
    margin: 0; padding: 0; height: 48px; overflow: hidden;
    background: #20242c; color: #e6e8eb;
    font-family: -apple-system, "Noto Sans CJK SC", "Microsoft YaHei", sans-serif;
    font-size: 13px;
    user-select: none;
  }
  .bar {
    display: flex; align-items: center; gap: 6px;
    height: 48px; padding: 0 10px;
    border-bottom: 1px solid #333844;
  }
  .brand {
    font-weight: 600; letter-spacing: .5px; color: #8ab4f8;
    padding-right: 6px; white-space: nowrap;
  }
  button {
    width: 30px; height: 30px; border: 0; border-radius: 8px;
    background: transparent; color: #cfd3da; cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    padding: 0;
  }
  button:hover { background: #2d3340; }
  button:active { background: #3a4150; }
  button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2;
    stroke-linecap: round; stroke-linejoin: round; }
  .addr-wrap {
    flex: 1; display: flex; align-items: center; height: 32px;
    background: #151920; border: 1px solid #333844; border-radius: 16px;
    padding: 0 12px; min-width: 0;
  }
  .addr-wrap:focus-within { border-color: #5b8def; background: #10141a; }
  #addr {
    flex: 1; min-width: 0; border: 0; outline: 0; background: transparent;
    color: #e6e8eb; font-size: 13px;
  }
  #addr::placeholder { color: #6b7280; }
  .lock { width: 14px; height: 14px; margin-right: 8px; color: #8b93a1; flex: none; }
</style>
</head>
<body>
  <div class="bar">
    <span class="brand">Shift</span>

    <button title="后退" onclick="act('back')">
      <svg viewBox="0 0 24 24"><path d="M15 18l-6-6 6-6"/></svg>
    </button>
    <button title="前进" onclick="act('forward')">
      <svg viewBox="0 0 24 24"><path d="M9 18l6-6-6-6"/></svg>
    </button>
    <button title="刷新" onclick="act('reload')">
      <svg viewBox="0 0 24 24"><path d="M21 12a9 9 0 1 1-3-6.7"/><path d="M21 3v6h-6"/></svg>
    </button>

    <div class="addr-wrap">
      <svg class="lock" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="4" y="10" width="16" height="10" rx="2"/>
        <path d="M8 10V7a4 4 0 0 1 8 0v3"/>
      </svg>
      <input id="addr" type="text" spellcheck="false" placeholder="输入网址或搜索词，回车打开">
    </div>
  </div>

<script>
  const addr = document.getElementById('addr');

  function act(cmd) {
    console.log('SHIFT_ACT:' + cmd);
  }

  addr.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      const v = addr.value.trim();
      if (v) act('nav|' + v);
      addr.blur();
    } else if (e.key === 'Escape') {
      addr.blur();
    }
  });

  addr.addEventListener('focus', () => addr.select());

  // 防止工具栏页面拖动/选中
  document.addEventListener('dragstart', (e) => e.preventDefault());
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::ensure_toolbar_html;
    use std::path::Path;

    #[test]
    fn ensure_toolbar_html_writes_file_and_content_bridge() {
        let dir = std::env::temp_dir().join(format!("cef-toolbar-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let out = ensure_toolbar_html(&dir).unwrap();
        assert!(out.starts_with("file://"), "须返回 file:// URL，实得 {out}");
        assert!(out.ends_with("toolbar.html"));
        // URL 是 file:// 形态，落盘路径剥掉前缀再验
        let disk_path = Path::new(out.strip_prefix("file://").expect("URL 前缀"));
        assert!(disk_path.exists(), "工具栏 HTML 未落盘: {out}");
        let body = std::fs::read_to_string(disk_path).unwrap();
        assert!(
            body.contains("SHIFT_ACT"),
            "工具栏 HTML 必须带 console.log 动作桥"
        );
        assert!(body.contains("addr"), "工具栏 HTML 必须含地址栏");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

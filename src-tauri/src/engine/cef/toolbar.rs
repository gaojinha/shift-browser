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
    margin: 0; padding: 0; height: 84px; overflow: hidden;
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
  /* 标签栏（2026-10-26 新增，M2 独立开窗方向） */
  .tabbar {
    display: flex; align-items: center; gap: 4px;
    height: 36px; padding: 0 8px;
    background: #1b1f26; border-bottom: 1px solid #2b3038;
    overflow-x: auto; overflow-y: hidden;
  }
  .tabbar::-webkit-scrollbar { height: 0; }
  .tab {
    display: flex; align-items: center; gap: 6px;
    min-width: 90px; max-width: 200px; height: 28px;
    padding: 0 4px 0 10px; border-radius: 8px;
    background: transparent; color: #9aa2ad; cursor: default;
    font-size: 12px; white-space: nowrap; flex: 0 0 auto;
  }
  .tab:hover { background: #242a33; }
  .tab.active { background: #2f3742; color: #e6e8eb; }
  .tab .t-title {
    flex: 1; overflow: hidden; text-overflow: ellipsis;
  }
  .tab .t-close {
    width: 18px; height: 18px; border-radius: 6px; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    font-size: 13px; line-height: 1; color: #8b93a0;
  }
  .tab .t-close:hover { background: #3d4552; color: #e6e8eb; }
  .tab-new {
    width: 26px; height: 26px; border-radius: 8px; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    color: #cfd3da; font-size: 17px; line-height: 1;
  }
  .tab-new:hover { background: #2d3340; }
  /* 加载中：刷新按钮转圈（Rust 侧 data-loading 控制） */
  @keyframes spin { to { transform: rotate(360deg); } }
  html[data-loading="1"] #reload svg { animation: spin .9s linear infinite; }
  html[data-loading="1"] #reload { color: #8ab4f8; }
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
  <div class="tabbar" id="tabbar">
    <div class="tab active" data-idx="0">
      <span class="t-title">新标签页</span>
      <span class="t-close" title="关闭标签">×</span>
    </div>
    <div class="tab-new" id="tabnew" title="新建标签">+</div>
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

  // ---- 地址栏 / 加载态（Rust 侧 execute_java_script 调用）----
  window.__shiftUrl = function (url) {
    if (document.activeElement === addr) return; // 用户正在输入时不打断
    addr.value = url || '';
  };
  window.__shiftLoading = function (on) {
    document.documentElement.dataset.loading = on ? '1' : '';
  };

  // ---- 标签栏（M2 独立开窗）----
  // Rust 侧通过 execute_java_script 调用 window.__shiftTabs(data) 刷新标签栏；
  // 点击/关闭/新建通过 console.log SHIFT_ACT 回传，与地址栏同一套桥。
  const tabbar = document.getElementById('tabbar');

  window.__shiftTabs = function (tabs, active) {
    if (!Array.isArray(tabs)) return;
    tabbar.innerHTML = '';
    tabs.forEach((t, i) => {
      const el = document.createElement('div');
      el.className = 'tab' + (i === active ? ' active' : '');
      el.dataset.idx = String(i);
      const title = document.createElement('span');
      title.className = 't-title';
      title.textContent = t.title || '新标签页';
      const close = document.createElement('span');
      close.className = 't-close';
      close.title = '关闭标签';
      close.textContent = '×';
      el.appendChild(title);
      el.appendChild(close);
      tabbar.appendChild(el);
    });
    const add = document.createElement('div');
    add.className = 'tab-new';
    add.id = 'tabnew';
    add.title = '新建标签';
    add.textContent = '+';
    tabbar.appendChild(add);
  };

  tabbar.addEventListener('click', (e) => {
    const closeBtn = e.target.closest('.t-close');
    if (closeBtn) {
      const idx = closeBtn.closest('.tab').dataset.idx;
      act('tabclose|' + idx);
      e.stopPropagation();
      return;
    }
    if (e.target.closest('.tab-new')) {
      act('tabnew');
      return;
    }
    const tab = e.target.closest('.tab');
    if (tab) act('tab|' + tab.dataset.idx);
  });

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

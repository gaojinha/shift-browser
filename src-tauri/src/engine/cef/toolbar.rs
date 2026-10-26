//! 工具栏 WebUI：本地 HTML 文件生成
//!
//! 工具栏本身就是一个网页，跑在 CEF 的独立 BrowserView 里——跟内容页同一套
//! 渲染/输入栈。里面用 `console.log("SHIFT_ACT:...")` 把动作传回浏览器进程
//! （MVP 桥，后续换 MessageRouter）。

use std::path::Path;

/// 生成（覆盖写）工具栏 HTML，返回可直接加载的 `file://` URL
pub fn ensure_toolbar_html(
    cache_dir: &Path,
    bookmarks_json: &str,
    passwords_json: &str,
) -> Result<String, String> {
    let dir = cache_dir.join("shell");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建工具栏目录失败: {e}"))?;

    // 书签直接嵌进页面：CEF 侧的推送时机不可靠（glib 定时器在回调链里不触发、
    // console 桥的单向请求也实测没到），生成页面时写死最稳。
    // 密码 JSON 只含 id/站点/用户名；"</" 转义防止数据提前闭合 <script>
    let html = TOOLBAR_HTML
        .replace("__SHIFT_BOOKMARKS_JSON__", bookmarks_json)
        .replace("__SHIFT_PASSWORDS_JSON__", &passwords_json.replace("</", "<\\/"));
    let path = dir.join("toolbar.html");
    std::fs::write(&path, html).map_err(|e| format!("写工具栏页失败: {e}"))?;

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
    margin: 0; padding: 0; height: 122px; overflow: hidden;
    /* Chrome 深色模式配色（2026-10-26 按 Chrome 实测对齐） */
    background: #35363a; color: #e8eaed;
    font-family: "Segoe UI", "Noto Sans CJK SC", "Microsoft YaHei", system-ui, sans-serif;
    font-size: 13px;
    user-select: none;
  }
  .bar {
    display: flex; align-items: center; gap: 2px;
    height: 48px; padding: 0 8px;
    background: #35363a;
  }
  .brand {
    font-weight: 600; letter-spacing: .3px; color: #8ab4f8;
    padding: 0 6px 0 2px; white-space: nowrap; font-size: 13px;
  }
  button {
    width: 28px; height: 28px; border: 0; border-radius: 50%;
    background: transparent; color: #e8eaed; cursor: default;
    display: flex; align-items: center; justify-content: center;
    padding: 0; flex: 0 0 auto;
  }
  button:hover { background: #3c4043; }
  button:active { background: #4a4d51; }
  button:disabled { color: #5f6368; }
  button svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 2;
    stroke-linecap: round; stroke-linejoin: round; }
  /* 标签栏：Chrome 深色 40px（标签在上、工具栏在下，顺序与 Chrome 一致） */
  .tabbar {
    display: flex; align-items: flex-end; gap: 0;
    height: 40px; padding: 4px 8px 0;
    background: #202124;
    overflow-x: auto; overflow-y: hidden;
  }
  .tabbar::-webkit-scrollbar { height: 0; }
  .tabs {
    display: flex; align-items: flex-end; flex: 1; min-width: 0;
    height: 100%; overflow-x: auto; overflow-y: hidden;
  }
  .tabs::-webkit-scrollbar { height: 0; }
  /* 窗口控制按钮（Chrome 画在标题栏右侧，Linux 版是方角 hover 块） */
  .win-ctrl { display: flex; align-items: flex-end; flex: 0 0 auto; height: 40px; }
  .win-ctrl button {
    width: 44px; height: 32px; border-radius: 0; color: #e8eaed;
    font-size: 13px; line-height: 1;
  }
  .win-ctrl button:hover { background: #3c4043; }
  .win-ctrl button.close:hover { background: #e81123; color: #fff; }
  .tab {
    display: flex; align-items: center; gap: 8px;
    min-width: 32px; max-width: 240px; height: 36px;
    padding: 0 6px 0 12px; border-radius: 8px 8px 0 0;
    background: transparent; color: #9aa0a6; cursor: default;
    font-size: 12px; white-space: nowrap; flex: 0 1 auto;
    position: relative;
  }
  .tab:hover { background: #2a2b2e; color: #e8eaed; }
  .tab.active { background: #35363a; color: #e8eaed; }
  /* 标签之间的分隔线（Chrome 非激活标签右侧一道细线，激活/相邻时隐藏） */
  .tab + .tab::before {
    content: ''; position: absolute; left: -1px; top: 10px; bottom: 10px;
    width: 1px; background: #3c4043;
  }
  .tab.active::before, .tab.active + .tab::before { display: none; }
  .tab:hover::before, .tab:hover + .tab::before { display: none; }
  .tab .t-title {
    flex: 1; overflow: hidden; text-overflow: ellipsis;
  }
  .tab .t-close {
    width: 16px; height: 16px; border-radius: 50%; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    font-size: 12px; line-height: 1; color: #9aa0a6;
  }
  .tab .t-close:hover { background: #3c4043; color: #e8eaed; }
  .tab-new {
    width: 28px; height: 28px; border-radius: 50%; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    color: #e8eaed; font-size: 18px; line-height: 1; margin: 0 0 4px 4px;
  }
  .tab-new:hover { background: #3c4043; }
  /* 加载中：刷新按钮转圈（Rust 侧 data-loading 控制） */
  @keyframes spin { to { transform: rotate(360deg); } }
  html[data-loading="1"] #reload svg { animation: spin .9s linear infinite; }
  html[data-loading="1"] #reload { color: #8ab4f8; }
  /* 书签栏：Chrome 34px，图标 + 名称，横向滚动 */
  .bookmarks {
    display: flex; align-items: center; gap: 2px;
    height: 34px; padding: 0 8px;
    background: #35363a;
    overflow-x: auto; overflow-y: hidden;
  }
  .bookmarks::-webkit-scrollbar { height: 0; }
  /* 书签栏收起态（F-CEF-09，Ctrl+Shift+B）：在 html 上挂 bookmarks-hidden，
     同时把页面高度从 122 收到 88（少书签栏 34px），否则底部留白。 */
  html.bookmarks-hidden, html.bookmarks-hidden body { height: 88px; }
  html.bookmarks-hidden .bookmarks { display: none; }
  .bm {
    display: flex; align-items: center; gap: 6px;
    height: 26px; padding: 0 8px; border-radius: 6px;
    color: #e8eaed; font-size: 12px; white-space: nowrap;
    flex: 0 0 auto; cursor: default;
  }
  .bm:hover { background: #3c4043; }
  .bm .bm-ico {
    width: 16px; height: 16px; border-radius: 4px; flex: 0 0 auto;
    display: flex; align-items: center; justify-content: center;
    background: #4a4d51; font-size: 10px; color: #e8eaed;
  }
  .bm .bm-name {
    max-width: 140px; overflow: hidden; text-overflow: ellipsis;
  }
  /* 密码面板（F-CEF-12）：展开时 Rust 把工具栏视图加高 PW_PANEL_EXTRA，html 跟随视图高度 */
  html.pw-open, html.pw-open body { height: 100vh; }
  #pwpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10;
  }
  html.pw-open #pwpanel { display: flex; }
  html:not(.bookmarks-hidden) #pwpanel { top: 122px; }
  #pwsearch {
    height: 28px; border: 1px solid #3c4043; border-radius: 14px; outline: 0;
    background: #202124; color: #e8eaed; padding: 0 12px; font-size: 12px; user-select: text;
  }
  #pwlist { flex: 1; overflow-y: auto; }
  .pw-row {
    display: flex; align-items: center; gap: 10px; height: 28px; padding: 0 6px;
    border-radius: 6px; font-size: 12px;
  }
  .pw-row:hover { background: #3c4043; }
  .pw-row .pw-dom { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pw-row .pw-usr { flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #9aa0a6; }
  .pw-row .pw-mask { flex: 0 0 48px; color: #9aa0a6; letter-spacing: 1px; }
  .pw-row .pw-btn {
    flex: 0 0 auto; padding: 0 8px; height: 22px; border-radius: 11px; line-height: 22px;
    color: #8ab4f8; font-size: 12px;
  }
  .pw-row .pw-btn:hover { background: #4a4d51; }
  .pw-row .pw-btn.danger { color: #f28b82; }
  .pw-empty { color: #9aa0a6; padding: 12px 6px; font-size: 12px; }
  .addr-wrap {
    flex: 1; display: flex; align-items: center; height: 32px;
    background: #202124; border: 1px solid transparent; border-radius: 16px;
    padding: 0 12px; min-width: 0; margin: 0 4px;
  }
  .addr-wrap:hover { background: #292a2d; }
  .addr-wrap:focus-within { background: #202124; border-color: #8ab4f8; }
  .addr-wrap svg { width: 16px; height: 16px; stroke: #9aa0a6; fill: none;
    stroke-width: 2; stroke-linecap: round; margin-right: 8px; flex: 0 0 auto; }
  #addr { flex: 1; height: 100%; border: 0; outline: 0; background: transparent;
    color: #e8eaed; font-size: 13px; min-width: 0; }
  #addr::placeholder { color: #9aa0a6; }
  #addr {
    flex: 1; min-width: 0; border: 0; outline: 0; background: transparent;
    color: #e6e8eb; font-size: 13px;
  }
  #addr::placeholder { color: #6b7280; }
  .lock { width: 14px; height: 14px; margin-right: 8px; color: #8b93a1; flex: none; }
</style>
</head>
<body>
  <div class="tabbar" id="tabbar">
    <div class="tabs" id="tabs">
      <div class="tab active" data-idx="0">
        <span class="t-title">新标签页</span>
        <span class="t-close" title="关闭标签">×</span>
      </div>
      <div class="tab-new" id="tabnew" title="新建标签">+</div>
    </div>
    <div class="win-ctrl">
      <button title="最小化" data-act="winmin">─</button>
      <button title="最大化" data-act="winmax">□</button>
      <button class="close" title="关闭" data-act="winclose">✕</button>
    </div>
  </div>
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
    <button title="密码" id="pwbtn">
      <svg viewBox="0 0 24 24"><circle cx="8" cy="15" r="4"/><path d="M11 12l9-9"/><path d="M16 7l3 3"/></svg>
    </button>
  </div>
  <div class="bookmarks" id="bookmarks"></div>
  <div id="pwpanel">
    <input id="pwsearch" type="text" spellcheck="false" placeholder="搜索站点或用户名">
    <div id="pwlist"></div>
  </div>
  <script id="bm-data" type="application/json">__SHIFT_BOOKMARKS_JSON__</script>
  <script id="pw-data" type="application/json">__SHIFT_PASSWORDS_JSON__</script>


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
  const tabs = document.getElementById('tabs');

  window.__shiftTabs = function (tabs_, active) {
    if (!Array.isArray(tabs_)) return;
    tabs.innerHTML = '';
    tabs_.forEach((t, i) => {
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
      tabs.appendChild(el);
    });
    const add = document.createElement('div');
    add.className = 'tab-new';
    add.id = 'tabnew';
    add.title = '新建标签';
    add.textContent = '+';
    tabs.appendChild(add);
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

  // 书签栏：Rust 侧读到数据库后整表刷新；未读到前保留 HTML 里的静态兜底
  window.__shiftBookmarks = function (list) {
    const bar = document.getElementById('bookmarks');
    if (!bar || !Array.isArray(list)) return;
    if (list.length === 0) return; // 库里没书签就保留兜底，避免空条
    bar.innerHTML = '';
    list.forEach((b) => {
      const el = document.createElement('div');
      el.className = 'bm';
      el.dataset.url = b.url || '';
      const ico = document.createElement('span');
      ico.className = 'bm-ico';
      const title = b.title || b.url || '书签';
      ico.textContent = title.trim().charAt(0).toUpperCase();
      const name = document.createElement('span');
      name.className = 'bm-name';
      name.textContent = title;
      el.appendChild(ico);
      el.appendChild(name);
      bar.appendChild(el);
    });
  };

  // 书签栏显隐（F-CEF-09，Ctrl+Shift+B，Rust 侧 execute_java_script 调用）：
  // 在 html 挂 bookmarks-hidden 类，CSS 同时隐藏书签栏并把页面高度 122→88。
  window.__shiftBookmarksVisible = function (visible) {
    document.documentElement.classList.toggle('bookmarks-hidden', !visible);
  };

  // 书签点击 → 当前标签导航
  document.getElementById('bookmarks').addEventListener('click', (e) => {
    const bm = e.target.closest('.bm');
    if (bm && bm.dataset.url) act('nav|' + bm.dataset.url);
  });

  // 窗口控制按钮
  document.querySelector('.win-ctrl').addEventListener('click', (e) => {
    const btn = e.target.closest('button[data-act]');
    if (!btn) return;
    act(btn.dataset.act);
  });

  // 拖动窗口：按住标签栏空白处拖动（走 _NET_WM_MOVERESIZE，由 WM 接管）
  tabbar.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    if (e.target.closest('.tab') || e.target.closest('button')) return;
    act('windrag');
  });

  // 中键关闭标签（浏览器通用习惯）
  tabbar.addEventListener('auxclick', (e) => {
    if (e.button !== 1) return;
    const tab = e.target.closest('.tab');
    if (!tab) return;
    e.preventDefault();
    act('tabclose|' + tab.dataset.idx);
  });

  // 标签栏空白处双击 = 新建标签（与主界面一致）
  tabbar.addEventListener('dblclick', (e) => {
    if (e.target.closest('.tab')) return;
    act('tabnew');
  });

  // 启动时渲染内嵌书签（生成页面时由 Rust 写入，无时序问题）
  (function renderBookmarks() {
    const el = document.getElementById('bm-data');
    const bar = document.getElementById('bookmarks');
    if (!el || !bar) return;
    let list = [];
    try { list = JSON.parse(el.textContent || '[]'); } catch (e) { list = []; }
    if (!Array.isArray(list) || list.length === 0) return;
    bar.innerHTML = '';
    list.forEach((b) => {
      const item = document.createElement('div');
      item.className = 'bm';
      item.dataset.url = b.url || '';
      const ico = document.createElement('span');
      ico.className = 'bm-ico';
      const title = b.title || b.url || '书签';
      ico.textContent = title.trim().charAt(0).toUpperCase();
      const name = document.createElement('span');
      name.className = 'bm-name';
      name.textContent = title;
      item.appendChild(ico);
      item.appendChild(name);
      bar.appendChild(item);
    });
  })();

  // ---- 密码面板（F-CEF-12）----
  // 数据只有 id/站点/用户名；明文永不进入本页——复制/删除全在 Rust 侧完成。
  let pwList = [];
  try { pwList = JSON.parse(document.getElementById('pw-data').textContent || '[]'); } catch (e) { pwList = []; }
  if (!Array.isArray(pwList)) pwList = [];
  const pwListEl = document.getElementById('pwlist');
  const pwSearch = document.getElementById('pwsearch');

  function renderPasswords() {
    const q = pwSearch.value.trim().toLowerCase();
    pwListEl.innerHTML = '';
    const hit = pwList.filter((p) =>
      !q || (p.domain || '').toLowerCase().includes(q) || (p.username || '').toLowerCase().includes(q));
    if (hit.length === 0) {
      const e = document.createElement('div');
      e.className = 'pw-empty';
      e.textContent = pwList.length === 0 ? '密码库为空' : '没有匹配项';
      pwListEl.appendChild(e);
      return;
    }
    hit.forEach((p) => {
      const row = document.createElement('div');
      row.className = 'pw-row';
      row.dataset.id = String(p.id);
      const dom = document.createElement('span');
      dom.className = 'pw-dom';
      dom.textContent = p.domain || '';
      const usr = document.createElement('span');
      usr.className = 'pw-usr';
      usr.textContent = p.username || '';
      const mask = document.createElement('span');
      mask.className = 'pw-mask';
      mask.textContent = '••••';
      const cp = document.createElement('span');
      cp.className = 'pw-btn';
      cp.dataset.op = 'copy';
      cp.textContent = '复制';
      const del = document.createElement('span');
      del.className = 'pw-btn danger';
      del.dataset.op = 'del';
      del.textContent = '删除';
      row.append(dom, usr, mask, cp, del);
      pwListEl.appendChild(row);
    });
  }

  function setPwPanel(open) {
    document.documentElement.classList.toggle('pw-open', open);
    act('pwpanel|' + (open ? '1' : '0'));
    if (open) { pwSearch.value = ''; renderPasswords(); pwSearch.focus(); }
  }
  document.getElementById('pwbtn').addEventListener('click', () => {
    setPwPanel(!document.documentElement.classList.contains('pw-open'));
  });
  pwSearch.addEventListener('input', renderPasswords);
  pwSearch.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') setPwPanel(false);
  });
  pwListEl.addEventListener('click', (e) => {
    const btn = e.target.closest('.pw-btn');
    const row = e.target.closest('.pw-row');
    if (!btn || !row) return;
    const id = row.dataset.id;
    if (btn.dataset.op === 'copy') {
      act('pwcopy|' + id);
      btn.textContent = '已复制';
      setTimeout(() => { btn.textContent = '复制'; }, 1500);
    } else if (btn.dataset.op === 'del') {
      // 二次确认：第一次点变「确认删除」，3 秒内再点才真删
      if (btn.dataset.armed === '1') {
        act('pwdel|' + id);
        pwList = pwList.filter((p) => String(p.id) !== id);
        renderPasswords();
      } else {
        btn.dataset.armed = '1';
        btn.textContent = '确认删除';
        setTimeout(() => { btn.dataset.armed = ''; btn.textContent = '删除'; }, 3000);
      }
    }
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
        let out = ensure_toolbar_html(
            &dir,
            r#"[{"title":"测试书签","url":"https://a.com"}]"#,
            r#"[{"id":1,"domain":"a.com</script>","username":"u"}]"#,
        )
        .unwrap();
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
        assert!(body.contains("pwpanel"), "工具栏 HTML 必须含密码面板");
        assert!(
            body.contains(r#""domain":"a.com<\/script>""#),
            "密码 JSON 里的 </ 必须转义，防提前闭合 script"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

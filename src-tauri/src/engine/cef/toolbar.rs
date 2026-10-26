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
    downloads_json: &str,
) -> Result<String, String> {
    let dir = cache_dir.join("shell");
    std::fs::create_dir_all(&dir).map_err(|e| format!("建工具栏目录失败: {e}"))?;

    // 书签直接嵌进页面：CEF 侧的推送时机不可靠（glib 定时器在回调链里不触发、
    // console 桥的单向请求也实测没到），生成页面时写死最稳。
    // 密码 JSON 只含 id/站点/用户名；"</" 转义防止数据提前闭合 <script>
    let html = TOOLBAR_HTML
        .replace("__SHIFT_BOOKMARKS_JSON__", bookmarks_json)
        .replace("__SHIFT_PASSWORDS_JSON__", &passwords_json.replace("</", "<\\/"))
        .replace("__SHIFT_DOWNLOADS_JSON__", &downloads_json.replace("</", "<\\/"));
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
  /* 文件夹项与下拉 */
  .bm.folder .bm-ico { background: #4a5a72; }
  #bmfolders {
    position: fixed; z-index: 180; display: none;
    min-width: 180px; max-width: 320px; max-height: 300px; overflow-y: auto;
    scrollbar-width: thin; scrollbar-color: #5f6368 transparent;
    padding: 4px; background: #292a2d; border: 1px solid #3c4043;
    border-radius: 8px; box-shadow: 0 8px 24px rgba(0,0,0,.5);
    font-size: 12px; color: #e8eaed;
  }
  #bmfolders.open { display: block; }
  #bmfolders::-webkit-scrollbar { width: 10px; }
  #bmfolders::-webkit-scrollbar-thumb {
    background: #5f6368; border-radius: 5px; border: 2px solid #292a2d;
  }
  #bmfolders::-webkit-scrollbar-track { background: transparent; }
  #bmfolders .fd-title { padding: 6px 10px; color: #9aa0a6; font-size: 11px; }
  #bmfolders .fd-item { padding: 6px 10px; border-radius: 6px; cursor: default;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  #bmfolders .fd-item:hover { background: #3c4043; }
  #bmfolders .fd-empty { padding: 8px 10px; color: #9aa0a6; }
  .bm-edit {
    width: 110px; height: 20px; padding: 0 6px;
    background: #202124; color: #e8eaed; font-size: 12px;
    border: 1px solid #8ab4f8; border-radius: 4px; outline: 0;
  }
  /* 书签右键菜单 */
  #bmmenu {
    position: fixed; z-index: 200; display: none;
    min-width: 130px; padding: 4px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    box-shadow: 0 6px 20px rgba(0,0,0,.5); font-size: 12px; color: #e8eaed;
  }
  #bmmenu.open { display: block; }
  #bmmenu div { padding: 7px 10px; border-radius: 6px; cursor: default; }
  #bmmenu div:hover { background: #3c4043; }
  #bmmenu div.danger:hover { background: #e81123; color: #fff; }
  #bmmenu .menu-edit { padding: 2px; }
  #bmmenu .menu-edit input { width: 150px; }
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
  /* 下载面板（F-CEF-15）：与密码面板同一套撑高机制，两个面板互斥 */
  html.dl-open, html.dl-open body { height: 100vh; }
  #dlpanel {
    display: none; position: absolute; left: 0; right: 0; top: 88px; bottom: 0;
    background: #292a2d; border-top: 1px solid #3c4043; padding: 8px 12px;
    flex-direction: column; gap: 6px; z-index: 10;
  }
  html.dl-open #dlpanel { display: flex; }
  html:not(.bookmarks-hidden) #dlpanel { top: 122px; }
  #dllist { flex: 1; overflow-y: auto; }
  .dl-row {
    display: flex; align-items: center; gap: 10px; height: 34px; padding: 0 6px;
    border-radius: 6px; font-size: 12px;
  }
  .dl-row:hover { background: #3c4043; }
  .dl-row .dl-name {
    flex: 2; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .dl-row .dl-state { flex: 0 0 92px; color: #9aa0a6; }
  .dl-row .dl-bar {
    flex: 1; height: 4px; background: #3c4043; border-radius: 2px; overflow: hidden;
  }
  .dl-row .dl-bar > i { display: block; height: 100%; width: 0; background: #8ab4f8; }
  .dl-row .dl-btn {
    flex: 0 0 auto; padding: 0 8px; height: 22px; border-radius: 11px; line-height: 22px;
    color: #8ab4f8; font-size: 12px; cursor: pointer;
  }
  .dl-row .dl-btn:hover { background: #4a4d51; }
  .dl-row .dl-btn.danger { color: #f28b82; }
  .dl-empty { color: #9aa0a6; padding: 12px 6px; font-size: 12px; }
  /* 主菜单（F-CEF-16）：右上角下拉，样式对齐 Chrome 深色菜单 */
  html.menu-open, html.menu-open body { height: 100vh; }
  #menupanel {
    display: none; position: absolute; right: 8px; top: 88px; width: 240px;
    background: #292a2d; border: 1px solid #3c4043; border-radius: 8px;
    padding: 6px; z-index: 20; box-shadow: 0 8px 24px rgba(0,0,0,.5);
  }
  html.menu-open #menupanel { display: block; }
  html:not(.bookmarks-hidden) #menupanel { top: 122px; }
  .menu-item {
    display: flex; align-items: center; gap: 10px; height: 30px; padding: 0 10px;
    border-radius: 4px; font-size: 12px; color: #e8eaed; cursor: default;
  }
  .menu-item:hover { background: #3c4043; }
  .menu-item.disabled { color: #5f6368; pointer-events: none; }
  .menu-item .menu-key { margin-left: auto; color: #9aa0a6; font-size: 11px; }
  .menu-sep { height: 1px; background: #3c4043; margin: 6px 0; }
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
    <button title="下载" id="dlbtn">
      <svg viewBox="0 0 24 24"><path d="M12 3v12"/><path d="M7 10l5 5 5-5"/><path d="M4 21h16"/></svg>
    </button>
    <button title="菜单" id="menubtn">
      <svg viewBox="0 0 24 24"><circle cx="12" cy="5" r="1.6"/><circle cx="12" cy="12" r="1.6"/><circle cx="12" cy="19" r="1.6"/></svg>
    </button>
  </div>
  <div class="bookmarks" id="bookmarks"></div>
  <div id="bmfolders"></div>
  <div id="bmmenu"></div>
  <div id="pwpanel">
    <input id="pwsearch" type="text" spellcheck="false" placeholder="搜索站点或用户名">
    <div id="pwlist"></div>
  </div>
  <div id="dlpanel">
    <div id="dllist"></div>
  </div>
  <div id="menupanel">
    <div class="menu-item" data-act="tabnew">新标签页<span class="menu-key">Ctrl+T</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item disabled">历史记录<span class="menu-key">待补</span></div>
    <div class="menu-item" data-panel="dl">下载内容<span class="menu-key">Ctrl+J</span></div>
    <div class="menu-item" data-act="bmtoggle">书签栏<span class="menu-key">Ctrl+Shift+B</span></div>
    <div class="menu-item" data-panel="pw">密码<span class="menu-key"></span></div>
    <div class="menu-sep"></div>
    <div class="menu-item disabled">缩放<span class="menu-key">待补</span></div>
    <div class="menu-item disabled">打印<span class="menu-key">待补</span></div>
    <div class="menu-item disabled">查找<span class="menu-key">待补</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item disabled">设置<span class="menu-key">待补</span></div>
    <div class="menu-item disabled">关于 Shift<span class="menu-key">待补</span></div>
    <div class="menu-sep"></div>
    <div class="menu-item" data-act="winclose">退出<span class="menu-key"></span></div>
  </div>
  <script id="bm-data" type="application/json">__SHIFT_BOOKMARKS_JSON__</script>
  <script id="pw-data" type="application/json">__SHIFT_PASSWORDS_JSON__</script>
  <script id="dl-data" type="application/json">__SHIFT_DOWNLOADS_JSON__</script>


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
  window.__shiftBookmarks = function (data) {
    // 新格式 {folders,bookmarks}；旧格式（纯数组）也能吃
    if (Array.isArray(data)) { renderBookmarks({ folders: [], bookmarks: data }); return; }
    renderBookmarks(data);
  };

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

  // 内联改标题（CEF 的 Views 模式没有 window.prompt，只能自己画输入框）
  function editBookmarkTitle(item) {
    const nameEl = item.querySelector('.bm-name');
    if (!nameEl || item.querySelector('.bm-edit')) return;
    const old = nameEl.textContent;
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.value = old;
    input.spellcheck = false;
    nameEl.replaceWith(input);
    input.focus();
    input.select();
    let done = false;
    const finish = (save) => {
      if (done) return;
      done = true;
      const v = input.value.trim();
      const span = document.createElement('span');
      span.className = 'bm-name';
      span.textContent = save && v ? v : old;
      input.replaceWith(span);
      if (save && v && v !== old) {
        act('bmedit|' + JSON.stringify({ id: item.dataset.id, title: v }));
      }
    };
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); finish(true); }
      else if (e.key === 'Escape') { e.preventDefault(); finish(false); }
    });
    input.addEventListener('blur', () => finish(false));
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  // ---- 书签右键菜单（F-CEF-13）----
  const bmmenu = document.getElementById('bmmenu');
  let bmMenuTarget = null;
  let bmMenuPos = { x: 0, y: 0 };

  function hideBmMenu() {
    bmmenu.classList.remove('open');
    bmMenuTarget = null;
  }

  // 菜单里就地输入文件夹名（CEF 没有 window.prompt，只能自己画）
  function newFolderPrompt() {
    bmmenu.innerHTML = '';
    const box = document.createElement('div');
    box.className = 'menu-edit';
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.placeholder = '文件夹名称';
    input.spellcheck = false;
    box.appendChild(input);
    bmmenu.appendChild(box);
    input.focus();
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const v = input.value.trim();
        hideBmMenu();
        if (v) act('bmnewfolder|' + v);
      } else if (e.key === 'Escape') {
        e.preventDefault();
        hideBmMenu();
      }
    });
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  document.getElementById('bookmarks').addEventListener('contextmenu', (e) => {
    e.preventDefault();
    const item = e.target.closest('.bm');
    bmMenuTarget = item;
    bmMenuPos = { x: e.clientX, y: e.clientY };
    let rows;
    if (!item) {
      rows = [
        ['新建书签（当前页）', () => act('bmadd')],
        ['新建文件夹', () => newFolderPrompt()],
      ];
    } else if (item.classList.contains('folder')) {
      const fid = Number(item.dataset.folder);
      rows = [
        ['打开文件夹', () => showFolderMenu({ id: fid, name: item.querySelector('.bm-name').textContent }, item)],
        ['在此新建子文件夹', () => newSubFolderPrompt(fid)],
      ];
    } else {
      rows = [
        ['打开', () => act('nav|' + item.dataset.url)],
        ['编辑标题', () => editBookmarkTitle(item)],
        ['移到文件夹', () => showMoveMenu(item.dataset.id, bmMenuPos)],
        ['删除', () => act('bmdel|' + item.dataset.id), 'danger'],
      ];
    }
    bmmenu.innerHTML = '';
    rows.forEach(([label, fn, cls]) => {
      const d = document.createElement('div');
      d.textContent = label;
      if (cls) d.className = cls;
      d.addEventListener('click', () => { hideBmMenu(); fn(); });
      bmmenu.appendChild(d);
    });
    bmmenu.style.left = e.clientX + 'px';
    bmmenu.style.top = e.clientY + 'px';
    bmmenu.classList.add('open');
  });

  document.addEventListener('click', (e) => {
    if (!e.target.closest('#bmmenu')) hideBmMenu();
  });

  // 「移到文件夹」选择面板（复用 #bmfolders 面板，位置沿用右键点）
  function showMoveMenu(bmId, pos) {
    const panel = document.getElementById('bmfolders');
    panel.innerHTML = '';
    const head = document.createElement('div');
    head.className = 'fd-title';
    head.textContent = '移到文件夹';
    panel.appendChild(head);
    const mk = (label, folderId) => {
      const d = document.createElement('div');
      d.className = 'fd-item';
      d.textContent = label;
      d.addEventListener('click', () => {
        hideFolderMenu();
        act('bmmove|' + JSON.stringify({ id: Number(bmId), folderId }));
      });
      panel.appendChild(d);
    };
    mk('— 书签栏（不分组）', null);
    // 按父子关系展开，缩进表示层级
    const kids = new Map();
    (bmData.folders || []).forEach((f) => {
      const k = f.parentId === null || f.parentId === undefined ? 'root' : f.parentId;
      if (!kids.has(k)) kids.set(k, []);
      kids.get(k).push(f);
    });
    const walk = (key, depth) => {
      (kids.get(key) || []).forEach((f) => {
        mk('　'.repeat(depth) + f.name, f.id);
        walk(f.id, depth + 1);
      });
    };
    walk('root', 0);
    panel.style.left = Math.min(pos.x, window.innerWidth - 340) + 'px';
    panel.style.top = Math.min(pos.y + 6, window.innerHeight - 380) + 'px';
    panel.classList.add('open');
    act('bmfoldopen|' + (Math.min(panel.scrollHeight, 300) + 20));
  }

  // 在指定文件夹下新建子文件夹
  function newSubFolderPrompt(parentId) {
    bmmenu.innerHTML = '';
    const box = document.createElement('div');
    box.className = 'menu-edit';
    const input = document.createElement('input');
    input.className = 'bm-edit';
    input.placeholder = '子文件夹名称';
    input.spellcheck = false;
    box.appendChild(input);
    bmmenu.appendChild(box);
    input.focus();
    input.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') {
        e.preventDefault();
        const v = input.value.trim();
        hideBmMenu();
        if (v) act('bmnewfolder|' + JSON.stringify({ name: v, parentId }));
      } else if (e.key === 'Escape') {
        e.preventDefault();
        hideBmMenu();
      }
    });
    input.addEventListener('click', (e) => e.stopPropagation());
  }

  // 书签栏渲染：未分组书签平铺 + 顶层文件夹（点击展开下拉）
  // 数据格式 { folders: [{id,name,parentId}], bookmarks: [{id,title,url,folderId}] }
  const bmfolders = document.getElementById('bmfolders');
  let bmData = { folders: [], bookmarks: [] };

  function hideFolderMenu() {
    if (!bmfolders.classList.contains('open')) return; // 避免每次点页面都往 Rust 发消息
    bmfolders.classList.remove('open');
    act('bmfoldclose');
  }

  function bmItemHtml(b) {
    const el = document.createElement('div');
    el.className = 'bm';
    el.dataset.url = b.url || '';
    el.dataset.id = String(b.id || '');
    const ico = document.createElement('span');
    ico.className = 'bm-ico';
    const title = b.title || b.url || '书签';
    ico.textContent = title.trim().charAt(0).toUpperCase();
    const name = document.createElement('span');
    name.className = 'bm-name';
    name.textContent = title;
    el.appendChild(ico);
    el.appendChild(name);
    return el;
  }

  function renderBookmarks(data) {
    const bar = document.getElementById('bookmarks');
    if (!bar || !data || !Array.isArray(data.bookmarks)) return;
    bmData = data;
    bar.innerHTML = '';
    const topFolders = (data.folders || []).filter((f) => f.parentId === null || f.parentId === undefined);
    // 所有层级的文件夹 id：子文件夹里的书签也只在下拉里出现，不平铺
    const allFolderIds = new Set((data.folders || []).map((f) => f.id));
    data.bookmarks
      .filter((b) => b.folderId === null || b.folderId === undefined || !allFolderIds.has(b.folderId))
      .forEach((b) => bar.appendChild(bmItemHtml(b)));
    // 顶层文件夹
    topFolders.forEach((f) => {
      const el = document.createElement('div');
      el.className = 'bm folder';
      el.dataset.folder = String(f.id);
      const ico = document.createElement('span');
      ico.className = 'bm-ico';
      ico.textContent = '📁';
      const name = document.createElement('span');
      name.className = 'bm-name';
      name.textContent = f.name;
      el.appendChild(ico);
      el.appendChild(name);
      el.addEventListener('click', () => showFolderMenu(f, el));
      bar.appendChild(el);
    });
  }

  function showFolderMenu(folder, anchorEl) {
    // 递归收集该文件夹及子文件夹里的书签
    const ids = new Set([folder.id]);
    let grew = true;
    while (grew) {
      grew = false;
      (bmData.folders || []).forEach((f) => {
        if (f.parentId && ids.has(f.parentId) && !ids.has(f.id)) { ids.add(f.id); grew = true; }
      });
    }
    const items = (bmData.bookmarks || []).filter((b) => ids.has(b.folderId));
    bmfolders.innerHTML = '';
    const head = document.createElement('div');
    head.className = 'fd-title';
    head.textContent = folder.name + '（' + items.length + '）';
    bmfolders.appendChild(head);
    if (items.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'fd-empty';
      empty.textContent = '这个文件夹是空的';
      bmfolders.appendChild(empty);
    }
    items.forEach((b) => {
      const d = document.createElement('div');
      d.className = 'fd-item';
      d.textContent = b.title || b.url || '书签';
      d.title = b.url || '';
      d.addEventListener('click', () => { hideFolderMenu(); act('nav|' + b.url); });
      bmfolders.appendChild(d);
    });
    // 按文件夹元素定位（不依赖事件坐标：程序化点击时 clientX/Y 都是 0）
    const rc = anchorEl ? anchorEl.getBoundingClientRect() : { left: 0, bottom: 122 };
    bmfolders.style.left = Math.max(4, Math.min(rc.left, window.innerWidth - 330)) + 'px';
    bmfolders.style.top = (rc.bottom + 4) + 'px';
    bmfolders.classList.add('open');
    // 工具栏视图临时加高（下拉是独立视图，不撑高就被 122px 视口裁掉）。
    // 加高值按下拉实际高度算，短文件夹就不会顶出一大块空白。
    act('bmfoldopen|' + (Math.min(bmfolders.scrollHeight, 300) + 20));
  }

  document.addEventListener('click', (e) => {
    if (!e.target.closest('#bmfolders') && !e.target.closest('.bm.folder')) hideFolderMenu();
  });

  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      hideFolderMenu();
      hideBmMenu();
    }
  });

  // 启动：解析内嵌数据并渲染
  (function bootBookmarks() {
    const el = document.getElementById('bm-data');
    if (!el) return;
    let data = { folders: [], bookmarks: [] };
    try { data = JSON.parse(el.textContent || '{}'); } catch (e) { data = { folders: [], bookmarks: [] }; }
    if (Array.isArray(data)) data = { folders: [], bookmarks: data };
    renderBookmarks(data);
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
    if (open) document.documentElement.classList.remove('dl-open');
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

  // ---- 下载面板（F-CEF-15）----
  // 数据只含 id/文件名/状态/字节数；打开目录/重下/删记录都回 Rust 执行，
  // 页面拿不到真实路径（减少一个可被页面脚本滥用的面）。
  let dlList = [];
  try { dlList = JSON.parse(document.getElementById('dl-data').textContent || '[]'); } catch (e) { dlList = []; }
  if (!Array.isArray(dlList)) dlList = [];
  const dlListEl = document.getElementById('dllist');

  function dlPct(d) {
    if (!d || !d.total || d.total <= 0) return 0;
    return Math.min(100, Math.round((d.received || 0) * 100 / d.total));
  }

  function dlStateText(d) {
    switch (d.state) {
      case 'in_progress': return '下载中 ' + dlPct(d) + '%';
      case 'completed': return '已完成';
      case 'failed': return '失败';
      case 'cancelled': return '已取消';
      case 'interrupted': return '已中断';
      default: return d.state || '';
    }
  }

  function renderDownloads() {
    dlListEl.innerHTML = '';
    if (dlList.length === 0) {
      const e = document.createElement('div');
      e.className = 'dl-empty';
      e.textContent = '还没有下载记录';
      dlListEl.appendChild(e);
      return;
    }
    dlList.forEach((d) => {
      const row = document.createElement('div');
      row.className = 'dl-row';
      row.dataset.id = String(d.id);
      const name = document.createElement('span');
      name.className = 'dl-name';
      name.textContent = d.filename || '(未命名)';
      const st = document.createElement('span');
      st.className = 'dl-state';
      st.textContent = dlStateText(d);
      const bar = document.createElement('div');
      bar.className = 'dl-bar';
      const fill = document.createElement('i');
      fill.style.width = dlPct(d) + '%';
      bar.appendChild(fill);
      const open = document.createElement('span');
      open.className = 'dl-btn';
      open.dataset.op = 'open';
      open.textContent = '打开目录';
      const retry = document.createElement('span');
      retry.className = 'dl-btn';
      retry.dataset.op = 'retry';
      retry.textContent = '重下';
      const del = document.createElement('span');
      del.className = 'dl-btn danger';
      del.dataset.op = 'del';
      del.textContent = '删除记录';
      if (d.state === 'in_progress') retry.style.display = 'none';
      row.append(name, st, bar, open, retry, del);
      dlListEl.appendChild(row);
    });
  }

  function setDlPanel(open) {
    document.documentElement.classList.toggle('dl-open', open);
    if (open) document.documentElement.classList.remove('pw-open');
    act('dlpanel|' + (open ? '1' : '0'));
    if (open) renderDownloads();
  }
  document.getElementById('dlbtn').addEventListener('click', () => {
    setDlPanel(!document.documentElement.classList.contains('dl-open'));
  });
  dlListEl.addEventListener('click', (e) => {
    const btn = e.target.closest('.dl-btn');
    const row = e.target.closest('.dl-row');
    if (!btn || !row) return;
    const id = row.dataset.id;
    if (btn.dataset.op === 'open') {
      act('dlopen|' + id);
    } else if (btn.dataset.op === 'retry') {
      act('dlretry|' + id);
    } else if (btn.dataset.op === 'del') {
      if (btn.dataset.armed === '1') {
        act('dldel|' + id);
        dlList = dlList.filter((d) => String(d.id) !== id);
        renderDownloads();
      } else {
        btn.dataset.armed = '1';
        btn.textContent = '确认删除';
        setTimeout(() => { btn.dataset.armed = ''; btn.textContent = '删除记录'; }, 3000);
      }
    }
  });
  // Rust 推送最新列表（下载登记/进度/终态时调用）
  window.__shiftDownloads = (list) => {
    if (!Array.isArray(list)) return;
    dlList = list;
    if (document.documentElement.classList.contains('dl-open')) renderDownloads();
  };

  // ---- 主菜单（F-CEF-16）----
  function setMenuPanel(open) {
    document.documentElement.classList.toggle('menu-open', open);
    if (open) document.documentElement.classList.remove('pw-open', 'dl-open');
    act('menupanel|' + (open ? '1' : '0'));
  }
  document.getElementById('menubtn').addEventListener('click', (e) => {
    e.stopPropagation();
    setMenuPanel(!document.documentElement.classList.contains('menu-open'));
  });
  document.getElementById('menupanel').addEventListener('click', (e) => {
    const item = e.target.closest('.menu-item');
    if (!item || item.classList.contains('disabled')) return;
    setMenuPanel(false);
    if (item.dataset.panel === 'dl') { setDlPanel(true); return; }
    if (item.dataset.panel === 'pw') { setPwPanel(true); return; }
    act(item.dataset.act);
  });
  document.addEventListener('click', (e) => {
    if (!e.target.closest('#menupanel') && !e.target.closest('#menubtn')) setMenuPanel(false);
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') setMenuPanel(false);
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
            r#"[{"id":7,"filename":"a.zip","state":"completed","total":10,"received":10}]"#,
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
        assert!(body.contains("dlpanel"), "工具栏 HTML 必须含下载面板");
        assert!(body.contains("menupanel"), "工具栏 HTML 必须含主菜单");
        assert!(
            body.contains(r#""filename":"a.zip""#),
            "下载记录 JSON 必须嵌进页面"
        );
        assert!(
            body.contains(r#""domain":"a.com<\/script>""#),
            "密码 JSON 里的 </ 必须转义，防提前闭合 script"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

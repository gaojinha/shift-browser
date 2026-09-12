// 新标签页（原版左侧栏面板生态迁移）
// 方案：Rust 读数据库渲染成完整 HTML，写到 app_data/newtab/index.html，
// 标签用 file:// 打开。面板静态数据（最近访问/书签/快捷访问）渲染时嵌入，
// 交互状态（待办勾选）用 localStorage 存本地，无需 IPC。
use crate::db::Db;
use tauri::{AppHandle, Manager, State};

/// 新标签页文件路径（app_data_dir/newtab/index.html）
pub fn newtab_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    Some(dir.join("newtab").join("index.html"))
}

/// 新标签页 URL（file:// 绝对路径），文件不存在则先生成
pub async fn newtab_url(app: &AppHandle) -> Option<String> {
    let path = newtab_path(app)?;
    if !path.exists() {
        let db = app.state::<Db>();
        let _ = generate(&db, &path).await;
    }
    Some(format!("file://{}", path.display()))
}

/// 默认快捷访问站点（原版 DEFAULT_QUICK_SITES 精简版，可被 settings.quick_sites 覆盖）
const DEFAULT_QUICK_SITES: &[(&str, &str, &str)] = &[
    ("", "必应", "https://www.bing.com"),
    ("📰", "新浪", "https://www.sina.com.cn"),
    ("🐧", "腾讯", "https://www.qq.com"),
    ("🔎", "百度", "https://www.baidu.com"),
    ("📺", "B站", "https://www.bilibili.com"),
    ("🛒", "京东", "https://www.jd.com"),
    ("💬", "微博", "https://weibo.com"),
    ("🧭", "世界导航", "https://www.hao123.com"),
];

/// 生成新标签页 HTML 并写盘。返回文件路径字符串。
pub async fn generate(db: &Db, path: &std::path::Path) -> Result<String, String> {
    // ---- 数据：最近访问 ----
    let recent: Vec<(String, String)> = sqlx::query_as(
        "SELECT url, COALESCE(title, '') FROM history ORDER BY last_visit_at DESC LIMIT 30",
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;

    // ---- 数据：书签（扁平列表）----
    let bookmarks: Vec<(String, String)> =
        sqlx::query_as("SELECT title, url FROM bookmarks ORDER BY sort_order, id")
            .fetch_all(&db.pool)
            .await
            .map_err(|e| e.to_string())?;

    // ---- 数据：设置（搜索引擎 / 快捷访问）----
    async fn setting(db: &Db, key: &str) -> Option<String> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&db.pool)
                .await
                .ok()
                .flatten();
        row.map(|r| r.0)
    }
    let search_engine = setting(db, "search_engine")
        .await
        .unwrap_or_else(|| "bing".into());
    let search_url = match search_engine.as_str() {
        "baidu" => "https://www.baidu.com/s?wd=",
        "google" => "https://www.google.com/search?q=",
        _ => "https://www.bing.com/search?q=",
    };
    let quick_sites: Vec<(String, String, String)> = match setting(db, "quick_sites").await {
        Some(j) => serde_json::from_str::<Vec<serde_json::Value>>(&j)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let icon = v.get("icon")?.as_str()?.to_string();
                        let name = v.get("name")?.as_str()?.to_string();
                        let url = v.get("url")?.as_str()?.to_string();
                        Some((icon, name, url))
                    })
                    .collect()
            })
            .unwrap_or_else(|_| default_quick_sites()),
        None => default_quick_sites(),
    };

    // ---- 渲染 HTML ----
    let html = render_html(search_url, &recent, &bookmarks, &quick_sites);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, html).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

fn default_quick_sites() -> Vec<(String, String, String)> {
    DEFAULT_QUICK_SITES
        .iter()
        .map(|(i, n, u)| (i.to_string(), n.to_string(), u.to_string()))
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_html(
    search_url: &str,
    recent: &[(String, String)],
    bookmarks: &[(String, String)],
    quick_sites: &[(String, String, String)],
) -> String {
    let recent_html = if recent.is_empty() {
        "<div class=\"empty\">暂无访问记录</div>".to_string()
    } else {
        recent
            .iter()
            .map(|(u, t)| {
                let title = if t.is_empty() { u.as_str() } else { t.as_str() };
                format!(
                    "<a class=\"item\" href=\"{}\" title=\"{}\"><span class=\"it-title\">{}</span><span class=\"it-url\">{}</span></a>",
                    esc(u), esc(u), esc(title), esc(u)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let bookmark_html = if bookmarks.is_empty() {
        "<div class=\"empty\">暂无书签</div>".to_string()
    } else {
        bookmarks
            .iter()
            .map(|(t, u)| {
                format!(
                    "<a class=\"item\" href=\"{}\"><span class=\"it-title\">🔖 {}</span><span class=\"it-url\">{}</span></a>",
                    esc(u), esc(t), esc(u)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };

    let quick_panel = if quick_sites.is_empty() {
        "<div class=\"empty\">暂无快捷访问</div>".to_string()
    } else {
        quick_sites
            .iter()
            .map(|(_, n, u)| {
                format!(
                    "<a class=\"item\" href=\"{}\"><span class=\"it-title\">⚡ {}</span><span class=\"it-url\">{}</span></a>",
                    esc(u), esc(n), esc(u)
                )
            })
            .collect::<Vec<_>>()
            .join("")
    };
    let quick_grid = quick_sites
        .iter()
        .map(|(i, n, u)| {
            format!(
                "<a class=\"grid-item\" href=\"{}\"><span class=\"gi-icon\">{}</span><span class=\"gi-name\">{}</span></a>",
                esc(u), esc(i), esc(n)
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>新标签页</title>
<style>
  :root {{ --bg:#1a1a1a; --bg2:#232323; --bg3:#2a2a2a; --fg:#e0e0e0; --muted:#888; --accent:#4a9eff; --border:#333; }}
  * {{ box-sizing:border-box; margin:0; padding:0; }}
  html,body {{ height:100%; }}
  body {{ background:var(--bg); color:var(--fg); font:14px/1.5 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif; display:flex; overflow:hidden; }}
  .icon-bar {{ width:44px; background:var(--bg2); border-right:1px solid var(--border); display:flex; flex-direction:column; align-items:center; padding:8px 0; gap:4px; flex-shrink:0; }}
  .icon-btn {{ width:36px; height:36px; display:flex; align-items:center; justify-content:center; font-size:18px; border:none; background:transparent; border-radius:8px; cursor:pointer; text-decoration:none; color:var(--fg); }}
  .icon-btn:hover {{ background:rgba(255,255,255,.1); }}
  .icon-btn.active {{ background:rgba(255,255,255,.16); box-shadow:inset 0 0 0 2px rgba(255,255,255,.3); }}
  .icon-btn.bottom {{ margin-top:auto; }}
  .side-panel {{ width:0; overflow:hidden; background:var(--bg2); border-right:1px solid var(--border); transition:width .18s ease; flex-shrink:0; display:flex; flex-direction:column; }}
  .side-panel.open {{ width:300px; }}
  .panel-head {{ display:flex; align-items:center; justify-content:space-between; padding:10px 12px; border-bottom:1px solid var(--border); flex-shrink:0; }}
  .panel-head h2 {{ font-size:14px; font-weight:600; white-space:nowrap; }}
  .fs-close {{ border:none; background:transparent; color:var(--muted); font-size:18px; cursor:pointer; padding:0 4px; }}
  .fs-close:hover {{ color:var(--fg); }}
  .panel-body {{ overflow-y:auto; padding:8px; flex:1; }}
  .item {{ display:block; padding:8px 10px; border-radius:8px; text-decoration:none; color:var(--fg); }}
  .item:hover {{ background:var(--bg3); }}
  .it-title {{ display:block; font-size:13px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .it-url {{ display:block; font-size:11px; color:var(--muted); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .empty {{ color:var(--muted); text-align:center; padding:24px 0; font-size:13px; }}
  .todo-panel {{ padding:4px; }}
  .todo-add {{ display:flex; gap:6px; margin-bottom:8px; }}
  .todo-add input {{ flex:1; background:var(--bg3); border:1px solid var(--border); color:var(--fg); border-radius:6px; padding:6px 8px; font-size:13px; outline:none; }}
  .todo-add button {{ background:var(--accent); color:#fff; border:none; border-radius:6px; padding:6px 12px; cursor:pointer; font-size:13px; }}
  .todo-item {{ display:flex; align-items:center; gap:8px; padding:6px 8px; border-radius:6px; }}
  .todo-item:hover {{ background:var(--bg3); }}
  .todo-item input[type=checkbox] {{ accent-color:var(--accent); width:15px; height:15px; flex-shrink:0; }}
  .todo-item.done .todo-text {{ text-decoration:line-through; color:var(--muted); }}
  .todo-text {{ flex:1; font-size:13px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .todo-del {{ border:none; background:transparent; color:var(--muted); cursor:pointer; font-size:13px; }}
  .todo-del:hover {{ color:#ff6b6b; }}
  .container {{ flex:1; display:flex; flex-direction:column; align-items:center; padding:8vh 24px 24px; overflow-y:auto; }}
  .greeting {{ font-size:30px; font-weight:600; margin-bottom:6px; }}
  .date {{ color:var(--muted); font-size:13px; margin-bottom:28px; }}
  .search {{ display:flex; width:min(560px,90%); margin-bottom:36px; }}
  .search input {{ flex:1; background:var(--bg3); border:1px solid var(--border); border-right:none; border-radius:10px 0 0 10px; color:var(--fg); padding:12px 16px; font-size:15px; outline:none; }}
  .search input:focus {{ border-color:var(--accent); }}
  .search button {{ background:var(--accent); color:#fff; border:none; border-radius:0 10px 10px 0; padding:0 22px; font-size:15px; cursor:pointer; }}
  .grid {{ display:grid; grid-template-columns:repeat(auto-fill,minmax(96px,1fr)); gap:12px; width:min(720px,92%); }}
  .grid-item {{ display:flex; flex-direction:column; align-items:center; gap:8px; padding:16px 8px; background:var(--bg2); border:1px solid var(--border); border-radius:12px; text-decoration:none; color:var(--fg); }}
  .grid-item:hover {{ border-color:var(--accent); }}
  .gi-icon {{ font-size:26px; }}
  .gi-name {{ font-size:12px; color:var(--muted); }}
</style>
</head>
<body>
  <div class="icon-bar">
    <button class="icon-btn" data-panel="bookmark" title="书签">🔖</button>
    <button class="icon-btn" data-panel="todo" title="待办">📝</button>
    <button class="icon-btn" data-panel="recent" title="最近访问">🕘</button>
    <button class="icon-btn" data-panel="quick" title="快捷访问">⚡</button>
    <button class="icon-btn" data-panel="feed" title="资讯（迁移中）">📰</button>
    <button class="icon-btn bottom" data-panel="profile" title="我的">👤</button>
  </div>
  <div class="side-panel" id="sidePanel">
    <div class="panel-head">
      <h2 id="panelTitle">🔖 书签</h2>
      <button class="fs-close" id="panelClose" title="关闭">×</button>
    </div>
    <div class="panel-body" id="panelBody"></div>
  </div>
  <div class="container">
    <div class="greeting" id="greeting"></div>
    <div class="date" id="dateStr"></div>
    <form class="search" id="searchForm">
      <input id="searchInput" placeholder="搜索或输入网址" autofocus autocomplete="off" spellcheck="false">
      <button type="submit">搜索</button>
    </form>
    <div class="grid">{quick_grid}</div>
  </div>
<script>
(function() {{
  // 问候语 + 日期（JS 动态生成，省得后端依赖时区库）
  var now = new Date();
  var h = now.getHours();
  var greet = h < 5 ? '夜深了' : h < 11 ? '早上好' : h < 13 ? '中午好' : h < 18 ? '下午好' : '晚上好';
  document.getElementById('greeting').textContent = greet + '，高哥';
  var wd = ['日','一','二','三','四','五','六'][now.getDay()];
  document.getElementById('dateStr').textContent =
    now.getFullYear() + '年' + (now.getMonth()+1) + '月' + now.getDate() + '日 星期' + wd;

  var SEARCH_BASE = {search_url_json};
  var PANEL_DATA = {{
    bookmark: {{ title: '🔖 书签', html: {bookmark_html_json} }},
    recent: {{ title: '🕘 最近访问', html: {recent_html_json} }},
    quick: {{ title: '⚡ 快捷访问', html: {quick_panel_json} }},
    feed: {{ title: '📰 资讯', html: '<div class="empty">RSS 资讯迁移中，下一版上线</div>' }}
  }};
  var panel = document.getElementById('sidePanel');
  var panelTitle = document.getElementById('panelTitle');
  var panelBody = document.getElementById('panelBody');
  var current = null;

  // ---- 待办（localStorage 持久化）----
  function loadTodos() {{ try {{ return JSON.parse(localStorage.getItem('sb_todos') || '[]'); }} catch(e) {{ return []; }} }}
  function saveTodos(t) {{ localStorage.setItem('sb_todos', JSON.stringify(t)); }}
  function renderTodos() {{
    var todos = loadTodos();
    var h2 = '<div class="todo-panel"><div class="todo-add"><input id="todoInput" placeholder="新增待办，回车添加"><button id="todoAddBtn">加</button></div>';
    if (!todos.length) h2 += '<div class="empty">暂无待办</div>';
    todos.forEach(function(t, i) {{
      h2 += '<div class="todo-item' + (t.done ? ' done' : '') + '"><input type="checkbox" data-i="' + i + '"' + (t.done ? ' checked' : '') + '><span class="todo-text">' + t.text.replace(/</g,'&lt;') + '</span><button class="todo-del" data-i="' + i + '">✕</button></div>';
    }});
    h2 += '</div>';
    return h2;
  }}
  function bindTodos() {{
    var input = document.getElementById('todoInput');
    var addBtn = document.getElementById('todoAddBtn');
    function add() {{
      var v = input.value.trim();
      if (!v) return;
      var todos = loadTodos();
      todos.unshift({{ text: v, done: false }});
      saveTodos(todos);
      panelBody.innerHTML = renderTodos();
      bindTodos();
    }}
    if (addBtn) addBtn.addEventListener('click', add);
    if (input) input.addEventListener('keydown', function(e) {{ if (e.key === 'Enter') add(); }});
    panelBody.querySelectorAll('.todo-item input[type=checkbox]').forEach(function(cb) {{
      cb.addEventListener('change', function() {{
        var todos = loadTodos();
        todos[+cb.dataset.i].done = cb.checked;
        saveTodos(todos);
        panelBody.innerHTML = renderTodos();
        bindTodos();
      }});
    }});
    panelBody.querySelectorAll('.todo-del').forEach(function(btn) {{
      btn.addEventListener('click', function() {{
        var todos = loadTodos();
        todos.splice(+btn.dataset.i, 1);
        saveTodos(todos);
        panelBody.innerHTML = renderTodos();
        bindTodos();
      }});
    }});
  }}

  // ---- "我的"面板 ----
  function renderProfile() {{
    var todos = loadTodos();
    var open = todos.filter(function(t) {{ return !t.done; }}).length;
    var h2 = '<div class="item"><span class="it-title">📚 书签 ' + {bookmark_count} + ' 条</span></div>';
    h2 += '<div class="item"><span class="it-title">🕘 历史 ' + {recent_count} + ' 条</span></div>';
    h2 += '<div class="item"><span class="it-title">📝 待办未完成 ' + open + ' 条</span></div>';
    h2 += '<div class="item"><span class="it-title">🌐 ShiftBrowser v5.5.0</span></div>';
    return h2;
  }}

  function switchPanel(name) {{
    var btn = document.querySelector('.icon-btn[data-panel="' + name + '"]');
    if (current === name) {{ closePanel(); return; }}
    var cfg;
    if (name === 'todo') cfg = {{ title: '📝 待办清单', html: renderTodos(), bind: bindTodos }};
    else if (name === 'profile') cfg = {{ title: '👤 我的', html: renderProfile() }};
    else cfg = PANEL_DATA[name];
    if (!cfg) return;
    panelTitle.textContent = cfg.title;
    panelBody.innerHTML = cfg.html;
    if (cfg.bind) cfg.bind();
    panel.classList.add('open');
    document.querySelectorAll('.icon-btn[data-panel]').forEach(function(b) {{ b.classList.remove('active'); }});
    if (btn) btn.classList.add('active');
    current = name;
  }}
  function closePanel() {{
    panel.classList.remove('open');
    document.querySelectorAll('.icon-btn[data-panel]').forEach(function(b) {{ b.classList.remove('active'); }});
    current = null;
  }}
  document.querySelector('.icon-bar').addEventListener('click', function(e) {{
    var btn = e.target.closest('.icon-btn[data-panel]');
    if (btn) switchPanel(btn.dataset.panel);
  }});
  document.getElementById('panelClose').addEventListener('click', closePanel);

  // ---- 搜索框 ----
  document.getElementById('searchForm').addEventListener('submit', function(e) {{
    e.preventDefault();
    var v = document.getElementById('searchInput').value.trim();
    if (!v) return;
    var url;
    if (/^https?:\/\//i.test(v)) url = v;
    else if (/^[\w-]+(\.[\w-]+)+/.test(v) && !/\s/.test(v)) url = 'https://' + v;
    else url = SEARCH_BASE + encodeURIComponent(v);
    window.open(url, '_blank');
  }});
}})();
</script>
</body>
</html>"#,
        quick_grid = quick_grid,
        search_url_json = serde_json::json!(search_url),
        bookmark_html_json = serde_json::json!(bookmark_html),
        recent_html_json = serde_json::json!(recent_html),
        quick_panel_json = serde_json::json!(quick_panel),
        bookmark_count = bookmarks.len(),
        recent_count = recent.len(),
    )
}

/// 生成/刷新新标签页（改设置或数据变化后可调，前端也可定时刷）
#[tauri::command]
pub async fn refresh_newtab(db: State<'_, Db>, app: AppHandle) -> Result<String, String> {
    let path = newtab_path(&app).ok_or("无法获取数据目录")?;
    generate(&db, &path).await
}

/// 取新标签页 URL（不存在则先生成）
#[tauri::command]
pub async fn get_newtab_url(app: AppHandle) -> Result<String, String> {
    newtab_url(&app)
        .await
        .ok_or_else(|| "无法生成新标签页".to_string())
}

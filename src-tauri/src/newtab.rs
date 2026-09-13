// 新标签页（原版左侧栏面板生态迁移）
// 方案：Rust 读数据库渲染成完整 HTML，写到 app_data/newtab/index.html，
// 标签用 file:// 打开。面板静态数据（最近访问/书签/快捷访问）渲染时嵌入，
// 交互状态（待办勾选）用 localStorage 存本地，无需 IPC。
use crate::db::Db;
use tauri::{AppHandle, Manager, State};

/// 世界导航数据（32 大类 721 站，从原版 worldNav.ts 移植）
const WORLDNAV_JSON: &str = include_str!("../resources/worldnav.json");

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

    // ---- 渲染 HTML（启动页 + 世界导航页）----
    let html = render_html(search_url, &recent, &bookmarks, &quick_sites);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        std::fs::write(parent.join("world-nav.html"), render_worldnav_html())
            .map_err(|e| e.to_string())?;
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
  .date {{ color:var(--muted); font-size:13px; margin-bottom:6px; }}
  .weather {{ color:var(--muted); font-size:13px; margin-bottom:24px; min-height:18px; }}
  .weather b {{ color:var(--fg); font-weight:600; }}
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
    <a class="icon-btn" href="world-nav.html" title="世界导航">🌐</a>
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
    <div class="weather" id="weather"></div>
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

  // ---- 运城天气（Open-Meteo 免费 API，无需 key）----
  var WMO = {{0:'☀️ 晴',1:'🌤️ 基本晴',2:'⛅ 多云',3:'☁️ 阴',45:'🌫️ 雾',48:'🌫️ 冻雾',51:'🌦️ 毛毛雨',53:'🌦️ 毛毛雨',55:'🌧️ 毛毛雨',61:'🌧️ 小雨',63:'🌧️ 中雨',65:'🌧️ 大雨',71:'🌨️ 小雪',73:'🌨️ 中雪',75:'❄️ 大雪',80:'🌦️ 阵雨',81:'🌧️ 强阵雨',82:'⛈️ 暴阵雨',95:'⛈️ 雷雨',96:'⛈️ 雷雨伴冰雹',99:'⛈️ 强冰雹'}};
  fetch('https://api.open-meteo.com/v1/forecast?latitude=35.03&longitude=110.45&current=temperature_2m,weather_code,wind_speed_10m&daily=temperature_2m_max,temperature_2m_min&timezone=Asia%2FShanghai')
    .then(function(r) {{ return r.json(); }})
    .then(function(d) {{
      var c = d.current || {{}};
      var desc = WMO[c.weather_code] || '🌡️';
      var t = Math.round(c.temperature_2m);
      var lo = Math.round(d.daily.temperature_2m_min[0]), hi = Math.round(d.daily.temperature_2m_max[0]);
      document.getElementById('weather').innerHTML =
        '📍 运城 ' + desc + ' <b>' + t + '°C</b>（' + lo + '~' + hi + '°C，风 ' + Math.round(c.wind_speed_10m) + ' km/h）';
    }})
    .catch(function() {{ document.getElementById('weather').textContent = '📍 运城天气获取失败'; }});

  var SEARCH_BASE = {search_url_json};
  function loadUserFavs() {{ try {{ return JSON.parse(localStorage.getItem('sb_user_favs') || '[]'); }} catch(e) {{ return []; }} }}
  function userFavHtml() {{
    return loadUserFavs().map(function(f) {{
      return '<a class=\"item\" href=\"' + f.url + '\"><span class=\"it-title\">' + (f.icon || '⭐') + ' ' + f.name.replace(/</g,'&lt;') + '</span><span class=\"it-url\">' + f.url + '</span></a>';
    }}).join('');
  }}
  var PANEL_DATA = {{
    bookmark: {{ title: '🔖 书签', html: {bookmark_html_json} }},
    recent: {{ title: '🕘 最近访问', html: {recent_html_json} }},
    quick: {{ title: '⚡ 快捷访问', html: userFavHtml() + {quick_panel_json} }},
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

  // ---- 主区九宫格追加用户收藏（世界导航页里 ⭐ 过的）----
  (function() {{
    var favs = loadUserFavs();
    if (!favs.length) return;
    var grid = document.querySelector('.grid');
    if (!grid) return;
    grid.insertAdjacentHTML('afterbegin', favs.map(function(f) {{
      return '<a class="grid-item" href="' + f.url + '"><span class="gi-icon">' + (f.icon || '⭐') + '</span><span class="gi-name">' + f.name.replace(/</g,'&lt;') + '</span></a>';
    }}).join(''));
  }})();

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

/// 生成世界导航页 HTML（数据内嵌，纯前端交互，file:// 同源 localStorage 与启动页共享）
fn render_worldnav_html() -> String {
    let tpl = r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>世界导航</title>
<style>
  :root { --bg:#1a1a1a; --bg2:#232323; --bg3:#2a2a2a; --fg:#e0e0e0; --muted:#888; --accent:#4a9eff; --border:#333; }
  * { box-sizing:border-box; margin:0; padding:0; }
  html,body { height:100%; }
  body { background:var(--bg); color:var(--fg); font:14px/1.5 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif; display:flex; flex-direction:column; }
  .nav-header { display:flex; align-items:center; gap:16px; padding:12px 20px; background:var(--bg2); border-bottom:1px solid var(--border); flex-shrink:0; }
  .nav-title { font-size:17px; font-weight:600; white-space:nowrap; }
  .nav-title a { color:var(--muted); text-decoration:none; font-size:13px; margin-left:10px; }
  .nav-search { flex:1; max-width:420px; }
  .nav-search input { width:100%; background:var(--bg3); border:1px solid var(--border); border-radius:8px; color:var(--fg); padding:8px 12px; font-size:13px; outline:none; }
  .nav-search input:focus { border-color:var(--accent); }
  .layout { flex:1; display:flex; overflow:hidden; }
  .cat-tabs { width:150px; overflow-y:auto; background:var(--bg2); border-right:1px solid var(--border); flex-shrink:0; padding:8px 6px; }
  .cat-tab { display:flex; align-items:center; gap:8px; width:100%; padding:8px 10px; border:none; background:transparent; color:var(--fg); border-radius:8px; cursor:pointer; font-size:13px; text-align:left; margin-bottom:2px; }
  .cat-tab span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .cat-tab:hover { background:var(--bg3); }
  .cat-tab.active { background:var(--accent); color:#fff; }
  .main { flex:1; overflow-y:auto; padding:18px 22px; }
  .cat-header { margin-bottom:14px; }
  .cat-header .ch-icon { font-size:22px; margin-right:8px; }
  .cat-header .ch-title { font-size:18px; font-weight:600; }
  .cat-header .ch-desc { color:var(--muted); font-size:12px; margin-top:2px; }
  .cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:12px; }
  .site-card { position:relative; background:var(--bg2); border:1px solid var(--border); border-radius:12px; padding:14px; cursor:pointer; transition:border-color .12s; }
  .site-card:hover { border-color:var(--accent); }
  .sc-icon { font-size:26px; margin-bottom:8px; }
  .sc-name { font-size:14px; font-weight:600; margin-bottom:3px; }
  .sc-desc { font-size:12px; color:var(--muted); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-bottom:8px; }
  .sc-rating { display:flex; align-items:center; gap:6px; margin-bottom:4px; }
  .stars { position:relative; display:inline-block; font-size:12px; line-height:1; }
  .stars-bg { color:#444; }
  .stars-fill { position:absolute; left:0; top:0; color:#f5a623; overflow:hidden; white-space:nowrap; }
  .rating-num { font-size:12px; color:var(--muted); }
  .sc-meta { font-size:11px; color:var(--muted); }
  .sc-star { position:absolute; top:10px; right:10px; border:none; background:transparent; color:var(--muted); font-size:16px; cursor:pointer; }
  .sc-star:hover { color:#f5a623; }
  .sc-star.faved { color:#f5a623; }
  .sc-remove { position:absolute; top:10px; right:34px; border:none; background:transparent; color:var(--muted); font-size:13px; cursor:pointer; display:none; }
  .site-card:hover .sc-remove { display:block; }
  .empty { color:var(--muted); text-align:center; padding:60px 0; }
  .add-fav-btn { margin-bottom:14px; background:var(--bg3); color:var(--accent); border:1px dashed var(--accent); border-radius:8px; padding:8px 16px; cursor:pointer; font-size:13px; }
  .modal-mask { display:none; position:fixed; inset:0; background:rgba(0,0,0,.55); z-index:50; align-items:center; justify-content:center; }
  .modal-mask.show { display:flex; }
  .modal-box { width:360px; background:var(--bg2); border:1px solid var(--border); border-radius:12px; padding:18px; }
  .modal-box h3 { font-size:15px; margin-bottom:12px; }
  .modal-box input { width:100%; background:var(--bg3); border:1px solid var(--border); border-radius:8px; color:var(--fg); padding:8px 10px; font-size:13px; margin-bottom:10px; outline:none; }
  .modal-actions { display:flex; justify-content:flex-end; gap:8px; }
  .modal-btn { border:none; border-radius:8px; padding:7px 16px; cursor:pointer; font-size:13px; }
  .modal-btn.cancel { background:var(--bg3); color:var(--fg); }
  .modal-btn.ok { background:var(--accent); color:#fff; }
</style>
</head>
<body>
  <div class="nav-header">
    <div class="nav-title">🌐 世界导航 <a href="index.html">← 返回新标签页</a></div>
    <div class="nav-search"><input id="searchInput" placeholder="搜索 721 个网站（名称 / 描述 / 域名）" autocomplete="off" spellcheck="false"></div>
  </div>
  <div class="layout">
    <div class="cat-tabs" id="catTabs"></div>
    <div class="main" id="main"></div>
  </div>
  <div class="modal-mask" id="favModal">
    <div class="modal-box">
      <h3>添加收藏</h3>
      <input id="favName" placeholder="名称（如：永济政府网）">
      <input id="favUrl" placeholder="网址（https://...）">
      <input id="favIcon" placeholder="图标 emoji（可选，默认 ⭐）">
      <div class="modal-actions">
        <button class="modal-btn cancel" id="favCancel">取消</button>
        <button class="modal-btn ok" id="favOk">添加</button>
      </div>
    </div>
  </div>
<script>
(function() {
  var CATS = __WORLDAV_DATA__;
  // ---- 用户收藏（localStorage，与启动页共享）----
  function loadUserFavs() { try { return JSON.parse(localStorage.getItem('sb_user_favs') || '[]'); } catch(e) { return []; } }
  function saveUserFavs(f) { localStorage.setItem('sb_user_favs', JSON.stringify(f)); }
  function isFaved(url) { return loadUserFavs().some(function(x) { return x.url === url; }); }

  var current = CATS[0].id;
  var keyword = '';

  function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;'); }

  function renderTabs() {
    document.getElementById('catTabs').innerHTML = CATS.map(function(c) {
      return '<button class="cat-tab' + (c.id === current ? ' active' : '') + '" data-cat="' + c.id + '">' + c.icon + '<span>' + esc(c.title) + '</span></button>';
    }).join('');
  }

  function cardHtml(s, isUser) {
    var pct = s.r ? Math.round((parseFloat(s.r) / 5) * 100) : 0;
    var fav = isUser || s.d === '我的收藏';
    return '<div class="site-card" data-url="' + esc(s.u) + '">' +
      '<div class="sc-icon">' + (s.i || '🔗') + '</div>' +
      '<div class="sc-name">' + esc(s.n) + '</div>' +
      '<div class="sc-desc">' + esc(s.d || s.u) + '</div>' +
      (fav ? '' : '<div class="sc-rating"><span class="stars"><span class="stars-bg">★★★★★</span><span class="stars-fill" style="width:' + pct + '%">★★★★★</span></span><span class="rating-num">' + esc(s.r) + '</span></div>') +
      '<div class="sc-meta">' + (fav ? '⭐ 我的收藏' : '👥 ' + esc(s.us) + ' · ⚡ ' + esc(s.sp)) + '</div>' +
      (fav ? '<button class="sc-remove" data-url="' + esc(s.u) + '" title="移除收藏">✕</button>' : '') +
      '<button class="sc-star' + (isFaved(s.u) ? ' faved' : '') + '" data-url="' + esc(s.u) + '" data-name="' + esc(s.n) + '" data-icon="' + (s.i || '🔗') + '" title="收藏到快捷访问">' + (isFaved(s.u) ? '★' : '☆') + '</button>' +
      '</div>';
  }

  function renderMain() {
    var main = document.getElementById('main');
    var cat = CATS.find(function(c) { return c.id === current; });
    if (!cat) return;
    var sites = cat.sites.slice();
    if (cat.id === 'favorites') {
      var userFavs = loadUserFavs().map(function(f) { return { n: f.name, u: f.url, d: '我的收藏', i: f.icon || '⭐' }; });
      sites = userFavs.concat(sites);
    }
    if (keyword) {
      var k = keyword.toLowerCase();
      sites = sites.filter(function(s) {
        return (s.n + ' ' + (s.d||'') + ' ' + s.u).toLowerCase().indexOf(k) !== -1;
      });
    }
    var h = '';
    if (cat.id === 'favorites') h += '<button class="add-fav-btn" id="addFavBtn">＋ 添加收藏</button>';
    h += '<div class="cat-header"><span class="ch-icon">' + cat.icon + '</span><span class="ch-title">' + esc(cat.title) + '</span><div class="ch-desc">' + esc(cat.desc) + '</div></div>';
    h += sites.length ? '<div class="cards">' + sites.map(function(s) { return cardHtml(s, s.d === '我的收藏'); }).join('') + '</div>' : '<div class="empty">没有匹配的网站</div>';
    main.innerHTML = h;
    main.scrollTop = 0;
  }

  document.getElementById('catTabs').addEventListener('click', function(e) {
    var btn = e.target.closest('.cat-tab');
    if (!btn) return;
    current = btn.dataset.cat;
    renderTabs(); renderMain();
  });
  document.getElementById('searchInput').addEventListener('input', function(e) {
    keyword = e.target.value.trim();
    renderMain();
  });
  document.getElementById('main').addEventListener('click', function(e) {
    var star = e.target.closest('.sc-star');
    if (star) {
      e.stopPropagation();
      var favs = loadUserFavs();
      var url = star.dataset.url;
      var idx = favs.findIndex(function(f) { return f.url === url; });
      if (idx >= 0) { favs.splice(idx, 1); }
      else { favs.unshift({ name: star.dataset.name, url: url, icon: star.dataset.icon }); }
      saveUserFavs(favs);
      renderMain();
      return;
    }
    var rm = e.target.closest('.sc-remove');
    if (rm) {
      e.stopPropagation();
      var favs2 = loadUserFavs().filter(function(f) { return f.url !== rm.dataset.url; });
      saveUserFavs(favs2);
      renderMain();
      return;
    }
    if (e.target.id === 'addFavBtn') {
      document.getElementById('favModal').classList.add('show');
      document.getElementById('favName').focus();
      return;
    }
    var card = e.target.closest('.site-card');
    if (card) window.open(card.dataset.url, '_blank');
  });
  document.getElementById('favCancel').addEventListener('click', function() {
    document.getElementById('favModal').classList.remove('show');
  });
  document.getElementById('favOk').addEventListener('click', function() {
    var n = document.getElementById('favName').value.trim();
    var u = document.getElementById('favUrl').value.trim();
    var i = document.getElementById('favIcon').value.trim();
    if (!n || !u) return;
    if (!/^https?:\/\//.test(u)) u = 'https://' + u;
    var favs = loadUserFavs();
    favs.unshift({ name: n, url: u, icon: i || '⭐' });
    saveUserFavs(favs);
    document.getElementById('favModal').classList.remove('show');
    document.getElementById('favName').value = '';
    document.getElementById('favUrl').value = '';
    document.getElementById('favIcon').value = '';
    current = 'favorites';
    renderTabs(); renderMain();
  });

  renderTabs();
  renderMain();
})();
</script>
</body>
</html>"#;
    tpl.replace("__WORLDAV_DATA__", WORLDNAV_JSON)
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

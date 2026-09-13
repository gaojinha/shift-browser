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

    // ---- 数据：资讯（内置源缓存最新 80 条）----
    let feed_items = crate::feed::latest_items(db, 80).await;

    // ---- 渲染 HTML（启动页 + 世界导航页）----
    let html = render_html(search_url, &recent, &bookmarks, &quick_sites, &feed_items);

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

/// 把 JSON 序列化成可安全嵌入 <script> 的字符串。
/// serde_json 默认不转义 `<`，若数据含 `</script>` 会提前闭合标签导致 HTML 注入/XSS，
/// 这里把 `<` 转成 `\u003c`（JS 解析结果不变，但不会再闭合 script 标签）。
fn json_script_safe(v: &serde_json::Value) -> String {
    serde_json::to_string(v)
        .unwrap_or_else(|_| "[]".into())
        .replace('<', "\\u003c")
}

fn render_html(
    search_url: &str,
    recent: &[(String, String)],
    bookmarks: &[(String, String)],
    quick_sites: &[(String, String, String)],
    feed_items: &[(String, String, String, String, String)],
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

    // 快捷访问内置项传给前端 JS（动态面板用）
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

    // 资讯数据（紧凑数组 [title, url, source, published, category]）
    let feed_data: Vec<serde_json::Value> = feed_items
        .iter()
        .map(|(source, category, title, url, published)| {
            serde_json::json!([title, url, source, published, category])
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>新标签页</title>
<meta http-equiv="Content-Security-Policy" content="default-src 'self' data:; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src https: http: data:; connect-src https: http: ipc: http://ipc.localhost; frame-src https: http:">
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
  .fs-item {{ position:relative; }}
  .fs-read {{ position:absolute; right:8px; top:50%; transform:translateY(-50%); z-index:2; width:26px; height:26px; border:none; background:var(--bg3); border:1px solid var(--border); border-radius:7px; cursor:pointer; font-size:13px; display:flex; align-items:center; justify-content:center; color:var(--accent); }}
  .fs-read:hover {{ background:var(--accent); color:#fff; border-color:var(--accent); }}
  .fs-item .it-url {{ padding-right:34px; }}  /* 给 📖 让位 */
  .feed-toggle {{ display:block; margin:0 0 8px 4px; background:var(--bg3); border:1px solid var(--border); color:var(--accent); border-radius:6px; padding:4px 10px; font-size:12px; cursor:pointer; }}
  .feed-bar {{ display:flex; flex-wrap:wrap; gap:4px; margin-bottom:8px; }}
  .feed-cat {{ background:var(--bg3); border:1px solid var(--border); color:var(--muted); border-radius:10px; padding:2px 8px; font-size:11px; cursor:pointer; }}
  .feed-cat.on {{ background:var(--accent); border-color:var(--accent); color:#fff; }}
  .q-item {{ display:flex; align-items:center; }}
  .q-link {{ flex:1; min-width:0; display:block; text-decoration:none; color:inherit; overflow:hidden; }}
  .q-del {{ border:none; background:transparent; color:var(--muted); cursor:pointer; font-size:12px; flex-shrink:0; visibility:hidden; }}
  .q-item:hover .q-del {{ visibility:visible; }}
  .q-del:hover {{ color:#ff6b6b; }}
  .modal-mask {{ display:none; position:fixed; inset:0; background:rgba(0,0,0,.55); z-index:50; align-items:center; justify-content:center; }}
  .modal-mask.show {{ display:flex; }}
  .modal-box {{ width:340px; background:var(--bg2); border:1px solid var(--border); border-radius:12px; padding:18px; }}
  .modal-box h3 {{ font-size:15px; margin-bottom:12px; }}
  .modal-box input {{ width:100%; background:var(--bg3); border:1px solid var(--border); border-radius:8px; color:var(--fg); padding:8px 10px; font-size:13px; margin-bottom:10px; outline:none; }}
  .modal-actions {{ display:flex; justify-content:flex-end; gap:8px; }}
  .modal-btn {{ border:none; border-radius:8px; padding:7px 16px; cursor:pointer; font-size:13px; }}
  .modal-btn.cancel {{ background:var(--bg3); color:var(--fg); }}
  .modal-btn.ok {{ background:var(--accent); color:#fff; }}
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
  <div class="modal-mask" id="quickModal">
    <div class="modal-box">
      <h3>添加快捷访问</h3>
      <input id="qName" placeholder="名称（如：永济政府网）" autocomplete="off" spellcheck="false">
      <input id="qUrl" placeholder="网址（https://...）" autocomplete="off" spellcheck="false">
      <input id="qIcon" placeholder="图标 emoji（可选，默认 ⭐）" autocomplete="off" spellcheck="false">
      <div class="modal-actions">
        <button class="modal-btn cancel" id="qCancel">取消</button>
        <button class="modal-btn ok" id="qOk">添加</button>
      </div>
    </div>
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
  function saveUserFavs(f) {{ localStorage.setItem('sb_user_favs', JSON.stringify(f)); }}
  function loadHidden() {{ try {{ return JSON.parse(localStorage.getItem('sb_quick_hidden') || '[]'); }} catch(e) {{ return []; }} }}

  // ---- 快捷访问面板（可增删）----
  var QUICK_BUILTIN = {quick_builtin_json};
  function renderQuick() {{
    var h = '<button class=\"feed-toggle\" id=\"addQuickBtn\">＋ 添加快捷访问</button>';
    var favs = loadUserFavs();
    favs.forEach(function(f) {{
      h += '<div class=\"item q-item\"><a class=\"q-link\" href=\"' + f.url + '\"><span class=\"it-title\">' + (f.icon || '⭐') + ' ' + f.name.replace(/</g,'&lt;') + '</span><span class=\"it-url\">' + f.url + '</span></a><button class=\"q-del\" data-url=\"' + f.url + '\" title=\"删除收藏\">✕</button></div>';
    }});
    var hidden = loadHidden();
    QUICK_BUILTIN.filter(function(b) {{ return hidden.indexOf(b.u) < 0; }}).forEach(function(b) {{
      h += '<div class=\"item q-item\"><a class=\"q-link\" href=\"' + b.u + '\"><span class=\"it-title\">' + b.i + ' ' + b.n + '</span><span class=\"it-url\">' + b.u + '</span></a><button class=\"q-del\" data-hide=\"' + b.u + '\" title=\"隐藏（不删除收藏，仅收起）\">✕</button></div>';
    }});
    if (!favs.length && !QUICK_BUILTIN.filter(function(b) {{ return hidden.indexOf(b.u) < 0; }}).length) h += '<div class="empty">都藏完了，去世界导航 ⭐ 点几个</div>';
    return h;
  }}
  function bindQuick() {{
    var addBtn = document.getElementById('addQuickBtn');
    if (addBtn) addBtn.addEventListener('click', function() {{ document.getElementById('quickModal').classList.add('show'); document.getElementById('qName').focus(); }});
    panelBody.querySelectorAll('.q-del').forEach(function(btn) {{
      btn.addEventListener('click', function(e) {{
        e.preventDefault();
        e.stopPropagation();
        if (btn.dataset.hide) {{
          var hidden = loadHidden();
          hidden.push(btn.dataset.hide);
          localStorage.setItem('sb_quick_hidden', JSON.stringify(hidden));
        }} else {{
          saveUserFavs(loadUserFavs().filter(function(f) {{ return f.url !== btn.dataset.url; }}));
        }}
        panelBody.innerHTML = renderQuick();
        bindQuick();
      }});
    }});
  }}

  // ---- 资讯面板（已读 localStorage + 只看未读）----
  var FEED_DATA = {feed_data_json};
  function loadRead() {{ try {{ return JSON.parse(localStorage.getItem('sb_feed_read') || '{{}}'); }} catch(e) {{ return {{}}; }} }}
  function timeAgo(s) {{
    if (!s) return '';
    var d = new Date(s);
    if (isNaN(d.getTime())) return '';
    var mins = Math.floor((Date.now() - d.getTime()) / 60000);
    if (mins < 1) return '刚刚';
    if (mins < 60) return mins + ' 分钟前';
    if (mins < 24 * 60) return Math.floor(mins / 60) + ' 小时前';
    return Math.floor(mins / 1440) + ' 天前';
  }}
  var feedHideRead = localStorage.getItem('sb_feed_hide_read') === '1';
  var feedCat = localStorage.getItem('sb_feed_cat') || '';
  function renderFeed() {{
    var read = loadRead();
    var cats = [];
    FEED_DATA.forEach(function(f) {{ if (f[4] && cats.indexOf(f[4]) < 0) cats.push(f[4]); }});
    var items = FEED_DATA.filter(function(f) {{
      if (feedHideRead && read[f[1]]) return false;
      if (feedCat && f[4] !== feedCat) return false;
      return true;
    }});
    var h = '<div class="feed-bar"><button class=\"feed-cat' + (feedHideRead ? '' : ' on') + '\" id=\"feedHideReadBtn\">' + (feedHideRead ? '全部' : '只看未读') + '</button>';
    h += '<button class=\"feed-cat' + (!feedCat ? ' on' : '') + '\" data-cat=\"\">全部</button>';
    cats.forEach(function(c) {{
      var n = FEED_DATA.filter(function(f) {{ return f[4] === c; }}).length;
      h += '<button class=\"feed-cat' + (feedCat === c ? ' on' : '') + '\" data-cat=\"' + c + '\">' + c + ' ' + n + '</button>';
    }});
    h += '</div>';
    if (!items.length) return h + '<div class="empty">' + (FEED_DATA.length ? '这个分类都读过了' : '资讯还没抓到，稍等 30 秒刷新本页') + '</div>';
    h += items.map(function(f) {{
      // f = [title, url, source, published, category]
      var isRead = !!read[f[1]];
      return '<a class=\"item fs-item' + (isRead ? ' read' : '') + '\" href=\"' + f[1] + '\" data-url=\"' + f[1] + '\">' +
        '<span class=\"it-title\">' + f[0].replace(/</g,'&lt;') + '</span>' +
        '<span class=\"it-url\">' + f[2] + (f[3] ? ' · ' + timeAgo(f[3]) : '') + '</span>' +
        '<span class=\"fs-read\" data-url=\"' + f[1] + '\" title=\"📖 大字阅读（提取正文，面板内就地看\" onclick=\"event.preventDefault();event.stopPropagation();return showInlineReader(\'' + f[1].replace(/'/g,'\\\'') + '\'),false;\">📖</span></a>';
    }}).join('');
    return h;
  }}
  function bindFeed() {{
    var btn = document.getElementById('feedHideReadBtn');
    if (btn) btn.addEventListener('click', function() {{ feedHideRead = !feedHideRead; localStorage.setItem('sb_feed_hide_read', feedHideRead ? '1' : ''); panelBody.innerHTML = renderFeed(); bindFeed(); }});
    panelBody.querySelectorAll('.feed-cat[data-cat]').forEach(function(el) {{
      el.addEventListener('click', function() {{ feedCat = el.dataset.cat; localStorage.setItem('sb_feed_cat', feedCat); panelBody.innerHTML = renderFeed(); bindFeed(); }});
    }});
    panelBody.querySelectorAll('.fs-item').forEach(function(el) {{
      el.addEventListener('mousedown', function() {{
        var read = loadRead();
        read[el.dataset.url] = 1;
        localStorage.setItem('sb_feed_read', JSON.stringify(read));
        el.classList.add('read');
      }});
    }});
  }}

  // ---- 内联大字阅读：fetch_article（纯 JSON 不落盘）→ panelBody 就地铺大字正文，不跳标签 ----
  var inlineBackHtml = '';   // 记住原列表 HTML，返回时还原
  var inlineFs = parseInt(localStorage.getItem('sb_reader_fs') || '19', 10);
  function imgFix(cont) {{
    // 图片懒加载修复：data-src/data-original… 提升真 src，剥 srcset，补协议头，滚动出图
    Array.prototype.forEach.call(cont.querySelectorAll('img'), function(im) {{
      var s = im.getAttribute('data-src') || im.getAttribute('data-original') || im.getAttribute('data-lazy-src') || im.getAttribute('data-echo') || im.getAttribute('src');
      if (!s) return;
      if (s.charAt(0) === '/' && s.charAt(1) === '/') s = location.protocol + s;
      if (im.hasAttribute('srcset')) im.removeAttribute('srcset');
      if (im.src === im.baseURI) im.removeAttribute('src');
      im.setAttribute('src', s);
      im.setAttribute('loading', 'lazy');
      im.setAttribute('decoding', 'async');
      im.style.maxWidth = '100%'; im.style.height = 'auto';
    }});
  }}
  function showInlineReader(f) {{
    inlineBackHtml = panelBody.innerHTML;
    panelBody.innerHTML = '<div class="inline-read-loading">📖 正在提取正文…</div>';
    window.__TAURI_INTERNALS__.invoke('fetch_article', {{ url: f[1] }})
      .then(function(a) {{
        panelBody.innerHTML = renderInline(a, f);
        bindInline();
      }})
      .catch(function(e) {{ panelBody.innerHTML = '<div class="inline-read-loading">⚠️ ' + String(e).replace(/</g,'&lt;') + '<br><button id="inlineBack" class="feed-cat on">← 返回列表</button></div>'; document.getElementById('inlineBack').addEventListener('click', function() {{ panelBody.innerHTML = inlineBackHtml; bindFeed(); }}); }});
  }}
  function renderInline(a, f) {{
    // a = {{title, content, text}}; f = [title, url, source, published, category]
    var h = '<div class="ir-bar">' +
      '<button id="inlineBack" title="返回资讯列表">←</button>' +
      '<button class="ir-fs" id="inlineFsDown" title="缩小字号">A−</button>' +
      '<button class="ir-fs" id="inlineFsUp" title="放大字号">A＋</button>' +
      '<a href="' + f[1] + '" target="_blank" class="ir-src">看原文 ↗</a>' +
      '</div>';
    h += '<div class="ir-art"><h1>' + a.title.replace(/</g,'&lt;') + '</h1><div id="content">' + a.content.replace(/data-src=/g,'src=').replace(/data-original=/g,'src=').replace(/data-lazy-src=/g,'src=') + '</div></div>';
    return h;
  }}
  function bindInline() {{
    document.getElementById('inlineBack').addEventListener('click', function() {{ panelBody.innerHTML = inlineBackHtml; bindFeed(); }});
    document.getElementById('inlineFsUp').addEventListener('click', function() {{ inlineFs = Math.min(inlineFs + 2, 30); applyInlineFs(); }});
    document.getElementById('inlineFsDown').addEventListener('click', function() {{ inlineFs = Math.max(inlineFs - 2, 14); applyInlineFs(); }});
    var content = document.getElementById('content');
    content.style.setProperty('--fs', inlineFs + 'px');
    imgFix(content);
  }}
  function applyInlineFs() {{
    localStorage.setItem('sb_reader_fs', inlineFs);
    panelBody.innerHTML = panelBody.innerHTML;  // 重渲染
    var c = document.getElementById('content');
    if (c) c.style.setProperty('--fs', inlineFs + 'px');
  }}

  var PANEL_DATA = {{
    bookmark: {{ title: '🔖 书签', html: {bookmark_html_json} }},
    recent: {{ title: '🕘 最近访问', html: {recent_html_json} }},
    quick: {{ title: '⚡ 快捷访问', html: renderQuick(), bind: bindQuick }},
    feed: {{ title: '📰 资讯', html: renderFeed(), bind: bindFeed }}
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

  // ---- 添加快捷访问弹窗 ----
  document.getElementById('qCancel').addEventListener('click', function() {{ document.getElementById('quickModal').classList.remove('show'); }});
  document.getElementById('qOk').addEventListener('click', function() {{
    var n = document.getElementById('qName').value.trim();
    var u = document.getElementById('qUrl').value.trim();
    var i = document.getElementById('qIcon').value.trim();
    if (!n || !u) return;
    if (!/^https?:\/\//.test(u)) u = 'https://' + u;
    var favs = loadUserFavs();
    favs.unshift({{ name: n, url: u, icon: i || '⭐' }});
    saveUserFavs(favs);
    document.getElementById('quickModal').classList.remove('show');
    document.getElementById('qName').value = '';
    document.getElementById('qUrl').value = '';
    document.getElementById('qIcon').value = '';
    if (current === 'quick') {{ panelBody.innerHTML = renderQuick(); bindQuick(); }}
  }});
  document.getElementById('quickModal').addEventListener('click', function(e) {{
    if (e.target === this) this.classList.remove('show');
  }});

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
        search_url_json = json_script_safe(&serde_json::json!(search_url)),
        feed_data_json = json_script_safe(&serde_json::json!(feed_data)),
        bookmark_html_json = json_script_safe(&serde_json::json!(bookmark_html)),
        recent_html_json = json_script_safe(&serde_json::json!(recent_html)),
        quick_builtin_json = json_script_safe(&serde_json::json!(quick_sites
            .iter()
            .map(|(i, n, u)| serde_json::json!({ "i": i, "n": n, "u": u }))
            .collect::<Vec<_>>())),
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
<meta http-equiv="Content-Security-Policy" content="default-src 'self' data:; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src https: http: data:; connect-src https: http: ipc: http://ipc.localhost; frame-src https: http:">
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

//! CEF 专用启动页：不依赖 Tauri IPC 的最小可用版本。
//!
//! 现有的 `newtab/index.html` 重度依赖 `window.__TAURI_INTERNALS__`（23 处 invoke），
//! 在 CEF 里跑会整屏报错。这里给一个自包含的替代页：搜索框 + 常用站点，
//! 交互全靠页面自己（`location.href`），不回传宿主。

use std::path::Path;

/// 启动页快捷站点（与主应用 `settings.quick_sites` 同结构）。
#[derive(serde::Serialize)]
struct QuickSite {
    icon: String,
    name: String,
    url: String,
}

/// 最近访问（来自 history 表，最多 6 条）。
#[derive(serde::Serialize)]
struct RecentItem {
    title: String,
    url: String,
}

/// 写入启动页 HTML 并返回 `file://` URL（幂等：文件已存在则覆盖重写）。
///
/// 搜索引擎、快捷站点、最近访问都跟主应用数据联动，不再硬编码。
pub fn ensure_newtab_html(shell_dir: &Path) -> Result<String, String> {
    let dir = shell_dir;
    std::fs::create_dir_all(dir).map_err(|e| format!("建启动页目录失败: {e}"))?;
    let (search_url, sites, recent) = load_newtab_settings();
    let html = render_newtab_html(&search_url, &sites, &recent);
    let path = dir.join("cef-newtab.html");
    std::fs::write(&path, html).map_err(|e| format!("写启动页失败: {e}"))?;
    // 世界导航页（F-NEWTAB-01）：模板与 721 站数据都在仓库 resources/ 里，
    // 每次生成启动页时一并落盘，保证侧边栏链接永远可用（离线可用）。
    ensure_worldnav_html(dir)?;
    Ok(format!("file://{}", path.display()))
}

/// 生成世界导航页（32 大类 / 721 站）。
///
/// 模板里占位符是 `__WORLDAV_DATA__`（历史拼写，勿改）。
fn ensure_worldnav_html(dir: &Path) -> Result<(), String> {
    const TPL: &str = include_str!("../../../resources/newtab/world-nav.html");
    const DATA: &str = include_str!("../../../resources/worldnav.json");
    // 内嵌 JSON 防提前闭合 script
    let data = DATA.replace("</", "<\\/");
    let html = TPL.replace("__WORLDAV_DATA__", &data);
    let path = dir.join("world-nav.html");
    std::fs::write(&path, html).map_err(|e| format!("写世界导航页失败: {e}"))?;
    log::info!("[cef] 世界导航页已生成: {}", path.display());
    Ok(())
}

/// 读主应用设置与最近历史（同步：开窗时只跑一次，几十毫秒可接受）。
fn load_newtab_settings() -> (String, Vec<QuickSite>, Vec<RecentItem>) {
    let default_search =
        crate::constants::search::url_template(crate::constants::search::DEFAULT_ENGINE)
            .to_string();
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return (default_search, default_sites(), Vec::new());
    };
    let (engine, sites_json, recent_rows) = tauri::async_runtime::block_on(async move {
        let engine: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'search_engine'")
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        let sites: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'quick_sites'")
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        let recent = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT url, title FROM history ORDER BY last_visit_at DESC LIMIT 6",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default();
        (engine, sites, recent)
    });
    let engine = engine.unwrap_or_else(|| crate::constants::search::DEFAULT_ENGINE.to_string());
    let search_url = crate::constants::search::url_template(&engine).to_string();
    let sites = sites_json
        .and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(&j).ok())
        .map(|arr| arr.iter().filter_map(parse_site).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(default_sites);
    let recent = recent_rows
        .into_iter()
        .filter(|(url, _)| crate::newtab::quick_sites::is_safe_web_url(url))
        .map(|(url, title)| RecentItem {
            title: title.unwrap_or_default(),
            url,
        })
        .collect();
    (search_url, sites, recent)
}

/// 单条快捷站点：只放行 http/https（与 `newtab::quick_sites::is_safe_web_url` 同口径）。
fn parse_site(v: &serde_json::Value) -> Option<QuickSite> {
    let url = v.get("url")?.as_str()?.trim().to_string();
    if !crate::newtab::quick_sites::is_safe_web_url(&url) {
        return None;
    }
    Some(QuickSite {
        icon: v.get("icon").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        name: v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        url,
    })
}

/// 内置默认站点（用户没配置 `quick_sites` 时）。
fn default_sites() -> Vec<QuickSite> {
    [
        ("🔍", "百度", "https://www.baidu.com"),
        ("📖", "知乎", "https://www.zhihu.com"),
        ("📺", "B站", "https://www.bilibili.com"),
        ("🐙", "GitHub", "https://github.com"),
        ("📰", "HN", "https://news.ycombinator.com"),
    ]
    .iter()
    .map(|(icon, name, url)| QuickSite {
        icon: icon.to_string(),
        name: name.to_string(),
        url: url.to_string(),
    })
    .collect()
}

/// 模板替换（纯函数，便于单测）：注入搜索模板、快捷站点与最近访问。
fn render_newtab_html(search_url: &str, sites: &[QuickSite], recent: &[RecentItem]) -> String {
    let search_json = serde_json::to_string(search_url).unwrap_or_else(|_| "\"\"".to_string());
    let sites_json = serde_json::to_string(sites)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/");
    let recent_json = serde_json::to_string(recent)
        .unwrap_or_else(|_| "[]".to_string())
        .replace("</", "<\\/");
    NEWTAB_HTML
        .replace("__SEARCH_URL__", &search_json)
        .replace("__QUICK_SITES__", &sites_json)
        .replace("__RECENT__", &recent_json)
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
    background:
      radial-gradient(circle at 50% -10%, rgba(91,141,239,.13) 0%, transparent 55%),
      radial-gradient(circle at 50% 0%, #1a212d 0%, #101318 65%);
    color: #e6e8eb; font-family: -apple-system, "Noto Sans CJK SC", "Microsoft YaHei", sans-serif;
    display: flex; align-items: center; justify-content: center;
    user-select: none;
  }
  .wrap { width: 720px; max-width: 88vw; text-align: center; }
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
  /* 最近访问（F-CEF-17 配套）：3 列网格 */
  .recent {
    display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px;
    margin-top: 26px; text-align: left;
  }
  .recent-item {
    display: flex; flex-direction: column; gap: 3px;
    padding: 10px 12px; border-radius: 10px;
    background: #171b22; border: 1px solid #262d38; text-decoration: none;
  }
  .recent-item:hover { background: #1f2530; border-color: #3a4453; }
  .recent-item .r-title {
    color: #d7dce4; font-size: 13px; overflow: hidden;
    text-overflow: ellipsis; white-space: nowrap;
  }
  .recent-item .r-url {
    color: #5d6674; font-size: 11px; overflow: hidden;
    text-overflow: ellipsis; white-space: nowrap;
  }
  .hint { margin-top: 26px; font-size: 12px; color: #4d5563; }
  /* 侧边栏（F-NEWTAB-01）：只放真在用的三个入口 */
  .sidebar {
    position: fixed; left: 18px; top: 50%; transform: translateY(-50%);
    display: flex; flex-direction: column; gap: 10px; z-index: 5;
  }
  .sb-btn {
    width: 38px; height: 38px; display: flex; align-items: center; justify-content: center;
    border-radius: 10px; background: #171b22CC; border: 1px solid #262d38;
    color: #8b95a5; cursor: pointer; text-decoration: none; padding: 0;
    transition: background .15s, border-color .15s, color .15s;
  }
  .sb-btn:hover { background: #1f2530; border-color: #3a4453; color: #8ab4f8; }
  .sb-btn svg { width: 17px; height: 17px; fill: none; stroke: currentColor;
    stroke-width: 2; stroke-linecap: round; stroke-linejoin: round; }
  .sb-btn.active { color: #8ab4f8; border-color: #5b8def; }
</style>
</head>
<body>
<nav class="sidebar">
  <a class="sb-btn" id="sb-nav" href="world-nav.html" title="世界导航" aria-label="世界导航">
    <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a15 15 0 0 1 0 18 15 15 0 0 1 0-18z"/></svg>
  </a>
  <button class="sb-btn" id="sb-profile" title="我的" aria-label="我的">
    <svg viewBox="0 0 24 24"><circle cx="12" cy="8" r="4"/><path d="M4 21v-1a7 7 0 0 1 14 0v1"/></svg>
  </button>
  <button class="sb-btn" id="sb-settings" title="设置" aria-label="设置">
    <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>
  </button>
</nav>
<div class="wrap">
  <div class="brand">Shift</div>
  <div class="sub">输入网址或搜索词，回车打开</div>
  <div class="search">
    <svg viewBox="0 0 24 24"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>
    <input id="q" type="text" spellcheck="false" placeholder="搜索或输入网址" autofocus>
  </div>
  <div class="links" id="links"></div>
  <div class="recent" id="recent"></div>
  <div class="hint">Ctrl+T 新标签 · Ctrl+W 关闭 · Ctrl+Tab 切换</div>
</div>
<script>
  // 搜索模板与快捷站点由 Rust 从 settings 注入（F-CEF-17 配套）
  const SEARCH_URL = __SEARCH_URL__;
  const QUICK_SITES = __QUICK_SITES__;
  const q = document.getElementById('q');
  const linksEl = document.getElementById('links');

  (QUICK_SITES || []).forEach(function (s) {
    var a = document.createElement('a');
    a.className = 'link';
    a.href = s.url;
    var icon = document.createElement('i');
    icon.textContent = s.icon || '🔗';
    var name = document.createElement('span');
    name.textContent = s.name || s.url;
    a.append(icon, name);
    linksEl.appendChild(a);
  });

  // 最近访问（来自 history 表，最多 6 条）
  const RECENT = __RECENT__;
  const recentEl = document.getElementById('recent');
  (RECENT || []).forEach(function (r) {
    var a = document.createElement('a');
    a.className = 'recent-item';
    a.href = r.url;
    var t = document.createElement('span');
    t.className = 'r-title';
    t.textContent = r.title || r.url;
    var u = document.createElement('span');
    u.className = 'r-url';
    u.textContent = r.url;
    a.append(t, u);
    recentEl.appendChild(a);
  });

  function go() {
    const v = q.value.trim();
    if (!v) return;
    // 有 scheme 或像域名 → 直接访问；否则用设置里的搜索引擎
    let url;
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(v) || v.startsWith('about:') || v.startsWith('data:')) {
      url = v;
    } else if (v.includes('.') && !v.includes(' ')) {
      url = 'https://' + v;
    } else {
      url = SEARCH_URL + encodeURIComponent(v);
    }
    location.href = url;
  }

  q.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') go();
    else if (e.key === 'Escape') q.blur();
  });

  // 侧边栏（F-NEWTAB-01）：设置走 console 桥通知宿主（内容页无法直接调工具栏）
  document.getElementById('sb-settings')?.addEventListener('click', function () {
    console.log('SHIFT_NEWTAB:settings');
  });
  // 「我的」第一版：提示尚未接入账号（后续版本接 db/users）
  document.getElementById('sb-profile')?.addEventListener('click', function () {
    console.log('SHIFT_NEWTAB:profile');
  });

  q.focus();
  document.addEventListener('dragstart', (e) => e.preventDefault());
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::{ensure_newtab_html, parse_site, render_newtab_html, QuickSite, RecentItem};

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

    #[test]
    fn render_newtab_injects_search_and_sites() {
        let sites = vec![
            QuickSite {
                icon: "🔍".into(),
                name: "必应".into(),
                url: "https://www.bing.com".into(),
            },
            QuickSite {
                icon: "📖".into(),
                name: "a</script>".into(),
                url: "https://a.com".into(),
            },
        ];
        let recent = vec![RecentItem {
            title: "示例".into(),
            url: "https://example.com".into(),
        }];
        let html = render_newtab_html("https://www.bing.com/search?q=", &sites, &recent);
        assert!(
            html.contains("\"https://www.bing.com/search?q=\""),
            "搜索模板必须注入"
        );
        assert!(html.contains("必应"));
        assert!(html.contains("a<\\/script>"), "</ 必须转义，防提前闭合 script");
        assert!(!html.contains("a</script>"));
        assert!(!html.contains("__SEARCH_URL__"));
        assert!(!html.contains("__QUICK_SITES__"));
        assert!(html.contains("\"url\":\"https://example.com\""), "最近访问必须注入");
        assert!(!html.contains("__RECENT__"));
    }

    #[test]
    fn parse_site_rejects_non_web_schemes() {
        let ok = serde_json::json!({"icon":"x","name":"站点","url":"https://example.com"});
        assert!(parse_site(&ok).is_some());
        let bad = serde_json::json!({"icon":"x","name":"坏","url":"javascript:alert(1)"});
        assert!(parse_site(&bad).is_none());
        let no_url = serde_json::json!({"icon":"x","name":"缺 URL"});
        assert!(parse_site(&no_url).is_none());
    }
}

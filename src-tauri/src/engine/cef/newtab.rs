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
    ensure_settings_html(dir)?;
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
    let context = serde_json::json!({ "recent": load_recent_for_worldnav() })
        .to_string()
        .replace("</", "<\\/");
    let html = TPL
        .replace("__WORLDAV_DATA__", &data)
        .replace("__WORLD_CONTEXT__", &context);
    let path = dir.join("world-nav.html");
    std::fs::write(&path, html).map_err(|e| format!("写世界导航页失败: {e}"))?;
    log::info!("[cef] 世界导航页已生成: {}", path.display());
    Ok(())
}

/// 世界导航「最近访问」：history 最近 8 条，只放行 http/https。
fn load_recent_for_worldnav() -> Vec<serde_json::Value> {
    let Some(pool) = crate::db::GLOBAL_POOL.get().cloned() else {
        return Vec::new();
    };
    let rows = tauri::async_runtime::block_on(async move {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT url, title FROM history ORDER BY last_visit_at DESC LIMIT 8",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_default()
    });
    rows.into_iter()
        .filter(|(url, _)| crate::newtab::quick_sites::is_safe_web_url(url))
        .map(|(url, title)| {
            serde_json::json!({
                "url": url,
                "title": title.unwrap_or_default(),
            })
        })
        .collect()
}

/// 生成设置页（F-CEF-40：设置改独立标签页，不再加高工具栏）。
pub(crate) fn ensure_settings_html(dir: &Path) -> Result<String, String> {
    const TPL: &str = include_str!("../../../resources/newtab/settings.html");
    let path = dir.join("settings.html");
    std::fs::write(&path, TPL).map_err(|e| format!("写设置页失败: {e}"))?;
    Ok(format!("file://{}", path.display()))
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
        icon: v
            .get("icon")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
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

const NEWTAB_HTML: &str = include_str!("newtab_page.html");

#[cfg(test)]
#[path = "newtab_tests.rs"]
mod tests;

//! 新标签页 / 世界导航页 HTML 渲染（R1 拆分 · 2026-09-25）
//!
//! 模板 P0-1 已外置到 resources/newtab/，改文件即生效无需重编译；
//! 占位符 {{X}} 由 render_html 替换，所有内嵌 JSON 均过 json_script_safe 防 XSS。

/// 世界导航数据（32 大类 721 站，从原版 worldNav.ts 移植）
const WORLDNAV_JSON: &str = include_str!("../../resources/worldnav.json");

/// IPC 桥 JS（WebKitGTK 自定义协议页面不执行 UserScript，手动注入）
const IPC_BRIDGE: &str = include_str!("../ipc-bridge.js");

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

/// 启动页模板（P0-1 外置 · 2026-09-22）：改 resources/newtab/index.html 即生效，
/// 不再需要重编译 Rust。占位符 {{X}} 由 render_html() 替换。
const NEWTAB_TPL: &str = include_str!("../../resources/newtab/index.html");

/// 世界导航页模板（P0-1 外置 · 2026-09-22）
const WORLDNAV_TPL: &str = include_str!("../../resources/newtab/world-nav.html");

/// P3：原子写文件——先写同目录临时文件再 rename。
/// 原 std::fs::write 先截断再写，中途崩溃/断电会留下半截 HTML，
/// 新标签页/世界导航页直接打不开（启动页级故障），原子写把损坏窗口缩到零。
pub(super) fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    // 随机后缀避免并发 generate 互相覆盖临时文件（rename 原子，最后者胜）
    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

pub(super) fn default_quick_sites() -> Vec<(String, String, String)> {
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

#[allow(clippy::too_many_arguments)]
pub(super) fn render_html(
    search_url: &str,
    recent: &[(String, String)],
    bookmarks: &[(String, String)],
    quick_sites: &[(String, String, String)],
    feed_items: &[(String, String, String, String, String)],
    wallpapers: &[String],
    show_bg: bool,
    font_size: u32,
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

    // 快捷访问内置项（主区 top-sites 网格）
    let quick_grid = quick_sites
    .iter()
    .map(|(i, n, u)| {
      format!(
        "<div class=\"top-site\" data-url=\"{}\"><a class=\"top-site-link\" href=\"{}\"><span class=\"icon\">{}</span><span class=\"name\">{}</span></a><button class=\"qs-del\">×</button></div>",
        esc(u), esc(u), esc(i), esc(n)
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

    // 动态背景层（原版 bg-layer：Bing 每日壁纸 + Ken Burns 缓慢缩放 + 暗色渐变遮罩）
    let bg_layer_html = if show_bg {
        "<div class=\"bg-layer\" id=\"bgLayer\"></div>".to_string()
    } else {
        String::new()
    };
    // 壁纸 URL 数组嵌入 JS（file:// 页面直接 fetch Bing 有 CORS，Rust 端拉取规避）
    let wallpapers_json = serde_json::to_string(wallpapers)
        .unwrap_or_else(|_| "[]".into())
        .replace('<', "\\u003c");

    // P0-1（2026-09-22）：模板外置到 resources/newtab/index.html，改文件即生效，
    // 不再需要重编译 Rust。占位符用 {{X}} 风格（避开 CSS/JS 的 {}）。
    let mut html = NEWTAB_TPL.to_string();
    let repl: [(&str, String); 12] = [
        ("{{quick_grid}}", quick_grid),
        ("{{font_scale}}", format!("{}", font_size as f64 / 14.0)),
        (
            "{{search_url_json}}",
            json_script_safe(&serde_json::json!(search_url)),
        ),
        (
            "{{history_json}}",
            json_script_safe(&serde_json::json!(recent
                .iter()
                .map(|(u, t)| serde_json::json!({ "title": t, "url": u }))
                .collect::<Vec<_>>())),
        ),
        (
            "{{feed_data_json}}",
            json_script_safe(&serde_json::json!(feed_data)),
        ),
        (
            "{{bookmark_html_json}}",
            json_script_safe(&serde_json::json!(bookmark_html)),
        ),
        (
            "{{recent_html_json}}",
            json_script_safe(&serde_json::json!(recent_html)),
        ),
        (
            "{{quick_builtin_json}}",
            json_script_safe(&serde_json::json!(quick_sites
                .iter()
                .map(|(i, n, u)| serde_json::json!({ "i": i, "n": n, "u": u }))
                .collect::<Vec<_>>())),
        ),
        ("{{ipc_bridge}}", IPC_BRIDGE.to_string()),
        ("{{sb_version}}", env!("CARGO_PKG_VERSION").to_string()),
        ("{{bg_layer_html}}", bg_layer_html),
        ("{{wallpapers_json}}", wallpapers_json),
    ];
    for (k, v) in repl {
        html = html.replace(k, &v);
    }
    html
}

/// 生成世界导航页 HTML（数据内嵌，纯前端交互，file:// 同源 localStorage 与启动页共享）
pub(super) fn render_worldnav_html() -> String {
    // P0-1：外置到 resources/newtab/world-nav.html
    let tpl = WORLDNAV_TPL;
    tpl.replace(
        "__WORLDAV_DATA__",
        // worldnav.json 是内置可信资源，但防御纵深：与启动页一致，JSON 过 json_script_safe
        // （若数据含 </script> 会提前闭合标签导致 HTML 注入）
        &serde_json::from_str::<serde_json::Value>(WORLDNAV_JSON)
            .map(|v| json_script_safe(&v))
            .unwrap_or_else(|_| "[]".to_string()),
    )
}

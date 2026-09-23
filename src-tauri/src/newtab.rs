// 新标签页（原版左侧栏面板生态迁移）
// 方案：Rust 读数据库渲染成完整 HTML，写到 app_data/newtab/index.html，
// 标签用 file:// 打开。面板静态数据（最近访问/书签/快捷访问）渲染时嵌入，
// 交互状态（待办勾选）用 localStorage 存本地，无需 IPC。
use crate::db::Db;
use tauri::{AppHandle, Emitter, Manager, State};

/// 世界导航数据（32 大类 721 站，从原版 worldNav.ts 移植）
const WORLDNAV_JSON: &str = include_str!("../resources/worldnav.json");

/// IPC 桥 JS（WebKitGTK 自定义协议页面不执行 UserScript，手动注入）
const IPC_BRIDGE: &str = include_str!("ipc-bridge.js");

/// 生成互斥锁：序列化并发生成（setup 异步生成 / newtab_url 兜底 / refresh_newtab 可能同时触发）
static GEN_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

/// 壁纸缓存有效期（秒）：Bing 每日更新，6 小时缓存足够，避免每次生成都打网络
const WALLPAPER_CACHE_TTL: u64 = 6 * 3600;

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

/// 拉取 Bing 每日壁纸（原版 bg-images IPC 迁移）。
/// 主进程拉取规避 CORS，失败返回空 Vec（保持纯色渐变背景）。
async fn fetch_bing_wallpapers() -> Vec<String> {
    let api = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=zh-CN";
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Safari/537.36")
        .build()
        .unwrap_or_default();
    match client.get(api).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(data) => {
                let imgs = data
                    .get("images")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let urls: Vec<String> = imgs
                    .iter()
                    .filter_map(|i| {
                        let u = i.get("url")?.as_str()?;
                        Some(format!("https://www.bing.com{}", u))
                    })
                    .collect();
                log::info!("[newtab] Bing 壁纸拉取成功 {} 张", urls.len());
                urls
            }
            Err(e) => {
                log::warn!("[newtab] Bing 壁纸 JSON 解析失败: {}", e);
                Vec::new()
            }
        },
        Ok(resp) => {
            log::warn!("[newtab] Bing 壁纸 HTTP {}", resp.status());
            Vec::new()
        }
        Err(e) => {
            log::warn!("[newtab] Bing 壁纸拉取失败: {}", e);
            Vec::new()
        }
    }
}

/// 只读壁纸缓存（不拉网络）：generate 关键路径绝不能打网络——
/// 解锁后前端 createTab 的 refresh_newtab 会等 generate 的 GEN_LOCK，
/// 若 generate 里拉 Bing 壁纸（超时 6 秒）启动页就卡住几秒。
/// 壁纸由独立后台任务 refresh_wallpapers_background 拉取写缓存。
fn load_wallpapers_from_cache(cache: &std::path::Path) -> Vec<String> {
    if let Ok(s) = std::fs::read_to_string(cache) {
        if let Ok(v) = serde_json::from_str::<Vec<String>>(&s) {
            if !v.is_empty() {
                log::debug!("[newtab] 壁纸走缓存 {} 张", v.len());
                return v;
            }
        }
    }
    Vec::new()
}

/// 后台刷新壁纸缓存：拉取成功后写盘，失败静默（下次再试）。
/// 在 setup 锁屏前 spawn，锁屏期间完成网络拉取，不占解锁时间。
pub async fn refresh_wallpapers_background(app: &AppHandle) {
    let Some(path) = newtab_path(app) else { return };
    let cache = path
        .parent()
        .map(|p| p.join("wallpapers.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("wallpapers.json"));
    // 缓存新鲜就不拉（避免每次启动都打网络）
    if let Ok(meta) = std::fs::metadata(&cache) {
        if let Ok(modified) = meta.modified() {
            if modified
                .elapsed()
                .map(|d| d.as_secs() < WALLPAPER_CACHE_TTL)
                .unwrap_or(false)
            {
                log::debug!("[newtab] 壁纸缓存新鲜，跳过后台刷新");
                return;
            }
        }
    }
    let fresh = fetch_bing_wallpapers().await;
    if fresh.is_empty() {
        log::warn!("[newtab] 壁纸拉取失败（保留旧缓存或纯色背景）");
        return;
    }
    if let Err(e) = std::fs::write(&cache, serde_json::to_string(&fresh).unwrap_or_default()) {
        log::warn!("[newtab] 壁纸缓存写入失败: {}", e);
    } else {
        log::info!("[newtab] 壁纸缓存已刷新 {} 张", fresh.len());
    }
}

/// 生成新标签页 HTML 并写盘。返回文件路径字符串。
/// 启动页模板（P0-1 外置 · 2026-09-22）：改 resources/newtab/index.html 即生效，
/// 不再需要重编译 Rust。占位符 {{X}} 由 generate() 替换。
const NEWTAB_TPL: &str = include_str!("../resources/newtab/index.html");

pub async fn generate(db: &Db, path: &std::path::Path) -> Result<String, String> {
    // 并发生成去重：序列化 + 防抖（1.5s 内刚生成过则跳过）。
    // 调用点：setup 异步生成 / newtab_url 兜底 / refresh_newtab 命令，可能并发触发。
    let lock = GEN_LOCK.get_or_init(|| tokio::sync::Mutex::new(()));
    let _guard = lock.lock().await;
    if let Ok(meta) = std::fs::metadata(path) {
        if let Ok(modified) = meta.modified() {
            if modified
                .elapsed()
                .map(|d| d.as_millis() < 1500)
                .unwrap_or(false)
            {
                log::debug!("[newtab] 文件刚生成过，跳过本次生成");
                return Ok(path.display().to_string());
            }
        }
    }

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
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
        row.map(|r| r.0)
    }
    let search_engine = setting(db, "search_engine")
        .await
        .unwrap_or_else(|| crate::constants::search::DEFAULT_ENGINE.into());
    // 搜索引擎 URL 模板来自 constants.rs（唯一来源，未知 id 自动回退默认引擎）
    let search_url = crate::constants::search::url_template(&search_engine);
    let quick_sites: Vec<(String, String, String)> = match setting(db, "quick_sites").await {
        Some(j) => serde_json::from_str::<Vec<serde_json::Value>>(&j)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let icon = v.get("icon")?.as_str()?.to_string();
                        let name = v.get("name")?.as_str()?.to_string();
                        let url = v.get("url")?.as_str()?.to_string();
                        // P2：过滤危险 scheme（历史遗留/被篡改的 quick_sites 数据），
                        // 特权页面不允许渲染 javascript: 等
                        if !is_safe_web_url(&url) {
                            return None;
                        }
                        Some((icon, name, url))
                    })
                    .collect()
            })
            .unwrap_or_else(|_| default_quick_sites()),
        None => default_quick_sites(),
    };

    // ---- 数据：资讯（内置源缓存最新 80 条）----
    let feed_items = crate::feed::latest_items(db, 80).await;

    // ---- 数据：动态背景开关（原版 showBg，默认开）----
    let show_bg = setting(db, "show_bg")
        .await
        .map(|v| v != "0" && v != "false")
        .unwrap_or(true);
    let wallpapers = if show_bg {
        // 关键路径只读缓存（不拉网络）——网络由 refresh_wallpapers_background 后台做
        let cache = path
            .parent()
            .map(|p| p.join("wallpapers.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("wallpapers.json"));
        load_wallpapers_from_cache(&cache)
    } else {
        Vec::new()
    };

    // ---- 数据：字体大小（原版 fontSize，默认 14）----
    let font_size: u32 = setting(db, "font_size")
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(14);

    // ---- 渲染 HTML（启动页 + 世界导航页）----
    let html = render_html(
        search_url,
        &recent,
        &bookmarks,
        &quick_sites,
        &feed_items,
        &wallpapers,
        show_bg,
        font_size,
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        atomic_write(&parent.join("world-nav.html"), &render_worldnav_html())
            .map_err(|e| e.to_string())?;
    }
    atomic_write(path, &html).map_err(|e| e.to_string())?;
    Ok(path.display().to_string())
}

/// P3：原子写文件——先写同目录临时文件再 rename。
/// 原 std::fs::write 先截断再写，中途崩溃/断电会留下半截 HTML，
/// 新标签页/世界导航页直接打不开（启动页级故障），原子写把损坏窗口缩到零。
fn atomic_write(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    // 随机后缀避免并发 generate 互相覆盖临时文件（rename 原子，最后者胜）
    let tmp = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
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

#[allow(clippy::too_many_arguments)]
fn render_html(
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
        ("{{search_url_json}}", json_script_safe(&serde_json::json!(search_url))),
        ("{{history_json}}", json_script_safe(&serde_json::json!(recent
            .iter()
            .map(|(u, t)| serde_json::json!({ "title": t, "url": u }))
            .collect::<Vec<_>>()))),
        ("{{feed_data_json}}", json_script_safe(&serde_json::json!(feed_data))),
        ("{{bookmark_html_json}}", json_script_safe(&serde_json::json!(bookmark_html))),
        ("{{recent_html_json}}", json_script_safe(&serde_json::json!(recent_html))),
        ("{{quick_builtin_json}}", json_script_safe(&serde_json::json!(quick_sites
            .iter()
            .map(|(i, n, u)| serde_json::json!({ "i": i, "n": n, "u": u }))
            .collect::<Vec<_>>()))),
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
/// 世界导航页模板（P0-1 外置 · 2026-09-22）
const WORLDNAV_TPL: &str = include_str!("../resources/newtab/world-nav.html");

fn render_worldnav_html() -> String {
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

/// 启动页右侧栏按钮：通知主界面打开 AI 助手侧栏
#[tauri::command]
pub fn open_ai_from_newtab(app: AppHandle) {
    let _ = app.emit("open-ai-from-newtab", ());
}

/// 启动页右侧栏按钮：通知主界面打开设置弹窗
#[tauri::command]
pub fn open_settings_from_newtab(app: AppHandle) {
    let _ = app.emit("open-settings-from-newtab", ());
}

/// 启动页资讯点击：通知主界面在**当前标签右侧**新开标签打开该 URL（列表页保留）。
/// 走事件而非直接建标签，是为了复用前端 createTab 的完整同步逻辑（顺序/激活/归组）。
#[tauri::command]
pub fn open_url_from_newtab(app: AppHandle, url: String) {
    log::info!("[newtab] 资讯点击 → 右侧新标签: {}", crate::redact_url(&url));
    let _ = app.emit("open-url-from-newtab", url);
}

/// 待办清单存储键（settings 表）
const TODO_KEY: &str = "todos";

/// 读待办清单（JSON 数组字符串，默认 "[]"）。
/// 启动页是 shift:// 自定义协议，localStorage 在 WebKitGTK 下不可靠，
/// 与快捷访问一样改走 settings 表（IPC）持久化。
#[tauri::command]
pub async fn todo_list(db: State<'_, Db>) -> Result<String, String> {
    let row: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(TODO_KEY)
        .fetch_optional(&db.pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.unwrap_or_else(|| "[]".to_string()))
}

/// 保存待办清单（JSON 数组字符串）。
/// P2 防御：限制体积（防滥用撑爆 DB），内容按 JSON 数组校验。
#[tauri::command]
pub async fn todo_save(db: State<'_, Db>, todos: String) -> Result<(), String> {
    if todos.len() > 100 * 1024 {
        return Err("待办数据过大".to_string());
    }
    // 必须是 JSON 数组
    let parsed: serde_json::Value =
        serde_json::from_str(&todos).map_err(|e| format!("待办格式错误: {e}"))?;
    if !parsed.is_array() {
        return Err("待办格式错误: 需为数组".to_string());
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(TODO_KEY)
    .bind(&todos)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// 保存主区快捷访问（原版 quick-sites-set IPC 迁移）。
/// 启动页编辑完成后调用，写入 settings.quick_sites，下次生成 HTML 时生效。
#[tauri::command]
pub async fn save_quick_sites(
    db: State<'_, Db>,
    app: AppHandle,
    sites: Vec<serde_json::Value>,
) -> Result<(), String> {
    // P2：URL scheme 校验——新标签页是带 IPC 桥（invoke_key）的特权页面，
    // 若允许 javascript: 等 scheme 渲染成 <a href>，点击即在特权上下文执行任意命令
    let mut cleaned: Vec<serde_json::Value> = Vec::with_capacity(sites.len());
    for site in &sites {
        let mut s = site.clone();
        let url = s
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| "快捷站点缺少 url".to_string())?;
        let norm = normalize_quick_site_url(url)?;
        s["url"] = serde_json::Value::String(norm);
        cleaned.push(s);
    }
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('quick_sites', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    log::info!("[newtab] 快捷访问已保存 {} 条", cleaned.len());
    // 重新生成 HTML，让主区网格立即生效
    let path = newtab_path(&app).ok_or("无法获取数据目录")?;
    generate(&db, &path).await?;
    Ok(())
}

/// 校验是否为可安全渲染的网页 URL（http/https）
/// 新标签页是特权页面（带 IPC 桥），javascript:/data: 等 scheme 一旦渲染成
/// <a href> 点击即可在特权上下文执行任意命令，必须拒绝
fn is_safe_web_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// 规范化快捷站点 URL：显式危险 scheme 拒绝；无 scheme 自动补 https://
/// （用户常输入 "example.com"，原版行为是补全协议）
fn normalize_quick_site_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("快捷站点 URL 不能为空".to_string());
    }
    let lower = u.to_ascii_lowercase();
    for bad in ["javascript:", "data:", "file:", "vbscript:", "blob:"] {
        if lower.starts_with(bad) {
            return Err(format!("快捷站点不允许 {} 协议", bad));
        }
    }
    if lower.starts_with("http://") || lower.starts_with("https://") {
        Ok(u.to_string())
    } else {
        Ok(format!("https://{}", u))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_accepts_http_https_and_bare_domain() {
        assert_eq!(
            normalize_quick_site_url("https://example.com").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            normalize_quick_site_url("http://example.com").unwrap(),
            "http://example.com"
        );
        // 无 scheme 自动补 https://
        assert_eq!(
            normalize_quick_site_url("example.com").unwrap(),
            "https://example.com"
        );
        // 大小写容忍
        assert_eq!(
            normalize_quick_site_url("HTTPS://example.com").unwrap(),
            "HTTPS://example.com"
        );
        // 首尾空白裁剪
        assert_eq!(
            normalize_quick_site_url("  example.com  ").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn normalize_rejects_dangerous_schemes() {
        for bad in [
            "javascript:alert(1)",
            "data:text/html,<script>alert(1)</script>",
            "file:///etc/passwd",
            "vbscript:msgbox(1)",
            "blob:https://example.com/uuid",
        ] {
            assert!(normalize_quick_site_url(bad).is_err(), "应拒绝: {}", bad);
        }
        // 大小写变体也拒绝
        assert!(normalize_quick_site_url("JaVaScRiPt:alert(1)").is_err());
    }

    #[test]
    fn normalize_rejects_empty() {
        assert!(normalize_quick_site_url("").is_err());
        assert!(normalize_quick_site_url("   ").is_err());
    }

    #[test]
    fn is_safe_web_url_filters_schemes() {
        assert!(is_safe_web_url("https://example.com"));
        assert!(is_safe_web_url("http://example.com"));
        assert!(!is_safe_web_url("javascript:alert(1)"));
        assert!(!is_safe_web_url("file:///etc/passwd"));
        assert!(!is_safe_web_url("data:text/html,x"));
        assert!(!is_safe_web_url("example.com")); // 无 scheme 不算安全（渲染前必须规范化）
    }
}

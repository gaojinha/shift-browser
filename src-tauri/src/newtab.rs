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
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
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

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>新标签页</title>
<script>{ipc_bridge}</script>
<style>
  :root {{ --accent:#4a9eff; }}
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  html,body {{ height:100%; }}
  body {{ font-family:'PingFang SC','Microsoft YaHei',-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif; background:linear-gradient(135deg,#0f172a 0%,#1e1b4b 45%,#172554 100%); color:#e2e8f0; min-height:100vh; overflow-x:hidden; overflow-y:auto; }}
  /* 性能优化（2026-09-18）：Ken Burns 壁纸动画提升为 GPU 合成层。
     原实现缺 will-change/translateZ(0)，WebKit 每帧在 CPU 上重绘 110% 视口尺寸的
     高清壁纸位图，实测 1 个标签空闲时 WebKitWebProcess 持续 17-18% CPU。
     提升为合成层后缩放由 GPU 完成，CPU 占用大幅下降。 */
  .bg-layer {{ position:fixed; top:-5%; left:-5%; width:110%; height:110%; z-index:-1; background-size:cover; background-position:center; animation:kenburns 45s ease-in-out infinite alternate; will-change:transform; transform:translateZ(0); backface-visibility:hidden; }}
  .bg-layer::after {{ content:''; position:absolute; inset:0; background:linear-gradient(180deg, rgba(8,12,28,0.58) 0%, rgba(8,12,28,0.38) 35%, rgba(8,12,28,0.78) 100%); }}
  @keyframes kenburns {{ from {{ transform:scale(1) translate(0,0); }} to {{ transform:scale(1.08) translate(1%,1.5%); }} }}
  .main-layout {{ display:flex; max-width:none; margin:0; align-items:stretch; min-height:100vh; padding-left:56px; }}
  /* 左右栏：玻璃拟态悬浮卡片（2026-09-19 UI 美化） */
  .icon-bar {{ position:fixed; top:14px; bottom:14px; left:10px; width:46px; z-index:20; background:rgba(255,255,255,0.06); backdrop-filter:blur(20px); -webkit-backdrop-filter:blur(20px); border:1px solid rgba(255,255,255,0.12); border-radius:16px; display:flex; flex-direction:column; align-items:center; padding:12px 0; gap:6px; overflow-y:auto; scrollbar-width:none; box-shadow:0 8px 32px rgba(0,0,0,0.25); }}
  .icon-btn {{ position:relative; width:36px; height:36px; display:flex; align-items:center; justify-content:center; font-size:17px; border:none; background:transparent; border-radius:10px; cursor:pointer; transition:all 0.18s; color:rgba(255,255,255,0.85); }}
  .icon-btn:hover {{ background:rgba(255,255,255,0.14); transform:translateY(-1px); }}
  .icon-btn.active {{ background:linear-gradient(135deg,var(--accent),#7c5cff); color:#fff; box-shadow:0 4px 14px rgba(74,158,255,0.4); }}
  .icon-btn.bottom {{ margin-top:auto; }}
  .side-panel {{ width:280px; flex-shrink:0; background:rgba(15,23,42,0.72); backdrop-filter:blur(24px); -webkit-backdrop-filter:blur(24px); border-right:1px solid rgba(255,255,255,0.1); display:none; flex-direction:column; max-height:100vh; position:sticky; top:0; overflow:hidden; }}
  .side-panel.open {{ display:flex; }}
  .panel-head {{ display:flex; justify-content:space-between; align-items:center; padding:14px 16px; border-bottom:1px solid rgba(255,255,255,0.1); gap:8px; background:rgba(255,255,255,0.03); }}
  .panel-head h2 {{ font-size:15px; color:#fff; white-space:nowrap; font-weight:700; letter-spacing:0.5px; }}
  .panel-actions {{ display:flex; gap:6px; align-items:center; flex-shrink:0; }}
  .panel-body {{ flex:1; overflow-y:auto; padding:14px; }}
  .panel-body::-webkit-scrollbar {{ width:6px; }}
  .panel-body::-webkit-scrollbar-thumb {{ background:rgba(255,255,255,0.2); border-radius:3px; }}
  .fs-close {{ background:none; border:none; color:rgba(255,255,255,0.5); font-size:18px; cursor:pointer; line-height:1; padding:2px 6px; border-radius:4px; }}
  .fs-close:hover {{ color:#f44; background:rgba(255,255,255,0.08); }}
  .fs-item {{ display:block; padding:10px 12px; border-radius:10px; cursor:pointer; color:#ddd; font-size:13px; line-height:1.4; margin-bottom:6px; transition:background 0.15s; text-decoration:none; position:relative; background:rgba(255,255,255,0.04); border:1px solid rgba(255,255,255,0.06); }}
  .fs-item:hover {{ background:rgba(255,255,255,0.1); color:#fff; }}
  .fs-item .fs-title {{ display:block; }}
  .fs-item .fs-meta {{ display:block; font-size:11px; color:rgba(255,255,255,0.4); margin-top:2px; }}
  .fs-item.read {{ opacity:0.55; }}
  .fs-item.read .fs-title {{ color:rgba(255,255,255,0.55); }}
  .fs-item.read .fs-title::before {{ content:'✓ '; color:var(--accent); font-weight:600; }}
  .panel-body.hide-read .fs-item.read {{ display:none; }}
  .fs-count {{ text-align:center; font-size:11px; color:rgba(255,255,255,0.35); padding:10px 0 4px; }}
  .item {{ display:block; padding:8px 10px; border-radius:8px; text-decoration:none; color:#ddd; }}
  .item:hover {{ background:rgba(255,255,255,0.1); }}
  .it-title {{ display:block; font-size:13px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .it-url {{ display:block; font-size:11px; color:rgba(255,255,255,0.4); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .fs-item .it-url {{ padding-right:34px; }}
  .fs-read {{ position:absolute; right:8px; top:50%; transform:translateY(-50%); z-index:2; width:26px; height:26px; border:none; background:rgba(255,255,255,0.1); border:1px solid rgba(255,255,255,0.2); border-radius:7px; cursor:pointer; font-size:13px; display:flex; align-items:center; justify-content:center; color:var(--accent); }}
  .fs-read:hover {{ background:var(--accent); color:#fff; border-color:var(--accent); }}
  .feed-bar {{ display:flex; flex-wrap:wrap; gap:4px; margin-bottom:8px; }}
  .feed-toggle {{ display:block; margin:0 0 8px 4px; background:rgba(255,255,255,0.1); border:1px solid rgba(255,255,255,0.2); color:var(--accent); border-radius:6px; padding:4px 10px; font-size:12px; cursor:pointer; }}
  .feed-cat {{ background:rgba(255,255,255,0.1); border:1px solid rgba(255,255,255,0.15); color:rgba(255,255,255,0.6); border-radius:10px; padding:2px 8px; font-size:11px; cursor:pointer; }}
  .feed-cat.on {{ background:var(--accent); border-color:var(--accent); color:#fff; }}
  .q-item {{ display:flex; align-items:center; }}
  .q-link {{ flex:1; min-width:0; display:block; text-decoration:none; color:inherit; overflow:hidden; }}
  .q-del {{ border:none; background:transparent; color:rgba(255,255,255,0.4); cursor:pointer; font-size:12px; flex-shrink:0; visibility:hidden; }}
  .q-item:hover .q-del {{ visibility:visible; }}
  .q-del:hover {{ color:#ff6b6b; }}
  .todo-panel {{ display:flex; flex-direction:column; gap:8px; margin-bottom:10px; }}
  .todo-add {{ display:flex; gap:6px; margin-bottom:8px; }}
  .todo-add input {{ flex:1; padding:9px 12px; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; outline:none; }}
  .todo-add input:focus {{ border-color:var(--accent); }}
  .todo-add button {{ padding:9px 16px; background:linear-gradient(135deg,var(--accent),#7c5cff); border:none; border-radius:10px; color:#fff; font-size:13px; cursor:pointer; }}
  .todo-list {{ display:flex; flex-direction:column; gap:6px; }}
  .todo-item {{ display:flex; align-items:center; gap:8px; padding:10px 12px; background:rgba(255,255,255,0.05); border:1px solid rgba(255,255,255,0.07); border-radius:10px; font-size:13px; cursor:pointer; transition:background 0.15s; }}
  .todo-item:hover {{ background:rgba(255,255,255,0.1); }}
  .todo-item input[type=checkbox] {{ accent-color:var(--accent); width:15px; height:15px; flex-shrink:0; }}
  .todo-item.done .todo-text {{ text-decoration:line-through; color:rgba(255,255,255,0.4); }}
  .todo-text {{ flex:1; font-size:13px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .todo-del {{ border:none; background:transparent; color:rgba(255,255,255,0.4); cursor:pointer; font-size:13px; }}
  .todo-del:hover {{ color:#f44; }}
  .bookmark-list {{ display:flex; flex-wrap:wrap; gap:8px; }}
  .bookmark-item {{ display:flex; align-items:center; gap:6px; padding:9px 14px; background:rgba(255,255,255,0.05); border:1px solid rgba(255,255,255,0.07); border-radius:10px; text-decoration:none; color:#e0e0e0; font-size:13px; transition:background 0.15s; max-width:220px; }}
  .bookmark-item:hover {{ background:rgba(255,255,255,0.1); }}
  .bookmark-name {{ overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .inline-read-loading {{ padding:20px; text-align:center; color:rgba(255,255,255,0.6); font-size:13px; }}
  .ir-bar {{ display:flex; align-items:center; gap:6px; margin-bottom:10px; }}
  .ir-bar button {{ border:none; background:rgba(255,255,255,0.1); color:#ddd; border-radius:6px; padding:4px 10px; font-size:13px; cursor:pointer; }}
  .ir-bar .ir-src {{ margin-left:auto; color:var(--accent); font-size:12px; text-decoration:none; }}
  .ir-art h1 {{ font-size:18px; margin-bottom:10px; color:#fff; line-height:1.4; }}
  .ir-art {{ font-size:var(--fs,19px); line-height:1.8; color:#ddd; word-break:break-word; }}
  .ir-art img {{ max-width:100%; height:auto; }}
  .profile-info {{ text-align:center; padding:16px 10px; }}
  .profile-info .pi-avatar {{ font-size:44px; line-height:1.2; }}
  .profile-info .pi-avatar-img {{ width:64px; height:64px; border-radius:50%; object-fit:cover; border:2px solid rgba(255,255,255,0.2); box-shadow:0 4px 16px rgba(0,0,0,0.3); }}
  .profile-info .pi-name {{ font-size:16px; font-weight:600; color:#fff; margin-top:8px; }}
  .profile-info .pi-user {{ font-size:12px; color:rgba(255,255,255,0.45); margin-top:2px; }}
  .profile-info .pi-sig {{ font-size:12px; color:rgba(255,255,255,0.6); margin-top:8px; padding:0 6px; }}
  .panel-link-item {{ display:flex; align-items:center; gap:8px; padding:10px 12px; border-radius:10px; cursor:pointer; color:#ddd; font-size:13px; margin-bottom:6px; transition:background 0.15s; text-decoration:none; background:rgba(255,255,255,0.04); border:1px solid rgba(255,255,255,0.06); }}
  .panel-link-item:hover {{ background:rgba(255,255,255,0.1); color:#fff; }}
  .panel-link-item .pli-icon {{ font-size:16px; flex-shrink:0; }}
  .panel-link-item .pli-name {{ flex:1; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .panel-link-item .pli-url {{ font-size:11px; color:rgba(255,255,255,0.4); max-width:45%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .uc-loading {{ text-align:center; padding:30px; color:rgba(255,255,255,0.5); font-size:13px; }}
  .uc-wrap {{ padding:4px 2px; }}
  .uc-tabs {{ display:flex; gap:6px; margin-bottom:14px; background:rgba(255,255,255,0.06); border-radius:10px; padding:4px; }}
  .uc-tab {{ flex:1; padding:8px 0; border:none; background:transparent; color:rgba(255,255,255,0.6); font-size:13px; border-radius:8px; cursor:pointer; transition:all 0.15s; }}
  .uc-tab.active {{ background:linear-gradient(135deg,var(--accent),#7c5cff); color:#fff; }}
  .uc-form {{ display:flex; flex-direction:column; gap:10px; }}
  .uc-form input {{ padding:10px 12px; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; outline:none; }}
  .uc-form input:focus {{ border-color:var(--accent); }}
  .uc-btn {{ padding:10px 0; border:none; border-radius:10px; font-size:13px; cursor:pointer; transition:filter 0.15s; }}
  .uc-btn.primary {{ background:linear-gradient(135deg,var(--accent),#7c5cff); color:#fff; }}
  .uc-btn.ghost {{ background:rgba(255,255,255,0.1); color:#ddd; }}
  .uc-btn.danger {{ background:rgba(204,68,68,0.2); color:#f88; }}
  .uc-btn:hover {{ filter:brightness(1.12); }}
  .uc-section {{ margin-top:16px; padding-top:14px; border-top:1px solid rgba(255,255,255,0.08); }}
  .uc-sec-title {{ font-size:12px; color:rgba(255,255,255,0.5); margin-bottom:10px; letter-spacing:0.5px; }}
  .uc-section input {{ width:100%; padding:9px 10px; margin-bottom:8px; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; outline:none; box-sizing:border-box; }}
  .uc-section input:focus {{ border-color:var(--accent); }}
  .uc-check {{ display:flex; align-items:center; gap:8px; padding:7px 0; font-size:13px; color:#ddd; cursor:pointer; }}
  .uc-check input {{ width:auto; margin:0; accent-color:var(--accent); }}
  .uc-row {{ display:flex; align-items:center; gap:8px; margin-bottom:8px; font-size:13px; color:#ddd; }}
  .uc-row label {{ width:72px; flex-shrink:0; }}
  .uc-row input {{ flex:1; margin:0; }}
  .uc-row input[type=color] {{ width:46px; padding:2px; height:30px; flex:0 0 46px; }}
  .uc-inline {{ display:none; flex-direction:column; gap:8px; margin-top:10px; padding:12px; background:rgba(255,255,255,0.05); border-radius:10px; }}
  .uc-inline.show {{ display:flex; }}
  .uc-inline input {{ width:100%; padding:9px 10px; margin:0; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; outline:none; box-sizing:border-box; }}
  .uc-inline input:focus {{ border-color:var(--accent); }}
  .uc-btns {{ display:flex; gap:8px; margin-top:10px; }}
  .uc-btns .uc-btn {{ flex:1; }}
  .modal-mask {{ display:none; position:fixed; inset:0; background:rgba(0,0,0,.55); z-index:50; align-items:center; justify-content:center; backdrop-filter:blur(4px); -webkit-backdrop-filter:blur(4px); }}
  .modal-mask.show {{ display:flex; }}
  .modal-box {{ width:340px; background:rgba(30,42,69,0.92); backdrop-filter:blur(20px); -webkit-backdrop-filter:blur(20px); border:1px solid rgba(255,255,255,0.15); border-radius:16px; padding:20px; box-shadow:0 20px 60px rgba(0,0,0,0.4); }}
  .modal-box h3 {{ font-size:15px; margin-bottom:12px; }}
  .modal-box input {{ width:100%; padding:9px 10px; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; margin-bottom:10px; outline:none; }}
  .modal-box input:focus {{ border-color:var(--accent); }}
  .modal-actions {{ display:flex; justify-content:flex-end; gap:8px; }}
  .modal-btn {{ border:none; border-radius:10px; padding:8px 16px; cursor:pointer; font-size:13px; }}
  .modal-btn.cancel {{ background:rgba(255,255,255,0.1); color:#ddd; }}
  .modal-btn.ok {{ background:linear-gradient(135deg,var(--accent),#7c5cff); color:#fff; }}
  .empty {{ text-align:center; padding:20px; color:rgba(255,255,255,0.4); font-size:13px; }}
  .toast {{ position:fixed; bottom:40px; left:50%; transform:translateX(-50%); background:rgba(0,0,0,0.85); color:#fff; padding:10px 20px; border-radius:20px; font-size:13px; opacity:0; transition:opacity 0.3s; pointer-events:none; z-index:99; backdrop-filter:blur(8px); -webkit-backdrop-filter:blur(8px); box-shadow:0 8px 24px rgba(0,0,0,0.3); }}
  .toast.show {{ opacity:1; }}
  .container {{ position:relative; flex:1; min-width:0; max-width:1100px; margin:0 auto; padding:40px 48px 24px; zoom:{font_scale}; }}
  .hero {{ text-align:center; margin-bottom:24px; }}
  .hero h1 {{ font-size:40px; font-weight:800; background:linear-gradient(120deg,#ffffff 25%,#a5d8ff 60%,#c7b9ff 100%); -webkit-background-clip:text; background-clip:text; -webkit-text-fill-color:transparent; margin-bottom:8px; letter-spacing:1px; }}
  .hero p {{ font-size:14px; color:rgba(255,255,255,0.65); }}
  .hero .lunar {{ color:rgba(255,255,255,0.5); font-size:13px; margin-top:4px; }}
  .hero .quote {{ margin-top:12px; font-size:20px; color:rgba(255,255,255,0.65); font-style:italic; max-width:680px; margin-left:auto; margin-right:auto; letter-spacing:0.3px; line-height:1.7; }}
  .info-strip {{ display:flex; justify-content:center; align-items:stretch; flex-wrap:wrap; gap:14px; margin-bottom:30px; }}
  .info-card {{ background:rgba(255,255,255,0.07); backdrop-filter:blur(16px); -webkit-backdrop-filter:blur(16px); border:1px solid rgba(255,255,255,0.14); border-radius:16px; padding:18px 20px; width:300px; max-width:100%; min-height:170px; display:flex; flex-direction:column; align-items:center; justify-content:center; text-align:center; gap:8px; box-shadow:0 8px 28px rgba(0,0,0,0.18); transition:transform 0.2s, box-shadow 0.2s; }}
  .info-card:hover {{ transform:translateY(-3px); box-shadow:0 14px 36px rgba(0,0,0,0.28); }}
  .info-card .ic {{ font-size:28px; }}
  .info-card .id {{ font-size:12px; color:rgba(255,255,255,0.55); }}
  .info-card .iv {{ font-size:17px; font-weight:700; color:#fff; }}
  .info-card .is {{ font-size:12px; color:rgba(255,255,255,0.5); margin-top:2px; line-height:1.6; }}
  .clock-card {{ gap:10px; }}
  .clock-info {{ min-width:0; }}
  .clock-time {{ font-size:22px; font-weight:700; color:#fff; font-family:monospace; letter-spacing:1px; }}
  .clock-face {{ width:80px; height:80px; border-radius:50%; flex-shrink:0; background:radial-gradient(circle at 35% 30%, rgba(255,255,255,0.14), rgba(255,255,255,0.05) 45%, rgba(0,0,0,0.25)); border:1px solid rgba(255,255,255,0.2); box-shadow:0 0 20px rgba(74,158,255,0.2), inset 0 0 14px rgba(0,0,0,0.35); position:relative; }}
  .clock-inner {{ position:absolute; inset:0; }}
  .clock-num {{ position:absolute; left:50%; top:50%; width:16px; height:16px; margin:-8px; text-align:center; line-height:16px; font-size:9px; font-weight:600; color:rgba(255,255,255,0.85); transform-origin:center; }}
  .clock-hand {{ position:absolute; left:50%; top:50%; transform-origin:50% 100%; border-radius:3px; }}
  .hour-hand {{ width:3px; height:18px; margin:-18px 0 0 -1.5px; background:rgba(255,255,255,0.92); box-shadow:0 0 3px rgba(0,0,0,0.4); }}
  .minute-hand {{ width:2px; height:24px; margin:-24px 0 0 -1px; background:rgba(255,255,255,0.85); box-shadow:0 0 3px rgba(0,0,0,0.4); }}
  .second-hand {{ width:1.5px; height:28px; margin:-28px 0 0 -0.75px; background:var(--accent); box-shadow:0 0 6px rgba(74,158,255,0.9); }}
  .second-hand::after {{ content:''; position:absolute; bottom:-5px; left:50%; width:3px; height:5px; margin-left:-1.5px; background:var(--accent); border-radius:0 0 2px 2px; }}
  .clock-center {{ position:absolute; left:50%; top:50%; width:5px; height:5px; margin:-2.5px; background:#fff; border-radius:50%; box-shadow:0 0 4px rgba(74,158,255,0.8); }}
  /* ---- 天气卡 Bing 三层格式（2026-09-22 美化）：大字温度 / 状况高低温 / 底部 5 天预报 ---- */
  .wc-temp {{ font-size:40px; font-weight:300; color:#fff; line-height:1.1; letter-spacing:-1px; }}
  .wc-cond {{ font-size:13px; color:rgba(255,255,255,0.82); margin-top:2px; }}
  #weatherCard > div {{ width:100%; box-sizing:border-box; }}
  .wc-days {{ display:grid; grid-template-columns:repeat(5, 1fr); gap:4px; margin-top:8px; }}
  .wc-days .wd {{ background:rgba(255,255,255,0.08); border-radius:8px; padding:5px 1px; text-align:center; font-size:10px; color:rgba(255,255,255,0.72); line-height:1.4; }}
  .wc-days .wd b {{ display:block; font-size:11px; color:#fff; white-space:nowrap; }}
  .wc-days .wd span {{ display:block; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }}
  .forecast {{ display:grid; grid-template-columns:repeat(3, 1fr); gap:6px; margin-top:8px; }}
  .forecast .f-day {{ background:rgba(255,255,255,0.08); border-radius:8px; padding:6px 4px; text-align:center; font-size:11px; color:rgba(255,255,255,0.7); line-height:1.5; }}
  .forecast .f-day b {{ display:block; font-size:14px; color:#fff; }}
  .forecast .f-day span {{ display:block; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }}
  .search-box {{ position:relative; margin-bottom:36px; max-width:680px; margin-left:auto; margin-right:auto; }}
  .search-box input {{ width:100%; padding:16px 56px 16px 28px; background:rgba(255,255,255,0.1); backdrop-filter:blur(12px); -webkit-backdrop-filter:blur(12px); border:1px solid rgba(255,255,255,0.18); border-radius:32px; color:#fff; font-size:16px; outline:none; transition:all 0.25s; box-shadow:0 6px 24px rgba(0,0,0,0.15); }}
  .search-box input:focus {{ background:rgba(255,255,255,0.16); border-color:var(--accent); box-shadow:0 0 0 3px rgba(74,158,255,0.2), 0 8px 32px rgba(0,0,0,0.22); }}
  .search-box input::placeholder {{ color:rgba(255,255,255,0.5); }}
  .search-engine-tag {{ position:absolute; right:18px; top:50%; transform:translateY(-50%); font-size:11px; color:rgba(255,255,255,0.4); pointer-events:none; background:rgba(255,255,255,0.08); padding:3px 10px; border-radius:12px; }}
  .suggest-box {{ position:absolute; top:calc(100% + 8px); left:0; right:0; background:rgba(30,42,69,0.95); backdrop-filter:blur(20px); -webkit-backdrop-filter:blur(20px); border:1px solid rgba(255,255,255,0.15); border-radius:14px; overflow:hidden; display:none; z-index:10; max-height:240px; overflow-y:auto; box-shadow:0 16px 48px rgba(0,0,0,0.35); }}
  .suggest-item {{ display:flex; align-items:center; gap:8px; padding:10px 16px; cursor:pointer; font-size:13px; color:#ccc; }}
  .suggest-item:hover {{ background:rgba(74,158,255,0.15); }}
  .suggest-item .s-icon {{ color:var(--accent); font-size:12px; }}
  .suggest-item .s-url {{ color:rgba(255,255,255,0.4); font-size:11px; margin-left:auto; max-width:45%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }}
  .section {{ margin-bottom:32px; }}
  .section-head {{ display:flex; align-items:center; justify-content:space-between; margin-bottom:16px; }}
  .section-head h2 {{ font-size:15px; font-weight:700; color:rgba(255,255,255,0.9); letter-spacing:0.5px; padding-left:12px; position:relative; }}
  .section-head h2::before {{ content:''; position:absolute; left:0; top:50%; transform:translateY(-50%); width:4px; height:16px; border-radius:2px; background:linear-gradient(180deg,var(--accent),#7c5cff); }}
  .section-actions {{ display:flex; gap:8px; }}
  .btn-mini {{ padding:5px 14px; background:rgba(255,255,255,0.1); border:1px solid rgba(255,255,255,0.15); border-radius:14px; color:#ccc; font-size:12px; cursor:pointer; transition:all 0.15s; }}
  .btn-mini:hover {{ background:rgba(255,255,255,0.18); color:#fff; }}
  .btn-mini.primary {{ background:linear-gradient(135deg,var(--accent),#7c5cff); border-color:transparent; color:#fff; }}
  .btn-mini.primary:hover {{ filter:brightness(1.12); }}
  .top-sites {{ display:grid; grid-template-columns:repeat(auto-fill, minmax(110px, 1fr)); gap:14px; }}
  .top-site {{ position:relative; display:flex; flex-direction:column; align-items:center; padding:16px 8px 14px; background:rgba(255,255,255,0.06); backdrop-filter:blur(12px); -webkit-backdrop-filter:blur(12px); border:1px solid rgba(255,255,255,0.1); border-radius:16px; transition:all 0.22s; }}
  .top-site:hover {{ background:rgba(255,255,255,0.11); transform:translateY(-4px); box-shadow:0 10px 28px rgba(0,0,0,0.25); }}
  .top-site-link {{ display:flex; flex-direction:column; align-items:center; text-decoration:none; color:#e0e0e0; }}
  .top-site .icon {{ font-size:28px; margin-bottom:8px; width:52px; height:52px; display:flex; align-items:center; justify-content:center; background:linear-gradient(135deg, rgba(255,255,255,0.16), rgba(255,255,255,0.04)); border:1px solid rgba(255,255,255,0.12); border-radius:16px; transition:transform 0.22s; box-shadow:0 4px 12px rgba(0,0,0,0.15); }}
  .top-site:hover .icon {{ transform:scale(1.08); }}
  .top-site .name {{ font-size:12px; color:rgba(255,255,255,0.85); }}
  .qs-del {{ position:absolute; top:4px; right:4px; width:20px; height:20px; border-radius:50%; border:none; background:rgba(204,68,68,0.85); color:#fff; font-size:12px; line-height:1; cursor:pointer; display:none; }}
  .top-site.editing .qs-del, #quickSites.editing .qs-del {{ display:block; }}
  .qs-add {{ display:flex; flex-direction:column; align-items:center; justify-content:center; padding:16px 8px; background:rgba(255,255,255,0.03); border:1px dashed rgba(255,255,255,0.25); border-radius:16px; color:rgba(255,255,255,0.5); font-size:12px; cursor:pointer; transition:all 0.2s; min-height:96px; }}
  .qs-add:hover {{ background:rgba(255,255,255,0.08); color:#fff; border-color:var(--accent); }}
  .qs-add .plus {{ font-size:24px; margin-bottom:4px; }}
  .qs-form {{ display:none; margin-top:12px; padding:14px; background:rgba(255,255,255,0.06); border-radius:12px; gap:8px; }}
  .qs-form.show {{ display:flex; flex-wrap:wrap; }}
  .qs-form input {{ flex:1; min-width:140px; padding:9px 12px; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:10px; color:#fff; font-size:13px; outline:none; }}
  .qs-form input:focus {{ border-color:var(--accent); }}
  .feed-list {{ display:grid; grid-template-columns:repeat(auto-fill, minmax(280px, 1fr)); gap:10px; }}
  /* 资讯分栏阅读（2026-09-19）：左列表 + 右内容，点一条看一条，不跳标签 */
  .feed-split-head {{ display:flex; align-items:center; gap:10px; margin-bottom:14px; }}
  .feed-split-head .feed-split-title-text {{ font-size:15px; font-weight:700; color:rgba(255,255,255,0.9); }}
  .feed-split {{ display:flex; gap:14px; align-items:flex-start; }}
  .feed-split-list {{ width:300px; flex-shrink:0; max-height:72vh; overflow-y:auto; display:flex; flex-direction:column; gap:8px; padding-right:4px; }}
  .feed-split-list::-webkit-scrollbar {{ width:6px; }}
  .feed-split-list::-webkit-scrollbar-thumb {{ background:rgba(255,255,255,0.2); border-radius:3px; }}
  .fs-split-item {{ padding:10px 12px; background:rgba(255,255,255,0.05); border:1px solid rgba(255,255,255,0.08); border-radius:10px; cursor:pointer; transition:all 0.15s; }}
  .fs-split-item:hover {{ background:rgba(255,255,255,0.1); }}
  .fs-split-item.active {{ background:rgba(74,158,255,0.18); border-color:var(--accent); }}
  .fs-split-title {{ display:block; font-size:13px; line-height:1.4; color:#e0e0e0; }}
  .fs-split-meta {{ display:block; font-size:11px; color:rgba(255,255,255,0.45); margin-top:2px; }}
  .feed-split-read {{ flex:1; min-width:0; background:rgba(255,255,255,0.05); border:1px solid rgba(255,255,255,0.08); border-radius:14px; padding:20px 24px; max-height:72vh; overflow-y:auto; }}
  .feed-split-read::-webkit-scrollbar {{ width:6px; }}
  .feed-split-read::-webkit-scrollbar-thumb {{ background:rgba(255,255,255,0.2); border-radius:3px; }}
  .feed-item {{ display:flex; flex-direction:column; padding:14px 16px; background:rgba(255,255,255,0.05); backdrop-filter:blur(8px); -webkit-backdrop-filter:blur(8px); border:1px solid rgba(255,255,255,0.08); border-radius:12px; cursor:pointer; transition:all 0.2s; text-decoration:none; color:#e0e0e0; }}
  .feed-item:hover {{ background:rgba(255,255,255,0.1); transform:translateY(-2px); box-shadow:0 8px 20px rgba(0,0,0,0.2); }}
  .feed-title {{ font-size:13px; font-weight:500; line-height:1.4; display:-webkit-box; -webkit-line-clamp:2; -webkit-box-orient:vertical; overflow:hidden; margin-bottom:4px; }}
  .feed-meta {{ font-size:11px; color:rgba(255,255,255,0.5); }}
  .recent-list {{ display:grid; grid-template-columns:repeat(auto-fill, minmax(200px, 1fr)); gap:10px; }}
  .recent-item {{ display:flex; flex-direction:column; padding:14px 16px; background:rgba(255,255,255,0.05); backdrop-filter:blur(8px); -webkit-backdrop-filter:blur(8px); border:1px solid rgba(255,255,255,0.08); border-radius:12px; cursor:pointer; transition:all 0.2s; text-decoration:none; color:#e0e0e0; }}
  .recent-item:hover {{ background:rgba(255,255,255,0.1); transform:translateY(-2px); box-shadow:0 8px 20px rgba(0,0,0,0.2); }}
  .recent-title {{ font-size:13px; font-weight:500; white-space:nowrap; overflow:hidden; text-overflow:ellipsis; margin-bottom:2px; }}
  .recent-url {{ font-size:11px; color:rgba(255,255,255,0.5); white-space:nowrap; overflow:hidden; text-overflow:ellipsis; }}
  .footer {{ display:flex; justify-content:center; gap:12px; padding:20px 0 8px; border-top:1px solid rgba(255,255,255,0.08); margin-top:8px; }}
</style>
</head>
<body>
  {bg_layer_html}
  <div class="icon-bar" id="iconBar">
    <button class="icon-btn" data-panel="feed" title="资讯">📰</button>
    <button class="icon-btn" data-panel="bookmark" title="书签">🔖</button>
    <button class="icon-btn" data-panel="todo" title="待办">📝</button>
    <button class="icon-btn" data-panel="recent" title="最近访问">🕘</button>
    <button class="icon-btn" data-panel="quick" title="快捷访问">⚡</button>
    <a class="icon-btn" href="world-nav.html" title="世界导航">🌐</a>
    <button class="icon-btn" id="aiBtn" title="AI 助手">🤖</button>
    <button class="icon-btn" id="settingsBtn" title="设置">⚙️</button>
    <button class="icon-btn bottom" data-panel="profile" title="我的">👤</button>
  </div>
  <div class="main-layout">
    <div class="side-panel" id="sidePanel">
      <div class="panel-head">
        <h2 id="panelTitle">📰 资讯</h2>
        <div class="panel-actions">
          <button class="btn-mini" id="feedHideRead" style="display:none">只看未读</button>
          <button class="fs-close" id="panelClose" title="关闭面板">×</button>
        </div>
      </div>
      <div class="panel-body" id="panelBody"></div>
    </div>
    <div class="container" id="mainContainer">
      <div class="hero">
        <h1 id="greeting">早上好，高哥 👋</h1>
        <p id="dateStr"></p>
        <p class="lunar" id="lunarText"></p>
        <p class="quote" id="quoteText"></p>
      </div>
      <div class="info-strip">
        <div class="info-card clock-card">
          <div class="clock-face">
            <div class="clock-inner" id="clockInner"></div>
            <div class="clock-hand hour-hand" id="hourHand"></div>
            <div class="clock-hand minute-hand" id="minuteHand"></div>
            <div class="clock-hand second-hand" id="secondHand"></div>
            <div class="clock-center"></div>
          </div>
          <div class="clock-info">
            <div class="iv clock-time" id="clockCardTime">--:--:--</div>
            <div class="id" id="clockCardDate"></div>
            <div class="is" id="clockCardLunar"></div>
          </div>
        </div>
        <div class="info-card" id="weatherCard">
          <span class="ic">🌤️</span>
          <div>
            <div class="wc-temp" id="weatherMain">--°C</div>
            <div class="wc-cond" id="weatherCond">天气加载中…</div>
            <div class="id" id="weatherCity">永济</div>
            <div class="is" id="weatherDetail"></div>
            <div class="wc-days" id="weatherDays"></div>
          </div>
        </div>
        <div class="info-card" id="locationCard">
          <div>
            <div class="iv" id="locationMain">定位中…</div>
            <div class="id" id="locationDetail"></div>
            <div class="id" id="locationCoord"></div>
            <div class="is" id="locationIp"></div>
            <div class="is" id="locationTime"></div>
            <div class="is" id="locationSun"></div>
            <div class="is" id="locationDayLen"></div>
          </div>
        </div>
      </div>
      <div class="search-box">
        <input type="text" id="searchInput" placeholder="搜索或输入网址..." autocomplete="off" spellcheck="false">
        <span class="search-engine-tag" id="searchEngineTag"></span>
        <div class="suggest-box" id="suggestBox"></div>
      </div>
      <div class="section">
        <div class="section-head">
          <h2>⚡ 快捷访问</h2>
          <div class="section-actions">
            <button class="btn-mini" id="quickEditBtn">编辑</button>
          </div>
        </div>
        <div class="top-sites" id="quickSites">
          {quick_grid}
          <div class="qs-add" id="qsAddBtn"><span class="plus">＋</span><span>添加</span></div>
        </div>
        <div class="qs-form" id="qsForm">
          <input type="text" id="qsName" placeholder="名称（如 网易）">
          <input type="text" id="qsUrl" placeholder="网址（如 https://www.163.com）">
          <input type="text" id="qsIcon" placeholder="图标 emoji（如 🎵）" style="max-width:120px">
          <button class="btn-mini primary" id="qsSave">保存</button>
          <button class="btn-mini" id="qsCancel">取消</button>
        </div>
      </div>
      <div class="section" id="feedSection">
        <div class="section-head">
          <h2>📰 资讯</h2>
          <div class="section-actions">
            <button class="btn-mini" id="feedMoreBtn">更多</button>
          </div>
        </div>
        <div class="feed-list" id="feedList"></div>
      </div>
      <div class="section">
        <div class="section-head">
          <h2>🕘 最近访问</h2>
        </div>
        <div class="recent-list" id="recentList"></div>
      </div>
      <div class="footer">
        <span style="font-size:12px;color:rgba(255,255,255,0.35);line-height:30px;">Shift Browser v{sb_version}</span>
      </div>
    </div>
  </div>
  <div class="toast" id="toast"></div>
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
  window.onerror = function(msg, src, line) {{
    var d = document.createElement('div');
    d.style.cssText = 'position:fixed;top:0;left:0;z-index:99999;background:#c00;color:#fff;font-size:12px;padding:8px;max-width:100%;';
    d.textContent = 'JS错误: ' + msg + ' @行' + line;
    document.body.appendChild(d);
  }};
  // HTML 转义（属性/文本）：& < > " ' \ 全量转义，防属性断裂与标签注入。
  // 注意：单引号必须转义——feed URL 来自远端 RSS，clean_text 会把 &quot;/&#39; 等
  // 实体解码成裸引号，esc 不转义 ' 则 href="...' 类注入仍可逃逸。
  function esc(s) {{ return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;').replace(/\\/g,'&#92;'); }}
  // JS 字符串转义（单引号字符串内联场景，如 onclick="showInlineReader('...')"）
  function escJs(s) {{ return String(s).replace(/\\/g,'\\\\').replace(/'/g,"\\'").replace(/\n/g,'\\n').replace(/\r/g,'\\r'); }}
  // 动态背景（Bing 每日壁纸，Ken Burns 缓慢缩放；预加载成功才换图，失败保持渐变）
  var BING_WALLPAPERS = {wallpapers_json};
  function loadBg() {{
    var bgEl = document.getElementById('bgLayer');
    if (!bgEl || !BING_WALLPAPERS.length) return;
    var pick = BING_WALLPAPERS[Math.floor(Math.random() * BING_WALLPAPERS.length)];
    var img = new Image();
    img.onload = function() {{ bgEl.style.backgroundImage = 'url("' + pick + '")'; }};
    img.src = pick;
  }}
  loadBg();

  // 应用账号偏好（主题色/壁纸/农历/每日一句/天气显隐）——登录后写 settings 表，读出来动态生效
  if (window.__TAURI_INTERNALS__) {{
    window.__TAURI_INTERNALS__.invoke('get_all_settings').then(function(s) {{
      var map = {{}};
      (s || []).forEach(function(x) {{ map[x.key] = x.value; }});
      var accent = map.theme_color || '#4a9eff';
      document.documentElement.style.setProperty('--accent', accent);
      var bgEl = document.getElementById('bgLayer');
      var w = map.wallpaper || '';
      if (w && bgEl) {{
        var img = new Image();
        img.onload = function() {{ bgEl.style.backgroundImage = 'url("' + w + '")'; }};
        img.src = w;
      }}
      function hideIfOff(key, id) {{
        var v = map[key];
        if (v === '0' || v === 'false') {{
          var el = document.getElementById(id);
          if (el) el.style.display = 'none';
        }}
      }}
      hideIfOff('show_lunar', 'lunarText');
      hideIfOff('show_quote', 'quoteText');
      hideIfOff('show_weather', 'weatherCard');
    }}).catch(function() {{}});
  }}

  // 问候语 + 日期（JS 动态生成，省得后端依赖时区库）
  var now = new Date();
  var h = now.getHours();
  var greet = h < 5 ? '夜深了' : h < 11 ? '早上好' : h < 13 ? '中午好' : h < 18 ? '下午好' : '晚上好';
  document.getElementById('greeting').textContent = greet + '，高哥 👋';
  // 2026-09-19：问候语名字自动化——登录用户有昵称/用户名就用用户的，没登录保持默认
  if (window.__TAURI_INTERNALS__) {{
    window.__TAURI_INTERNALS__.invoke('user_get_current').then(function(r) {{
      if (r && r.ok && r.user) {{
        var nm = r.user.nickname || r.user.username;
        if (nm) document.getElementById('greeting').textContent = greet + '，' + nm + ' 👋';
      }}
    }}).catch(function() {{}});
  }}
  var wd = ['日','一','二','三','四','五','六'][now.getDay()];
  // 注意：必须用 innerHTML——dateStr 里嵌了时钟 span，textContent 不当 HTML 解析，
  // 会把 "<span id=\"clockTime\"></span>" 当字面文本显示，且 span 不创建导致时钟不显示。
  // 内容全是 JS 内部生成的数字/星期（无用户输入），无 XSS 风险。
  document.getElementById('dateStr').innerHTML =
    now.getFullYear() + '年' + (now.getMonth()+1) + '月' + now.getDate() + '日 星期' + wd + ' <span id="clockTime"></span>';

  // ---- 实时时钟（数字 + 指针）----
  function tickClock() {{
    var d = new Date();
    var hh = String(d.getHours()).padStart(2, '0');
    var mm = String(d.getMinutes()).padStart(2, '0');
    var ss = String(d.getSeconds()).padStart(2, '0');
    var el = document.getElementById('clockTime');
    if (el) el.textContent = hh + ':' + mm + ':' + ss;
    var ct = document.getElementById('clockCardTime');
    if (ct) ct.textContent = hh + ':' + mm + ':' + ss;
    var cd = document.getElementById('clockCardDate');
    if (cd) cd.textContent = (d.getMonth()+1) + '月' + d.getDate() + '日 星期' + ['日','一','二','三','四','五','六'][d.getDay()];
  }}
  tickClock();
  setInterval(tickClock, 1000);

  // 指针时钟刻度
  (function() {{
    var inner = document.getElementById('clockInner');
    if (!inner) return;
    var nums = '';
    for (var i = 0; i < 12; i++) {{
      var ang = i * 30 * Math.PI / 180;
      var x = 50 + 34 * Math.sin(ang), y = 50 - 34 * Math.cos(ang);
      nums += '<span class="clock-num" style="left:' + x + '%;top:' + y + '%">' + (i === 0 ? 12 : i) + '</span>';
    }}
    inner.innerHTML = nums;
  }})();
  var lastAnalogTick = 0;
  function updateAnalogClock(ts) {{
    if (ts - lastAnalogTick < 50) {{ requestAnimationFrame(updateAnalogClock); return; }}
    lastAnalogTick = ts;
    var d = new Date();
    var hh = d.getHours() % 12, mm = d.getMinutes(), ss = d.getSeconds(), ms = d.getMilliseconds();
    var hourDeg = (hh + mm/60 + ss/3600) * 30;
    var minDeg = (mm + ss/60 + ms/60000) * 6;
    var secDeg = (ss + ms/1000) * 6;
    var e1 = document.getElementById('hourHand'), e2 = document.getElementById('minuteHand'), e3 = document.getElementById('secondHand');
    if (e1) e1.style.transform = 'rotate(' + hourDeg + 'deg)';
    if (e2) e2.style.transform = 'rotate(' + minDeg + 'deg)';
    if (e3) e3.style.transform = 'rotate(' + secDeg + 'deg)';
    requestAnimationFrame(updateAnalogClock);
  }}
  requestAnimationFrame(updateAnalogClock);

  // ---- 农历（浏览器内置 Intl 中国农历）----
  function lunarDayName(n) {{
    var digits = ['','一','二','三','四','五','六','七','八','九','十'];
    var num = parseInt(n, 10);
    if (num === 10) return '初十';
    if (num < 10) return '初' + digits[num];
    if (num === 20) return '二十';
    if (num < 20) return '十' + digits[num % 10];
    if (num === 30) return '三十';
    return '廿' + digits[num % 10];
  }}
  function renderLunar() {{
    var el = document.getElementById('lunarText');
    var cl = document.getElementById('clockCardLunar');
    try {{
      var fmt = new Intl.DateTimeFormat('zh-CN-u-ca-chinese', {{ year:'numeric', month:'long', day:'numeric' }});
      var parts = fmt.formatToParts(new Date());
      var year = '', month = '', day = '';
      for (var i = 0; i < parts.length; i++) {{
        var p = parts[i];
        if (p.type === 'yearName') year = p.value;
        else if (p.type === 'year' || p.type === 'relatedYear') {{ if (!year) year = p.value; }}
        else if (p.type === 'month') month = p.value;
        else if (p.type === 'day') day = p.value;
      }}
      var lunarStr = '农历 ' + year + '年 ' + month + lunarDayName(day);
      if (el) el.textContent = lunarStr;
      if (cl) cl.textContent = lunarStr;
    }} catch(e) {{
      if (el) el.textContent = '';
      if (cl) cl.textContent = '';
    }}
  }}
  renderLunar();

  // ---- 每日一句（内置精选）----
  var QUOTES = ['路虽远，行则将至；事虽难，做则必成。','身体是革命的本钱，健康是最大的财富。','不积跬步，无以至千里；不积小流，无以成江海。','生活明朗，万物可爱，人间值得，未来可期。','种一棵树最好的时间是十年前，其次是现在。','知足者常乐，能忍者自安。','千里之行，始于足下。','日日行，不怕千万里；常常做，不怕千万事。','心若向阳，无谓悲伤；心若向暖，安之若素。','今日事，今日毕；明日事，明日计。'];
  var quoteEl = document.getElementById('quoteText');
  if (quoteEl) {{
    quoteEl.textContent = '💬 ' + QUOTES[new Date().getDate() % QUOTES.length];
    // 调试探针：把实际渲染字号上报日志，排查"改字号不生效"（2026-09-19）
    try {{ window.__TAURI_INTERNALS__.invoke('front_log', {{ msg: 'QUOTE_FONT=' + getComputedStyle(quoteEl).fontSize + '|css=' + (document.styleSheets[0] ? document.styleSheets[0].cssRules.length : -1) }}); }} catch(e) {{}}
  }}

  // ---- 天气（wttr.in 免费 API，无需 key；英文描述映射中文）----
  var WEATHER_ZH = {{ 'Clear':'晴','Sunny':'晴','Partly cloudy':'多云','Cloudy':'阴','Overcast':'阴天','Mist':'薄雾','Fog':'雾','Light drizzle':'毛毛雨','Drizzle':'毛毛雨','Light rain':'小雨','Moderate rain':'中雨','Heavy rain':'大雨','Torrential rain shower':'暴雨','Light rain shower':'阵雨','Moderate or heavy rain shower':'中到大雨','Patchy rain possible':'局部有雨','Thundery outbreaks possible':'雷阵雨','Thunderstorm':'雷阵雨','Light snow':'小雪','Moderate snow':'中雪','Heavy snow':'大雪','Light sleet':'雨夹雪','Freezing fog':'冻雾','Hail':'冰雹','Windy':'大风','Light rain with thunder':'雷雨','Smoky haze':'烟霾','Haze':'霾','Patches of fog':'局部有雾','Thundery outbreaks in nearby':'附近雷阵雨','Patchy light drizzle':'零星毛毛雨','Moderate or heavy rain with thunder':'雷中到大雨' }};
  function weatherZh(desc) {{
    if (!desc) return '';
    var key = String(desc).trim();
    if (WEATHER_ZH[key]) return WEATHER_ZH[key];
    var lk = key.toLowerCase();
    for (var k in WEATHER_ZH) {{ if (k.toLowerCase() === lk) return WEATHER_ZH[k]; }}
    return key;
  }}
  function loadWeather() {{
    var mainEl = document.getElementById('weatherMain');
    var detailEl = document.getElementById('weatherDetail');
    var cityEl = document.getElementById('weatherCity');
    if (!mainEl) return;
    fetch('https://wttr.in/Yuncheng?format=j1&lang=zh')
      .then(function(r) {{ return r.json(); }})
      .then(function(data) {{
        var cur = data.current_condition && data.current_condition[0];
        if (!cur) throw new Error('no data');
        var desc = cur.lang_zh && cur.lang_zh[0] ? cur.lang_zh[0].value : (cur.weatherDesc && cur.weatherDesc[0] ? cur.weatherDesc[0].value : '');
        mainEl.textContent = cur.temp_C + '°';
        var condEl = document.getElementById('weatherCond');
        var d0 = data.weather && data.weather[0] ? data.weather[0] : null;
        if (condEl) {{
          var hi = d0 ? d0.maxtempC : '';
          var lo = d0 ? d0.mintempC : '';
          condEl.textContent = weatherZh(desc) + (hi !== '' && lo !== '' ? ' · 最高 ' + hi + '° / 最低 ' + lo + '°' : '');
        }}
        detailEl.textContent = '湿度 ' + cur.humidity + '% · 体感 ' + cur.FeelsLikeC + '°C';
        var daysEl = document.getElementById('weatherDays');
        var WK = '日一二三四五六';
        function renderDays(list) {{
          if (!daysEl || !list || !list.length) return;
          daysEl.innerHTML = list.map(function(d) {{
            return '<div class="wd"><b>' + d.label + '</b><span>' + d.mx + '°/' + d.mn + '°</span><span>' + d.txt + '</span></div>';
          }}).join('');
        }}
        /* 兜底：先用 wttr 天级数据（通常 3 天）*/
        if (daysEl && data.weather && data.weather.length) {{
          renderDays(data.weather.map(function(d, i) {{
            var dt = new Date(d.date + 'T00:00:00');
            var dDesc = d.hourly && d.hourly[0] && d.hourly[0].lang_zh && d.hourly[0].lang_zh[0] ? d.hourly[0].lang_zh[0].value : '';
            return {{ label: i === 0 ? '今天' : '周' + WK[dt.getDay()], mx: d.maxtempC, mn: d.mintempC, txt: weatherZh(dDesc) }};
          }}));
        }}
        /* 升级：Open-Meteo 精确补到 5 天（需 CSP connect-src 放行 api.open-meteo.com）*/
        if (daysEl) {{
          fetch('https://api.open-meteo.com/v1/forecast?latitude=35.02&longitude=111.00&daily=weather_code,temperature_2m_max,temperature_2m_min&timezone=Asia%2FShanghai&forecast_days=5')
            .then(function(r) {{ return r.json(); }})
            .then(function(om) {{
              var dd = om.daily; if (!dd || !dd.time) return;
              var WM = {{ 0:'晴',1:'晴间多云',2:'多云',3:'阴',45:'雾',48:'雾凇',51:'毛毛雨',53:'毛毛雨',55:'密毛雨',56:'冻雨',57:'冻雨',61:'小雨',63:'中雨',65:'大雨',66:'冻雨',67:'冻雨',71:'小雪',73:'中雪',75:'大雪',77:'雪粒',80:'阵雨',81:'中阵雨',82:'强阵雨',85:'阵雪',86:'强阵雪',95:'雷阵雨',96:'雷阵雨',99:'强雷暴' }};
              var list = [];
              var n = dd.time.length < 5 ? dd.time.length : 5;
              for (var i = 0; i < n; i++) {{
                var dt = new Date(dd.time[i] + 'T00:00:00');
                list.push({{ label: i === 0 ? '今天' : '周' + WK[dt.getDay()], mx: dd.temperature_2m_max ? dd.temperature_2m_max[i] : '', mn: dd.temperature_2m_min ? dd.temperature_2m_min[i] : '', txt: WM[dd.weather_code ? dd.weather_code[i] : 0] || '' }});
              }}
              renderDays(list);
            }})
            .catch(function() {{}});
        }}
      }})
      .catch(function() {{
        mainEl.textContent = '天气获取失败';
        detailEl.textContent = '请检查网络或稍后重试';
      }});
  }}
  loadWeather();

  // ---- 定位（ipinfo.io 免费；中文映射）----
  var CN_REGION = {{ 'Shanxi':'山西','Beijing':'北京','Shanghai':'上海','Tianjin':'天津','Chongqing':'重庆','Guangdong':'广东','Zhejiang':'浙江','Jiangsu':'江苏','Shandong':'山东','Henan':'河南','Sichuan':'四川','Hubei':'湖北','Hunan':'湖南','Fujian':'福建','Hebei':'河北','Shaanxi':'陕西','Anhui':'安徽','Jiangxi':'江西','Guangxi':'广西','Yunnan':'云南','Guizhou':'贵州','Gansu':'甘肃','Liaoning':'辽宁','Jilin':'吉林','Heilongjiang':'黑龙江','Inner Mongolia':'内蒙古','Xinjiang':'新疆','Tibet':'西藏','Ningxia':'宁夏','Qinghai':'青海','Hainan':'海南','Hong Kong':'香港','Macau':'澳门','Taiwan':'台湾' }};
  var CN_CITY = {{ 'Taiyuan':'太原','Datong':'大同','Yangquan':'阳泉','Changzhi':'长治','Jincheng':'晋城','Shuozhou':'朔州','Jinzhong':'晋中','Yuncheng':'运城','Xinzhou':'忻州','Linfen':'临汾','Lvliang':'吕梁','Beijing':'北京','Shanghai':'上海','Guangzhou':'广州','Shenzhen':'深圳','Hangzhou':'杭州','Nanjing':'南京','Wuhan':'武汉','Chengdu':'成都','Xi\'an':'西安','Zhengzhou':'郑州','Jinan':'济南','Qingdao':'青岛','Tianjin':'天津','Chongqing':'重庆','Suzhou':'苏州','Xiamen':'厦门','Changsha':'长沙','Hefei':'合肥','Nanchang':'南昌','Fuzhou':'福州','Kunming':'昆明','Guiyang':'贵阳','Lanzhou':'兰州','Harbin':'哈尔滨','Changchun':'长春','Shenyang':'沈阳','Shijiazhuang':'石家庄','Hohhot':'呼和浩特','Yinchuan':'银川','Xining':'西宁','Urumqi':'乌鲁木齐','Lhasa':'拉萨','Haikou':'海口','Nanning':'南宁','Taipei':'台北' }};
  function mapOrg(org) {{
    if (!org) return '';
    var o = String(org).toUpperCase();
    if (o.indexOf('CHINANET') >= 0 || o.indexOf('CHINA TELECOM') >= 0) return '中国电信';
    if (o.indexOf('UNICOM') >= 0 || o.indexOf('CHINA UNICOM') >= 0) return '中国联通';
    if (o.indexOf('CHINA MOBILE') >= 0 || o.indexOf('CMNET') >= 0 || o.indexOf('CMCC') >= 0) return '中国移动';
    if (o.indexOf('CERNET') >= 0) return '教育网';
    var cleaned = org.replace(/^AS\d+\s*/i, '');
    return cleaned.length > 14 ? cleaned.slice(0, 14) : cleaned;
  }}
  function formatLocalTime(timeZone) {{
    try {{ return new Intl.DateTimeFormat('zh-CN', {{ timeZone:timeZone, hour:'2-digit', minute:'2-digit', hour12:false }}).format(new Date()); }} catch(e) {{ return ''; }}
  }}
  function tzOffsetHours(timeZone, date) {{
    try {{
      var dtf = new Intl.DateTimeFormat('en-US', {{ timeZone:timeZone, hour12:false, year:'numeric', month:'2-digit', day:'2-digit', hour:'2-digit', minute:'2-digit', second:'2-digit' }});
      var parts = dtf.formatToParts(date);
      var map = {{}};
      for (var i = 0; i < parts.length; i++) map[parts[i].type] = parts[i].value;
      var asUTC = Date.UTC(+map.year, +map.month - 1, +map.day, +map.hour % 24, +map.minute, +map.second);
      return (asUTC - date.getTime()) / 3600000;
    }} catch(e) {{ return 0; }}
  }}
  function calcSunTimes(lat, lng, date, timeZone) {{
    var rad = Math.PI / 180;
    var start = new Date(date.getFullYear(), 0, 1);
    var dayOfYear = Math.floor((date - start) / 86400000) + 1;
    var latRad = lat * rad;
    var decl = 23.44 * rad * Math.sin(2 * Math.PI * (284 + dayOfYear) / 365);
    var cosH = (Math.sin(-0.833 * rad) - Math.sin(latRad) * Math.sin(decl)) / (Math.cos(latRad) * Math.cos(decl));
    if (cosH > 1 || cosH < -1) return null;
    var H = Math.acos(cosH) / rad;
    var sunriseSolar = 12 - H / 15;
    var sunsetSolar = 12 + H / 15;
    var tz = tzOffsetHours(timeZone, date);
    var lngCorr = lng / 15 - tz;
    var norm = function(h2) {{ return ((h2 % 24) + 24) % 24; }};
    var fmt = function(h2) {{
      var hh = Math.floor(h2);
      var mm = Math.round((h2 - hh) * 60);
      return String(hh).padStart(2, '0') + ':' + String(mm).padStart(2, '0');
    }};
    var rise = norm(sunriseSolar + lngCorr);
    var set = norm(sunsetSolar + lngCorr);
    var dayLenH = sunsetSolar - sunriseSolar;
    var dh = Math.floor(dayLenH);
    var dm = Math.round((dayLenH - dh) * 60);
    return {{ rise: fmt(rise), set: fmt(set), dayLen: dh + '小时' + dm + '分' }};
  }}
  function loadLocation() {{
    var mainEl = document.getElementById('locationMain');
    var detailEl = document.getElementById('locationDetail');
    var coordEl = document.getElementById('locationCoord');
    var ipEl = document.getElementById('locationIp');
    var timeEl = document.getElementById('locationTime');
    var sunEl = document.getElementById('locationSun');
    var dayLenEl = document.getElementById('locationDayLen');
    if (!mainEl) return;
    fetch('https://ipinfo.io/json')
      .then(function(r) {{ return r.json(); }})
      .then(function(data) {{
        var region = CN_REGION[data.region] || data.region || '';
        var city = CN_CITY[data.city] || data.city || '';
        var country = data.country === 'CN' ? '中国' : (data.country || '');
        var tz = data.timezone || '';
        mainEl.textContent = (data.country === 'CN' ? (region ? region + ' · ' : '') + (city || '未知') : (country ? country + ' · ' : '') + (city || '未知'));
        detailEl.textContent = country ? country + ' · ' + mapOrg(data.org || '') : mapOrg(data.org || '');
        var locParts = (data.loc || '').split(',');
        var lat = parseFloat(locParts[0]);
        var lng = parseFloat(locParts[1]);
        coordEl.textContent = (data.loc && !isNaN(lat) && !isNaN(lng)) ? '经纬度 ' + lat.toFixed(2) + ', ' + lng.toFixed(2) : '';
        ipEl.textContent = data.ip ? 'IP 地址 ' + data.ip : '';
        if (tz) timeEl.textContent = '当地时间 ' + formatLocalTime(tz);
        if (!isNaN(lat) && !isNaN(lng)) {{
          var sun = calcSunTimes(lat, lng, new Date(), tz);
          if (sun) {{
            sunEl.textContent = '日出 ' + sun.rise + ' · 日落 ' + sun.set;
            dayLenEl.textContent = '昼长 ' + sun.dayLen;
          }}
        }}
      }})
      .catch(function() {{
        mainEl.textContent = '位置获取失败';
        detailEl.textContent = '请检查网络';
      }});
  }}
  loadLocation();

  // ---- toast 提示条 ----
  var toastTimer = null;
  function showToast(msg) {{
    var t = document.getElementById('toast');
    if (!t) return;
    t.textContent = msg;
    t.classList.add('show');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function() {{ t.classList.remove('show'); }}, 1800);
  }}

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
      h += '<div class=\"item q-item\"><a class=\"q-link\" href=\"' + esc(f.url) + '\"><span class=\"it-title\">' + esc(f.icon || '⭐') + ' ' + esc(f.name) + '</span><span class=\"it-url\">' + esc(f.url) + '</span></a><button class=\"q-del\" data-url=\"' + esc(f.url) + '\" title=\"删除收藏\">✕</button></div>';
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
      return '<a class=\"item fs-item' + (isRead ? ' read' : '') + '\" href=\"' + esc(f[1]) + '\" data-url=\"' + esc(f[1]) + '\">' +
        '<span class=\"it-title\">' + esc(f[0]) + '</span>' +
        '<span class=\"it-url\">' + esc(f[2]) + (f[3] ? ' · ' + timeAgo(f[3]) : '') + '</span>' +
        '<span class=\"fs-read\" data-url=\"' + esc(f[1]) + '\" title=\"📖 大字阅读（提取正文，面板内就地看\" onclick=\"event.preventDefault();event.stopPropagation();return showInlineReader(\'' + escJs(f[1]) + '\'),false;\">📖</span></a>';
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
      // 2026-09-19：点资讯标题不再导航新页面，改为在主区（右侧）打开分栏阅读，
      // 左侧面板保持打开，随时点下一条；📖 按钮走原有内嵌阅读（stopPropagation 已隔开）
      el.addEventListener('click', function(e) {{
        e.preventDefault();
        var url = el.dataset.url;
        var f = null;
        for (var i = 0; i < FEED_DATA.length; i++) {{
          if (FEED_DATA[i][1] === url) {{ f = FEED_DATA[i]; break; }}
        }}
        if (f) openFeedSplit(f);
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
      '<a href="' + esc(f[1]) + '" target="_blank" class="ir-src">看原文 ↗</a>' +
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
    // 注意：不能 panelBody.innerHTML = panelBody.innerHTML 重渲染，
    // 会重建 DOM 丢掉 bindInline 绑定的事件（字号按钮点一次就失效）
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

  // ---- 待办（IPC 存 settings 表；启动页是 shift:// 自定义协议，localStorage 不可靠）----
  function loadTodos() {{
    if (!window.__TAURI_INTERNALS__) {{ try {{ return Promise.resolve(JSON.parse(localStorage.getItem('sb_todos') || '[]')); }} catch(e) {{ return Promise.resolve([]); }} }}
    return window.__TAURI_INTERNALS__.invoke('todo_list').then(function(json) {{
      var todos;
      try {{ todos = JSON.parse(json || '[]'); }} catch(e) {{ todos = []; }}
      // 迁移旧 localStorage 数据（早期版本待办存在 sb_todos）
      if (!todos.length) {{
        var legacy = null;
        try {{ legacy = JSON.parse(localStorage.getItem('sb_todos') || '[]'); }} catch(e) {{}}
        if (legacy && legacy.length) {{
          todos = legacy;
          saveTodos(todos).catch(function() {{}});
          try {{ localStorage.removeItem('sb_todos'); }} catch(e) {{}}
        }}
      }}
      return todos;
    }}).catch(function() {{ return []; }});
  }}
  function saveTodos(t) {{
    if (!window.__TAURI_INTERNALS__) {{ try {{ localStorage.setItem('sb_todos', JSON.stringify(t)); }} catch(e) {{}} return Promise.resolve(); }}
    return window.__TAURI_INTERNALS__.invoke('todo_save', {{ todos: JSON.stringify(t) }});
  }}
  function renderTodos() {{
    return loadTodos().then(function(todos) {{
      var h2 = '<div class="todo-panel"><div class="todo-add"><input id="todoInput" placeholder="新增待办，回车添加"><button id="todoAddBtn">加</button></div>';
      if (!todos.length) h2 += '<div class="empty">暂无待办，输入一条开始吧</div>';
      todos.forEach(function(t, i) {{
        h2 += '<div class="todo-item' + (t.done ? ' done' : '') + '"><input type="checkbox" data-i="' + i + '"' + (t.done ? ' checked' : '') + '><span class="todo-text">' + String(t.text || '').replace(/</g,'&lt;') + '</span><button class="todo-del" data-i="' + i + '">✕</button></div>';
      }});
      h2 += '</div>';
      return h2;
    }});
  }}
  function refreshTodoPanel() {{
    renderTodos().then(function(h) {{ if (panelBody) panelBody.innerHTML = h; bindTodos(); }});
  }}
  function bindTodos() {{
    var input = document.getElementById('todoInput');
    var addBtn = document.getElementById('todoAddBtn');
    function add() {{
      var v = input ? input.value.trim() : '';
      if (!v) return;
      loadTodos().then(function(todos) {{
        todos.unshift({{ text: v, done: false }});
        return saveTodos(todos);
      }}).then(refreshTodoPanel).catch(function(e) {{ showToast('保存待办失败：' + e); }});
    }}
    if (addBtn) addBtn.addEventListener('click', add);
    if (input) input.addEventListener('keydown', function(e) {{ if (e.key === 'Enter') add(); }});
    panelBody.querySelectorAll('.todo-item input[type=checkbox]').forEach(function(cb) {{
      cb.addEventListener('change', function() {{
        loadTodos().then(function(todos) {{
          if (todos[+cb.dataset.i]) todos[+cb.dataset.i].done = cb.checked;
          return saveTodos(todos);
        }}).then(refreshTodoPanel).catch(function(e) {{ showToast('保存待办失败：' + e); }});
      }});
    }});
    panelBody.querySelectorAll('.todo-del').forEach(function(btn) {{
      btn.addEventListener('click', function() {{
        loadTodos().then(function(todos) {{
          todos.splice(+btn.dataset.i, 1);
          return saveTodos(todos);
        }}).then(refreshTodoPanel).catch(function(e) {{ showToast('删除待办失败：' + e); }});
      }});
    }});
  }}

  // ---- "我的"面板（个人中心：登录/注册/资料/偏好/备份）----
  var ucUser = null;
  function renderProfile() {{
    var h = '<div class="uc-loading">⏳ 加载中…</div>';
    if (window.__TAURI_INTERNALS__) {{
      window.__TAURI_INTERNALS__.invoke('user_get_current').then(function(r) {{
        if (r && r.ok && r.user) {{
          ucUser = r.user;
          panelBody.innerHTML = renderProfileLoggedIn(r.user);
          bindProfileLoggedIn(r.user);
        }} else {{
          ucUser = null;
          panelBody.innerHTML = renderProfileLogin();
          bindProfileLogin();
        }}
      }}).catch(function() {{
        ucUser = null;
        panelBody.innerHTML = renderProfileLogin();
        bindProfileLogin();
      }});
    }} else {{
      var keys = Object.keys(window).filter(function(k){{ return k.indexOf('TAURI') >= 0 || k.indexOf('ipc') >= 0 || k.indexOf('webkit') >= 0 || k.indexOf('__') === 0; }});
      h = '<div style="padding:20px;color:#ff4444;font-size:14px;"><b>⚠ 用户系统不可用</b><br>TAURI_INTERNALS: ' + (typeof window.__TAURI_INTERNALS__) + '<br>window keys: ' + keys.join(', ') + '</div>';
    }}
    return h;
  }}

  function renderProfileLogin() {{
    var h = '<div class="uc-wrap">';
    h += '<div class="uc-tabs">';
    h += '<button class="uc-tab active" data-tab="login">登录</button>';
    h += '<button class="uc-tab" data-tab="register">注册</button>';
    h += '</div>';
    h += '<div class="uc-form" id="ucLoginForm">';
    h += '<input id="ucLoginUser" placeholder="用户名" autocomplete="off">';
    h += '<input id="ucLoginPass" type="password" placeholder="密码">';
    h += '<button class="uc-btn primary" id="ucLoginBtn">登 录</button>';
    h += '<button class="uc-btn ghost" id="ucImportBtn">📥 导入账号备份</button>';
    h += '</div>';
    h += '<div class="uc-form" id="ucRegForm" style="display:none">';
    h += '<input id="ucRegUser" placeholder="用户名（2-20位，中英文/数字/_/-）" autocomplete="off">';
    h += '<input id="ucRegPass" type="password" placeholder="密码（至少 6 位）">';
    h += '<input id="ucRegPass2" type="password" placeholder="确认密码">';
    h += '<button class="uc-btn primary" id="ucRegBtn">注 册</button>';
    h += '</div>';
    h += '</div>';
    return h;
  }}

  function bindProfileLogin() {{
    var tabs = panelBody.querySelectorAll('.uc-tab');
    tabs.forEach(function(t) {{
      t.addEventListener('click', function() {{
        tabs.forEach(function(x) {{ x.classList.remove('active'); }});
        t.classList.add('active');
        document.getElementById('ucLoginForm').style.display = t.dataset.tab === 'login' ? '' : 'none';
        document.getElementById('ucRegForm').style.display = t.dataset.tab === 'register' ? '' : 'none';
      }});
    }});
    var loginBtn = document.getElementById('ucLoginBtn');
    if (loginBtn) loginBtn.addEventListener('click', function() {{
      var u = document.getElementById('ucLoginUser').value.trim();
      var p = document.getElementById('ucLoginPass').value;
      if (!u || !p) {{ showToast('请输入用户名和密码'); return; }}
      loginBtn.disabled = true;
      window.__TAURI_INTERNALS__.invoke('user_login', {{ username: u, password: p }}).then(function(r) {{
        loginBtn.disabled = false;
        if (r && r.ok) {{ showToast(r.msg || '登录成功'); setTimeout(function() {{ location.reload(); }}, 500); }}
        else {{ showToast((r && r.msg) || '登录失败'); }}
      }}).catch(function(e) {{ loginBtn.disabled = false; showToast('登录失败：' + e); }});
    }});
    var regBtn = document.getElementById('ucRegBtn');
    if (regBtn) regBtn.addEventListener('click', function() {{
      var u = document.getElementById('ucRegUser').value.trim();
      var p = document.getElementById('ucRegPass').value;
      var p2 = document.getElementById('ucRegPass2').value;
      if (!u || !p) {{ showToast('请输入用户名和密码'); return; }}
      if (p !== p2) {{ showToast('两次密码不一致'); return; }}
      regBtn.disabled = true;
      window.__TAURI_INTERNALS__.invoke('user_register', {{ username: u, password: p }}).then(function(r) {{
        regBtn.disabled = false;
        if (r && r.ok) {{ showToast(r.msg || '注册成功'); setTimeout(function() {{ location.reload(); }}, 500); }}
        else {{ showToast((r && r.msg) || '注册失败'); }}
      }}).catch(function(e) {{ regBtn.disabled = false; showToast('注册失败：' + e); }});
    }});
    var importBtn = document.getElementById('ucImportBtn');
    if (importBtn) importBtn.addEventListener('click', function() {{
      window.__TAURI_INTERNALS__.invoke('user_import').then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '导入成功'); setTimeout(function() {{ location.reload(); }}, 500); }}
        else {{ showToast((r && r.msg) || '已取消导入'); }}
      }}).catch(function(e) {{ showToast('导入失败：' + e); }});
    }});
  }}

  function renderProfileLoggedIn(u) {{
    var p = u.prefs || {{}};
    var avatar = u.avatar || '👤';
    var name = u.nickname || u.username;
    var sig = u.signature || '这个人很懒，什么都没写';
    // 安全渲染头像：后端已清洗（禁 < > + MIME 白名单），这里再兜底——
    // data:image 白名单才当图片，其余一律 esc 后当文本，杜绝 HTML 注入（2026-09-19 修复）
    function safeAvatar(a) {{
      if (/^data:image\/(png|jpe?g|gif|webp);/i.test(a)) {{
        return '<img class="pi-avatar-img" src="' + esc(a) + '" alt="">';
      }}
      return esc(a);
    }}
    var h = '<div class="uc-wrap">';
    h += '<div class="profile-info">';
    h += '<div class="pi-avatar">' + safeAvatar(avatar) + '</div>';
    h += '<div class="pi-name">' + esc(name) + '</div>';
    h += '<div class="pi-user">@' + esc(u.username) + '</div>';
    h += '<div class="pi-sig">' + esc(sig) + '</div>';
    h += '</div>';
    h += '<div class="uc-section"><div class="uc-sec-title">✏️ 编辑资料</div>';
    h += '<input id="ucNick" placeholder="昵称" value="' + esc(u.nickname || '') + '">';
    h += '<input id="ucAvatar" placeholder="头像 emoji（如 🐶）" value="' + esc(u.avatar || '') + '">';
    h += '<input id="ucSig" placeholder="个性签名" value="' + esc(u.signature || '') + '">';
    h += '<button class="uc-btn primary" id="ucSaveProfileBtn">保存资料</button>';
    h += '</div>';
    h += '<div class="uc-section"><div class="uc-sec-title">⚙️ 偏好设置</div>';
    h += '<label class="uc-check"><input type="checkbox" id="ucShowLunar"' + (p.show_lunar ? ' checked' : '') + '> 显示农历</label>';
    h += '<label class="uc-check"><input type="checkbox" id="ucShowQuote"' + (p.show_quote ? ' checked' : '') + '> 显示每日一句</label>';
    h += '<label class="uc-check"><input type="checkbox" id="ucShowWeather"' + (p.show_weather ? ' checked' : '') + '> 显示天气</label>';
    h += '<div class="uc-row"><label>字体大小</label><input type="number" id="ucFontSize" min="12" max="22" value="' + (p.font_size || 14) + '"></div>';
    h += '<div class="uc-row"><label>天气城市</label><input id="ucCity" value="' + esc(p.weather_city || '运城') + '"></div>';
    h += '<div class="uc-row"><label>主题色</label><input type="color" id="ucThemeColor" value="' + (p.theme_color || '#4a9eff') + '"></div>';
    h += '<div class="uc-row"><label>壁纸</label><input id="ucWallpaper" placeholder="图片链接（留空=默认）" value="' + esc(p.wallpaper || '') + '"></div>';
    h += '<button class="uc-btn primary" id="ucSavePrefsBtn">保存偏好</button>';
    h += '</div>';
    h += '<div class="uc-section"><div class="uc-sec-title">🔐 账号</div>';
    h += '<div class="uc-btns"><button class="uc-btn ghost" id="ucExportBtn">📤 导出备份</button>';
    h += '<button class="uc-btn ghost" id="ucChangePassBtn">🔑 改密码</button></div>';
    h += '<div class="uc-inline" id="ucPassBox">';
    h += '<input type="password" id="ucOldPass" placeholder="旧密码">';
    h += '<input type="password" id="ucNewPass" placeholder="新密码（至少 6 位）">';
    h += '<input type="password" id="ucNewPass2" placeholder="确认新密码">';
    h += '<div class="uc-btns"><button class="uc-btn primary" id="ucPassOkBtn">确认修改</button>';
    h += '<button class="uc-btn ghost" id="ucPassCancelBtn">取消</button></div>';
    h += '</div>';
    h += '<div class="uc-btns"><button class="uc-btn danger" id="ucLogoutBtn">退出登录</button>';
    h += '<button class="uc-btn danger" id="ucDeleteBtn">删除账号</button></div>';
    h += '<div class="uc-inline" id="ucDelBox">';
    h += '<input type="password" id="ucDelPass" placeholder="输入密码确认删除">';
    h += '<div class="uc-btns"><button class="uc-btn danger" id="ucDelOkBtn">确认删除</button>';
    h += '<button class="uc-btn ghost" id="ucDelCancelBtn">取消</button></div>';
    h += '</div>';
    h += '</div>';
    h += '</div>';
    return h;
  }}

  function bindProfileLoggedIn(u) {{
    var saveBtn = document.getElementById('ucSaveProfileBtn');
    if (saveBtn) saveBtn.addEventListener('click', function() {{
      var nick = document.getElementById('ucNick').value.trim();
      var avatar = document.getElementById('ucAvatar').value.trim();
      var sig = document.getElementById('ucSig').value.trim();
      window.__TAURI_INTERNALS__.invoke('user_update_profile', {{ nickname: nick, avatar: avatar, signature: sig }}).then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '资料已保存'); renderProfile(); }}
        else {{ showToast((r && r.msg) || '保存失败'); }}
      }}).catch(function(e) {{ showToast('保存失败：' + e); }});
    }});
    var prefsBtn = document.getElementById('ucSavePrefsBtn');
    if (prefsBtn) prefsBtn.addEventListener('click', function() {{
      var prefs = {{
        show_lunar: document.getElementById('ucShowLunar').checked,
        show_quote: document.getElementById('ucShowQuote').checked,
        show_weather: document.getElementById('ucShowWeather').checked,
        font_size: parseInt(document.getElementById('ucFontSize').value, 10) || 14,
        weather_city: document.getElementById('ucCity').value.trim() || '运城',
        theme_color: document.getElementById('ucThemeColor').value,
        wallpaper: document.getElementById('ucWallpaper').value.trim()
      }};
      window.__TAURI_INTERNALS__.invoke('user_update_prefs', {{ prefs: prefs }}).then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '偏好已保存'); setTimeout(function() {{ location.reload(); }}, 600); }}
        else {{ showToast((r && r.msg) || '保存失败'); }}
      }}).catch(function(e) {{ showToast('保存失败：' + e); }});
    }});
    var expBtn = document.getElementById('ucExportBtn');
    if (expBtn) expBtn.addEventListener('click', function() {{
      window.__TAURI_INTERNALS__.invoke('user_export').then(function(r) {{
        if (r && r.ok) {{ showToast((r.path ? '已导出：' + r.path : r.msg) || '已导出'); }}
        else {{ showToast((r && r.msg) || '已取消导出'); }}
      }}).catch(function(e) {{ showToast('导出失败：' + e); }});
    }});
    var passBtn = document.getElementById('ucChangePassBtn');
    var passBox = document.getElementById('ucPassBox');
    if (passBtn) passBtn.addEventListener('click', function() {{ passBox.classList.toggle('show'); }});
    var passOk = document.getElementById('ucPassOkBtn');
    if (passOk) passOk.addEventListener('click', function() {{
      var oldp = document.getElementById('ucOldPass').value;
      var np = document.getElementById('ucNewPass').value;
      var np2 = document.getElementById('ucNewPass2').value;
      if (!oldp || !np) {{ showToast('请填写旧密码和新密码'); return; }}
      if (np !== np2) {{ showToast('两次新密码不一致'); return; }}
      window.__TAURI_INTERNALS__.invoke('user_change_password', {{ old_password: oldp, new_password: np }}).then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '密码已修改'); renderProfile(); }}
        else {{ showToast((r && r.msg) || '修改失败'); }}
      }}).catch(function(e) {{ showToast('修改失败：' + e); }});
    }});
    var passCancel = document.getElementById('ucPassCancelBtn');
    if (passCancel) passCancel.addEventListener('click', function() {{ passBox.classList.remove('show'); }});
    var logoutBtn = document.getElementById('ucLogoutBtn');
    if (logoutBtn) logoutBtn.addEventListener('click', function() {{
      window.__TAURI_INTERNALS__.invoke('user_logout').then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '已退出登录'); setTimeout(function() {{ location.reload(); }}, 500); }}
        else {{ showToast((r && r.msg) || '退出失败'); }}
      }}).catch(function(e) {{ showToast('退出失败：' + e); }});
    }});
    var delBtn = document.getElementById('ucDeleteBtn');
    var delBox = document.getElementById('ucDelBox');
    if (delBtn) delBtn.addEventListener('click', function() {{ delBox.classList.toggle('show'); }});
    var delOk = document.getElementById('ucDelOkBtn');
    if (delOk) delOk.addEventListener('click', function() {{
      var p = document.getElementById('ucDelPass').value;
      if (!p) {{ showToast('请输入密码确认删除'); return; }}
      window.__TAURI_INTERNALS__.invoke('user_delete', {{ password: p }}).then(function(r) {{
        if (r && r.ok) {{ showToast(r.msg || '账号已删除'); setTimeout(function() {{ location.reload(); }}, 500); }}
        else {{ showToast((r && r.msg) || '删除失败'); }}
      }}).catch(function(e) {{ showToast('删除失败：' + e); }});
    }});
    var delCancel = document.getElementById('ucDelCancelBtn');
    if (delCancel) delCancel.addEventListener('click', function() {{ delBox.classList.remove('show'); }});
  }}

  function switchPanel(name) {{
    var btn = document.querySelector('.icon-btn[data-panel="' + name + '"]');
    if (current === name) {{ closePanel(); return; }}
    panel.classList.add('open');
    document.querySelectorAll('.icon-btn[data-panel]').forEach(function(b) {{ b.classList.remove('active'); }});
    if (btn) btn.classList.add('active');
    current = name;
    if (name === 'todo') {{
      // 待办是异步 IPC 渲染，需先给"加载中"占位再填充
      panelTitle.textContent = '📝 待办清单';
      panelBody.innerHTML = '<div class="empty">⏳ 加载中…</div>';
      renderTodos().then(function(h) {{
        if (current === name) {{ panelBody.innerHTML = h; bindTodos(); }}
      }}).catch(function() {{ panelBody.innerHTML = '<div class="empty">加载失败，请重试</div>'; }});
      return;
    }}
    var cfg;
    if (name === 'profile') cfg = {{ title: '👤 我的', html: renderProfile() }};
    else cfg = PANEL_DATA[name];
    if (!cfg) {{ closePanel(); return; }}
    panelTitle.textContent = cfg.title;
    panelBody.innerHTML = cfg.html;
    if (cfg.bind) cfg.bind();
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

// ---- 主区快捷访问追加用户收藏（世界导航页里 ⭐ 过的）----
  (function() {{
    var favs = loadUserFavs();
    if (!favs.length) return;
    var box = document.getElementById('quickSites');
    if (!box) return;
    box.insertAdjacentHTML('afterbegin', favs.map(function(f) {{
      return '<div class="top-site" data-url="' + esc(f.url) + '"><a class="top-site-link" href="' + esc(f.url) + '"><span class="icon">' + esc(f.icon || '⭐') + '</span><span class="name">' + esc(f.name) + '</span></a><button class="qs-del">×</button></div>';
    }}).join(''));
  }})();

  // ---- 搜索：回车 + 本地历史联想（原版方式）----
  var RE_HTTP = new RegExp('^https?://', 'i');
  var RE_DOMAIN = new RegExp('^[a-z0-9-]+\\.[a-z]{{2,}}', 'i');
  var searchInput = document.getElementById('searchInput');
  var suggestBox = document.getElementById('suggestBox');
  var HISTORY_DATA = {history_json};
  var SEARCH_BASE = {search_url_json};
  var searchEngineName = SEARCH_BASE.indexOf('baidu.com') >= 0 ? '百度' : SEARCH_BASE.indexOf('google.com') >= 0 ? 'Google' : 'Bing';
  var seTag = document.getElementById('searchEngineTag');
  if (seTag) seTag.textContent = searchEngineName;
  searchInput.addEventListener('keydown', function(e) {{
    if (e.key === 'Enter') {{
      var q = e.target.value.trim();
      if (!q) return;
      var url;
      if (RE_HTTP.test(q)) url = q;
      else if (RE_DOMAIN.test(q)) url = 'https://' + q;
      else url = SEARCH_BASE + encodeURIComponent(q);
      window.open(url, '_blank');
    }}
  }});
  searchInput.addEventListener('input', function() {{
    var q = searchInput.value.trim().toLowerCase();
    if (!q) {{ suggestBox.style.display = 'none'; return; }}
    var matches = HISTORY_DATA.filter(function(h) {{ return (h.title || '').toLowerCase().indexOf(q) >= 0 || h.url.toLowerCase().indexOf(q) >= 0; }}).slice(0, 8);
    if (matches.length === 0) {{ suggestBox.style.display = 'none'; return; }}
    suggestBox.innerHTML = matches.map(function(h) {{
      return '<div class="suggest-item" data-url="' + esc(h.url) + '"><span class="s-icon">🔗</span><span>' + esc(h.title || h.url) + '</span><span class="s-url">' + esc(h.url) + '</span></div>';
    }}).join('');
    suggestBox.style.display = 'block';
  }});
  suggestBox.addEventListener('click', function(e) {{
    var item = e.target.closest('.suggest-item');
    if (item) window.open(item.dataset.url, '_blank');
  }});
  document.addEventListener('click', function(e) {{
    if (!e.target.closest('.search-box')) suggestBox.style.display = 'none';
  }});

  // ---- 主区快捷访问：编辑模式 + 添加 ----
  var quickSitesBox = document.getElementById('quickSites');
  var quickEditBtn = document.getElementById('quickEditBtn');
  var editing = false;
  quickEditBtn.addEventListener('click', function() {{
    if (!editing) {{
      editing = true;
      quickSitesBox.classList.add('editing');
      quickEditBtn.textContent = '完成';
      return;
    }}
    var sites = Array.prototype.slice.call(quickSitesBox.querySelectorAll('.top-site')).map(function(card) {{
      return {{ name: card.querySelector('.name').textContent, url: card.dataset.url, icon: card.querySelector('.icon').textContent }};
    }});
    if (sites.length === 0) {{ showToast('快捷访问不能为空'); return; }}
    // 保存到数据库（原版 quick-sites-set），并重新生成 HTML
    try {{
      if (window.__TAURI_INTERNALS__) {{
        window.__TAURI_INTERNALS__.invoke('save_quick_sites', {{ sites: sites }}).then(function() {{
          showToast('快捷访问已保存');
        }}).catch(function(e) {{ showToast('保存失败：' + String(e).replace(/</g,'&lt;')); }});
      }} else {{
        localStorage.setItem('sb_quick_sites', JSON.stringify(sites));
        showToast('快捷访问已保存（本地）');
      }}
    }} catch(e) {{ showToast('保存失败'); }}
    editing = false;
    quickSitesBox.classList.remove('editing');
    quickEditBtn.textContent = '编辑';
  }});
  quickSitesBox.addEventListener('click', function(e) {{
    var del = e.target.closest('.qs-del');
    if (!del || !editing) return;
    var card = del.closest('.top-site');
    if (card) card.remove();
    showToast('已移除，点「完成」保存');
  }});
  var qsForm = document.getElementById('qsForm');
  document.getElementById('qsAddBtn').addEventListener('click', function() {{
    qsForm.classList.add('show');
    document.getElementById('qsName').focus();
  }});
  document.getElementById('qsCancel').addEventListener('click', function() {{
    qsForm.classList.remove('show');
  }});
  document.getElementById('qsSave').addEventListener('click', function() {{
    var name = document.getElementById('qsName').value.trim();
    var url = document.getElementById('qsUrl').value.trim();
    var icon = document.getElementById('qsIcon').value.trim() || '🌐';
    if (!name || !url) {{ showToast('名称和网址都要填'); return; }}
    var finalUrl = RE_HTTP.test(url) ? url : 'https://' + url;
    var card = document.createElement('div');
    card.className = 'top-site';
    card.dataset.url = finalUrl;
    card.innerHTML = '<a class="top-site-link" href="' + esc(finalUrl) + '"><span class="icon">' + esc(icon) + '</span><span class="name">' + esc(name) + '</span></a><button class="qs-del">×</button>';
    quickSitesBox.insertBefore(card, document.getElementById('qsAddBtn'));
    qsForm.classList.remove('show');
    document.getElementById('qsName').value = '';
    document.getElementById('qsUrl').value = '';
    document.getElementById('qsIcon').value = '';
    showToast('已添加，点「完成」保存');
  }});

  // ---- 主区资讯（feed-list 网格）----
  (function() {{
    var list = document.getElementById('feedList');
    if (!list) return;
    var items = FEED_DATA.slice(0, 12);
    if (!items.length) {{
      list.innerHTML = '<div class="empty">资讯还没抓到，稍等 30 秒刷新本页</div>';
      return;
    }}
    list.innerHTML = items.map(function(f) {{
      return '<a class="feed-item" href="' + esc(f[1]) + '"><span class="feed-title">' + esc(f[0]) + '</span><span class="feed-meta">' + esc(f[2]) + (f[3] ? ' · ' + timeAgo(f[3]) : '') + '</span></a>';
    }}).join('');
    document.getElementById('feedMoreBtn').addEventListener('click', function() {{
      switchPanel('feed');
    }});
  }})();

  // ---- 主区最近访问（recent-list 网格）----
  (function() {{
    var list = document.getElementById('recentList');
    if (!list) return;
    var items = HISTORY_DATA.slice(0, 10);
    if (!items.length) {{
      list.innerHTML = '<div class="empty">暂无最近访问</div>';
      return;
    }}
    list.innerHTML = items.map(function(h) {{
      return '<a class="recent-item" href="' + esc(h.url) + '"><span class="recent-title">' + esc(h.title || h.url) + '</span><span class="recent-url">' + esc(h.url) + '</span></a>';
    }}).join('');
  }})();

  // ---- 主区资讯分栏阅读：左列表 + 右内容，点一条看一条（2026-09-19）----
  // 需求：点资讯不跳标签，启动页内"左列表右内容"浏览，随时点下一条
  var feedSplitActive = false;
  var feedSplitBackHtml = '';
  function openFeedSplit(f) {{
    var sec = document.getElementById('feedSection');
    if (!sec) return;
    // 从侧边面板点资讯时主区可能不在资讯区块，滚过去让用户看到分栏
    try {{ sec.scrollIntoView({{ behavior: 'smooth', block: 'start' }}); }} catch(e) {{ sec.scrollIntoView(); }}
    if (!feedSplitActive) {{
      var fl = document.getElementById('feedList');
      feedSplitBackHtml = fl ? fl.innerHTML : '';
      feedSplitActive = true;
    }}
    var items = FEED_DATA.slice(0, 12);
    var listHtml = items.map(function(x) {{
      var active = x[1] === f[1] ? ' active' : '';
      return '<div class="fs-split-item' + active + '" data-i="' + items.indexOf(x) + '"><span class="fs-split-title">' + esc(x[0]) + '</span><span class="fs-split-meta">' + esc(x[2]) + (x[3] ? ' · ' + timeAgo(x[3]) : '') + '</span></div>';
    }}).join('');
    sec.innerHTML = '<div class="feed-split-head">' +
      '<button id="feedSplitBack" class="btn-mini">← 返回列表</button>' +
      '<span class="feed-split-title-text">📰 资讯速读</span>' +
      '</div>' +
      '<div class="feed-split">' +
      '<div class="feed-split-list">' + listHtml + '</div>' +
      '<div class="feed-split-read" id="feedSplitRead"><div class="inline-read-loading">📖 正在提取正文…</div></div>' +
      '</div>';
    document.getElementById('feedSplitBack').addEventListener('click', function() {{
      sec.innerHTML = '<div class="section-head"><h2>📰 资讯</h2><div class="section-actions"><button class="btn-mini" id="feedMoreBtn2">更多</button></div></div>' +
        '<div class="feed-list" id="feedList">' + feedSplitBackHtml + '</div>';
      feedSplitActive = false;
      document.getElementById('feedMoreBtn2').addEventListener('click', function() {{ switchPanel('feed'); }});
      bindOpenInTab(document.getElementById('feedList'));
    }});
    sec.querySelectorAll('.fs-split-item').forEach(function(el) {{
      el.addEventListener('click', function() {{
        var idx = +el.dataset.i;
        var item = items[idx];
        if (!item) return;
        sec.querySelectorAll('.fs-split-item').forEach(function(x) {{ x.classList.remove('active'); }});
        el.classList.add('active');
        loadFeedSplitRead(item);
      }});
    }});
    loadFeedSplitRead(f);
  }}
  function loadFeedSplitRead(f) {{
    var readEl = document.getElementById('feedSplitRead');
    if (!readEl) return;
    readEl.innerHTML = '<div class="inline-read-loading">📖 正在提取正文…</div>';
    window.__TAURI_INTERNALS__.invoke('fetch_article', {{ url: f[1] }})
      .then(function(a) {{
        readEl.innerHTML = renderSplitInline(a, f);
        var c = document.getElementById('content');
        if (c) {{ c.style.setProperty('--fs', inlineFs + 'px'); imgFix(c); }}
      }})
      .catch(function(e) {{
        readEl.innerHTML = '<div class="inline-read-loading">⚠️ ' + String(e).replace(/</g,'&lt;') + '<br><a class="ir-src" href="' + esc(f[1]) + '" target="_blank">去原网页看 ↗</a></div>';
      }});
  }}
  function renderSplitInline(a, f) {{
    var h = '<div class="ir-bar"><a href="' + esc(f[1]) + '" target="_blank" class="ir-src">看原文 ↗</a></div>';
    h += '<div class="ir-art"><h1>' + a.title.replace(/</g,'&lt;') + '</h1><div id="content">' + a.content.replace(/data-src=/g,'src=').replace(/data-original=/g,'src=').replace(/data-lazy-src=/g,'src=') + '</div></div>';
    return h;
  }}

  // ---- 主区外链点击：统一走 window.open（触发 on_new_window → 当前窗口新标签页）----
  // 原因：启动页是 shift:// 自定义协议，WebKit 对协议内页面的跨 scheme 链接
  // （http/https）默认弹独立新窗口；on_new_window 回调已把 window.open 统一
  // 转成当前窗口新标签，这里拦截普通链接点击，避免弹独立窗口。
  // 例外：资讯（feedList）点击进分栏阅读，不跳标签。
  function bindOpenInTab(container) {{
    if (!container) return;
    container.addEventListener('click', function(e) {{
      var a = e.target.closest('a[href]');
      if (!a) return;
      var href = a.getAttribute('href');
      if (!href || href.indexOf('http') !== 0) return; // 只拦截 http/https 外链
      e.preventDefault();
      if (container.id === 'feedList') {{
        var all = container.querySelectorAll('.feed-item');
        var idx = Array.prototype.indexOf.call(all, a);
        var items = FEED_DATA.slice(0, 12);
        if (idx >= 0 && items[idx]) {{ openFeedSplit(items[idx]); return; }}
      }}
      window.open(href, '_blank');
    }});
  }}
  bindOpenInTab(document.getElementById('feedList'));
  bindOpenInTab(document.getElementById('recentList'));
  bindOpenInTab(document.getElementById('quickSites'));

  // ---- 右侧栏：AI / 设置（经 IPC 通知主界面打开对应组件）----
  document.getElementById('aiBtn').addEventListener('click', function() {{
    if (window.__TAURI_INTERNALS__) {{
      window.__TAURI_INTERNALS__.invoke('open_ai_from_newtab').catch(function(e) {{ showToast('打开失败：' + e); }});
    }} else {{ showToast('🤖 AI 助手'); }}
  }});
  document.getElementById('settingsBtn').addEventListener('click', function() {{
    if (window.__TAURI_INTERNALS__) {{
      window.__TAURI_INTERNALS__.invoke('open_settings_from_newtab').catch(function(e) {{ showToast('打开失败：' + e); }});
    }} else {{ showToast('⚙️ 设置'); }}
  }});

  }})();
</script>
</body>
</html>"#,
        quick_grid = quick_grid,
        font_scale = font_size as f64 / 14.0,
        search_url_json = json_script_safe(&serde_json::json!(search_url)),
        history_json = json_script_safe(&serde_json::json!(recent
            .iter()
            .map(|(u, t)| serde_json::json!({ "title": t, "url": u }))
            .collect::<Vec<_>>())),
        feed_data_json = json_script_safe(&serde_json::json!(feed_data)),
        bookmark_html_json = json_script_safe(&serde_json::json!(bookmark_html)),
        recent_html_json = json_script_safe(&serde_json::json!(recent_html)),
        quick_builtin_json = json_script_safe(&serde_json::json!(quick_sites
            .iter()
            .map(|(i, n, u)| serde_json::json!({ "i": i, "n": n, "u": u }))
            .collect::<Vec<_>>())),
        ipc_bridge = IPC_BRIDGE,
        sb_version = env!("CARGO_PKG_VERSION"),
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
  :root { --bg2:rgba(255,255,255,0.05); --bg3:rgba(255,255,255,0.1); --fg:#e0e0e0; --muted:rgba(255,255,255,0.45); --accent:#4a9eff; --border:rgba(255,255,255,0.1); }
  * { box-sizing:border-box; margin:0; padding:0; }
  html,body { height:100%; }
  body { background:linear-gradient(135deg,#1a1a2e 0%,#16213e 50%,#0f3460 100%); color:var(--fg); font:14px/1.5 -apple-system,"PingFang SC","Microsoft YaHei",sans-serif; display:flex; flex-direction:column; border-radius:12px; overflow-x:hidden; }
  .nav-header { display:flex; align-items:center; gap:16px; padding:12px 20px; background:rgba(255,255,255,0.05); border-bottom:1px solid var(--border); flex-shrink:0; }
  .nav-title { font-size:17px; font-weight:600; white-space:nowrap; }
  .nav-title a { color:var(--muted); text-decoration:none; font-size:13px; margin-left:10px; }
  .nav-search { flex:1; max-width:420px; }
  .nav-search input { width:100%; background:rgba(255,255,255,0.1); border:1px solid rgba(255,255,255,0.2); border-radius:8px; color:var(--fg); padding:8px 12px; font-size:13px; outline:none; }
  .nav-search input:focus { border-color:var(--accent); }
  .layout { flex:1; display:flex; overflow:hidden; }
  .cat-tabs { width:150px; overflow-y:auto; background:rgba(255,255,255,0.05); border-right:1px solid var(--border); flex-shrink:0; padding:8px 6px; }
  .cat-tab { display:flex; align-items:center; gap:8px; width:100%; padding:8px 10px; border:none; background:transparent; color:var(--fg); border-radius:8px; cursor:pointer; font-size:13px; text-align:left; margin-bottom:2px; }
  .cat-tab span { overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .cat-tab:hover { background:rgba(255,255,255,0.1); }
  .cat-tab.active { background:var(--accent); color:#fff; }
  .main { flex:1; overflow-y:auto; padding:18px 22px; }
  .cat-header { margin-bottom:14px; }
  .cat-header .ch-icon { font-size:22px; margin-right:8px; }
  .cat-header .ch-title { font-size:18px; font-weight:600; }
  .cat-header .ch-desc { color:var(--muted); font-size:12px; margin-top:2px; }
  .cards { display:grid; grid-template-columns:repeat(auto-fill,minmax(220px,1fr)); gap:12px; }
  .site-card { position:relative; background:rgba(255,255,255,0.05); border:1px solid rgba(255,255,255,0.1); border-radius:12px; padding:14px; cursor:pointer; transition:border-color .12s,background .15s; }
  .site-card:hover { border-color:var(--accent); background:rgba(255,255,255,0.08); }
  .sc-icon { font-size:26px; margin-bottom:8px; }
  .sc-name { font-size:14px; font-weight:600; margin-bottom:3px; }
  .sc-desc { font-size:12px; color:var(--muted); overflow:hidden; text-overflow:ellipsis; white-space:nowrap; margin-bottom:8px; }
  .sc-rating { display:flex; align-items:center; gap:6px; margin-bottom:4px; }
  .stars { position:relative; display:inline-block; font-size:12px; line-height:1; }
  .stars-bg { color:rgba(255,255,255,0.2); }
  .stars-fill { position:absolute; left:0; top:0; color:#f5a623; overflow:hidden; white-space:nowrap; }
  .rating-num { font-size:12px; color:var(--muted); }
  .sc-meta { font-size:11px; color:var(--muted); }
  .sc-star { position:absolute; top:10px; right:10px; border:none; background:transparent; color:var(--muted); font-size:16px; cursor:pointer; }
  .sc-star:hover { color:#f5a623; }
  .sc-star.faved { color:#f5a623; }
  .sc-remove { position:absolute; top:10px; right:34px; border:none; background:transparent; color:var(--muted); font-size:13px; cursor:pointer; display:none; }
  .site-card:hover .sc-remove { display:block; }
  .empty { color:var(--muted); text-align:center; padding:60px 0; }
  .add-fav-btn { margin-bottom:14px; background:rgba(255,255,255,0.1); color:var(--accent); border:1px dashed var(--accent); border-radius:8px; padding:8px 16px; cursor:pointer; font-size:13px; }
  .modal-mask { display:none; position:fixed; inset:0; background:rgba(0,0,0,.55); z-index:50; align-items:center; justify-content:center; }
  .modal-mask.show { display:flex; }
  .modal-box { width:360px; background:#1e2a45; border:1px solid rgba(255,255,255,0.15); border-radius:12px; padding:18px; }
  .modal-box h3 { font-size:15px; margin-bottom:12px; }
  .modal-box input { width:100%; background:rgba(0,0,0,0.3); border:1px solid rgba(255,255,255,0.2); border-radius:8px; color:var(--fg); padding:8px 10px; font-size:13px; margin-bottom:10px; outline:none; }
  .modal-box input:focus { border-color:var(--accent); }
  .modal-actions { display:flex; justify-content:flex-end; gap:8px; }
  .modal-btn { border:none; border-radius:8px; padding:7px 16px; cursor:pointer; font-size:13px; }
  .modal-btn.cancel { background:rgba(255,255,255,0.1); color:var(--fg); }
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

  function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;').replace(/'/g,'&#39;').replace(/\\/g,'&#92;'); }

  function renderTabs() {
    document.getElementById('catTabs').innerHTML = CATS.map(function(c) {
      return '<button class="cat-tab' + (c.id === current ? ' active' : '') + '" data-cat="' + c.id + '">' + esc(c.icon) + '<span>' + esc(c.title) + '</span></button>';
    }).join('');
  }

  function cardHtml(s, isUser) {
    var pct = s.r ? Math.round((parseFloat(s.r) / 5) * 100) : 0;
    var fav = isUser || s.d === '我的收藏';
    return '<div class="site-card" data-url="' + esc(s.u) + '">' +
      '<div class="sc-icon">' + esc(s.i || '🔗') + '</div>' +
      '<div class="sc-name">' + esc(s.n) + '</div>' +
      '<div class="sc-desc">' + esc(s.d || s.u) + '</div>' +
      (fav ? '' : '<div class="sc-rating"><span class="stars"><span class="stars-bg">★★★★★</span><span class="stars-fill" style="width:' + pct + '%">★★★★★</span></span><span class="rating-num">' + esc(s.r) + '</span></div>') +
      '<div class="sc-meta">' + (fav ? '⭐ 我的收藏' : '👥 ' + esc(s.us) + ' · ⚡ ' + esc(s.sp)) + '</div>' +
      (fav ? '<button class="sc-remove" data-url="' + esc(s.u) + '" title="移除收藏">✕</button>' : '') +
      '<button class="sc-star' + (isFaved(s.u) ? ' faved' : '') + '" data-url="' + esc(s.u) + '" data-name="' + esc(s.n) + '" data-icon="' + esc(s.i || '🔗') + '" title="收藏到快捷访问">' + (isFaved(s.u) ? '★' : '☆') + '</button>' +
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

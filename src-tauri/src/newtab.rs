// 新标签页门面（原版左侧栏面板生态迁移）
// 方案：Rust 读数据库渲染成完整 HTML，写到 app_data/newtab/index.html，
// 标签用 file:// 打开。面板静态数据（最近访问/书签/快捷访问）渲染时嵌入，
// 交互状态（待办勾选）用 localStorage 存本地，无需 IPC。
//
// R1 拆分（2026-09-25）：
//   - wallpaper：Bing 壁纸拉取/缓存（后台任务，不占生成关键路径）
//   - render：启动页/世界导航页模板渲染与原子写
//   - quick_sites：特权页 URL scheme 安全校验
use crate::db::Db;
use crate::error::{AppError, CommandResult};
use tauri::{AppHandle, Emitter, Manager, State};

#[path = "newtab/quick_sites.rs"]
pub(crate) mod quick_sites;
#[path = "newtab/render.rs"]
mod render;
#[path = "newtab/wallpaper.rs"]
mod wallpaper;

// setup（app/bootstrap.rs）在锁屏前 spawn 壁纸后台刷新
pub(crate) use wallpaper::refresh_wallpapers_background;

/// 生成互斥锁：序列化并发生成（setup 异步生成 / newtab_url 兜底 / refresh_newtab 可能同时触发）
static GEN_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

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
        crate::log_if_err!("newtab", generate(&db, &path).await);
    }
    Some(format!("file://{}", path.display()))
}

pub async fn generate(db: &Db, path: &std::path::Path) -> Result<String, AppError> {
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
    generate_inner(&db.pool, path).await
}

/// 生成核心（与 Tauri 解耦，直接吃连接池，便于集成测试）：
/// 数据采集（history/bookmarks/settings/feeds/wallpaper 缓存）+ 渲染 + 原子写。
/// 防抖与并发锁留在 `generate` 壳（计时与锁依赖真实文件与运行时，不在 inner 重复）。
async fn generate_inner(
    pool: &sqlx::SqlitePool,
    path: &std::path::Path,
) -> Result<String, AppError> {
    // ---- 数据：最近访问 ----
    let recent: Vec<(String, String)> = sqlx::query_as(
        "SELECT url, COALESCE(title, '') FROM history ORDER BY last_visit_at DESC LIMIT 30",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::db(e.to_string()))?;

    // ---- 数据：书签（扁平列表）----
    let bookmarks: Vec<(String, String)> =
        sqlx::query_as("SELECT title, url FROM bookmarks ORDER BY sort_order, id")
            .fetch_all(pool)
            .await
            .map_err(|e| AppError::db(e.to_string()))?;

    // ---- 数据：设置（搜索引擎 / 快捷访问）----
    async fn setting(pool: &sqlx::SqlitePool, key: &str) -> Option<String> {
        let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
        row.map(|r| r.0)
    }
    let search_engine = setting(pool, "search_engine")
        .await
        .unwrap_or_else(|| crate::constants::search::DEFAULT_ENGINE.into());
    // 搜索引擎 URL 模板来自 constants.rs（唯一来源，未知 id 自动回退默认引擎）
    let search_url = crate::constants::search::url_template(&search_engine);
    let quick_sites: Vec<(String, String, String)> = match setting(pool, "quick_sites").await {
        Some(j) => serde_json::from_str::<Vec<serde_json::Value>>(&j)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| {
                        let icon = v.get("icon")?.as_str()?.to_string();
                        let name = v.get("name")?.as_str()?.to_string();
                        let url = v.get("url")?.as_str()?.to_string();
                        // P2：过滤危险 scheme（历史遗留/被篡改的 quick_sites 数据），
                        // 特权页面不允许渲染 javascript: 等
                        if !quick_sites::is_safe_web_url(&url) {
                            return None;
                        }
                        Some((icon, name, url))
                    })
                    .collect()
            })
            .unwrap_or_else(|_| render::default_quick_sites()),
        None => render::default_quick_sites(),
    };

    // ---- 数据：资讯（内置源缓存最新 80 条）----
    let feed_items = crate::feed::latest_items_db(pool, 80).await;

    // ---- 数据：动态背景开关（原版 showBg，默认开）----
    let show_bg = setting(pool, "show_bg")
        .await
        .map(|v| v != "0" && v != "false")
        .unwrap_or(true);
    let wallpapers = if show_bg {
        // 关键路径只读缓存（不拉网络）——网络由 refresh_wallpapers_background 后台做
        let cache = path
            .parent()
            .map(|p| p.join("wallpapers.json"))
            .unwrap_or_else(|| std::path::PathBuf::from("wallpapers.json"));
        wallpaper::load_wallpapers_from_cache(&cache)
    } else {
        Vec::new()
    };

    // ---- 数据：字体大小（原版 fontSize，默认 14）----
    let font_size: u32 = setting(pool, "font_size")
        .await
        .and_then(|v| v.parse().ok())
        .unwrap_or(14);

    // ---- 渲染 HTML（启动页 + 世界导航页）----
    let html = render::render_html(
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
        std::fs::create_dir_all(parent).map_err(|e| AppError::internal(e.to_string()))?;
        render::atomic_write(
            &parent.join("world-nav.html"),
            &render::render_worldnav_html(),
        )
        .map_err(|e| AppError::internal(e.to_string()))?;
    }
    render::atomic_write(path, &html).map_err(|e| AppError::internal(e.to_string()))?;
    Ok(path.display().to_string())
}

/// 生成/刷新新标签页（改设置或数据变化后可调，前端也可定时刷）
#[tauri::command]
pub async fn refresh_newtab(db: State<'_, Db>, app: AppHandle) -> CommandResult<String> {
    let path = newtab_path(&app).ok_or_else(|| AppError::internal("无法获取数据目录"))?;
    generate(&db, &path)
        .await
        .map_err(crate::error::CommandError::from)
}

/// 取新标签页 URL（不存在则先生成）
#[tauri::command]
pub async fn get_newtab_url(app: AppHandle) -> CommandResult<String> {
    newtab_url(&app)
        .await
        .ok_or_else(|| AppError::internal("无法生成新标签页").into())
}

/// 启动页右侧栏按钮：通知主界面打开导入模块
#[tauri::command]
pub fn open_import_from_newtab(app: AppHandle) {
    let _ = app.emit(crate::contracts::events::OPEN_IMPORT_FROM_NEWTAB, ());
}

/// 启动页右侧栏按钮：通知主界面打开 AI 助手侧栏
#[tauri::command]
pub fn open_ai_from_newtab(app: AppHandle) {
    let _ = app.emit(crate::contracts::events::OPEN_AI_FROM_NEWTAB, ());
}

/// 启动页右侧栏按钮：通知主界面打开设置弹窗
#[tauri::command]
pub fn open_settings_from_newtab(app: AppHandle) {
    let _ = app.emit(crate::contracts::events::OPEN_SETTINGS_FROM_NEWTAB, ());
}

/// 启动页资讯点击：通知主界面在**当前标签右侧**新开标签打开该 URL（列表页保留）。
/// 走事件而非直接建标签，是为了复用前端 createTab 的完整同步逻辑（顺序/激活/归组）。
#[tauri::command]
pub fn open_url_from_newtab(app: AppHandle, url: String) {
    log::info!(
        "[newtab] 资讯点击 → 右侧新标签: {}",
        crate::redact_url(&url)
    );
    let _ = app.emit(crate::contracts::events::OPEN_URL_FROM_NEWTAB, url);
}

/// 待办清单存储键（settings 表）
const TODO_KEY: &str = "todos";

/// 读待办清单（JSON 数组字符串，默认 "[]"）。
/// 启动页是 shift:// 自定义协议，localStorage 在 WebKitGTK 下不可靠，
/// 与快捷访问一样改走 settings 表（IPC）持久化。
#[tauri::command]
pub async fn todo_list(db: State<'_, Db>) -> CommandResult<String> {
    Ok(todo_list_inner(&db.pool).await)
}

/// 读待办核心（吃连接池，便于测试）：缺键返回 "[]"。
async fn todo_list_inner(pool: &sqlx::SqlitePool) -> String {
    let row: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(TODO_KEY)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();
    row.unwrap_or_else(|| "[]".to_string())
}

#[tauri::command]
pub async fn todo_save(db: State<'_, Db>, todos: String) -> CommandResult<()> {
    todo_save_inner(&db.pool, &todos).await?;
    Ok(())
}

/// 保存待办核心（吃连接池，便于测试）：体积上限 + JSON 数组校验 + upsert。
fn validate_todo(todos: &str) -> Result<(), AppError> {
    if todos.len() > 100 * 1024 {
        return Err(AppError::validation("待办数据过大"));
    }
    // 必须是 JSON 数组
    let parsed: serde_json::Value = serde_json::from_str(todos)
        .map_err(|e| AppError::validation(format!("待办格式错误: {e}")))?;
    if !parsed.is_array() {
        return Err(AppError::validation("待办格式错误: 需为数组"));
    }
    Ok(())
}

async fn todo_save_inner(pool: &sqlx::SqlitePool, todos: &str) -> Result<(), AppError> {
    validate_todo(todos)?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(TODO_KEY)
    .bind(todos)
    .execute(pool)
    .await
    .map_err(|e| AppError::db(e.to_string()))?;
    Ok(())
}

#[tauri::command]
pub async fn save_quick_sites(
    db: State<'_, Db>,
    app: AppHandle,
    sites: Vec<serde_json::Value>,
) -> CommandResult<()> {
    save_quick_sites_inner(&db.pool, sites).await?;
    // 重新生成 HTML，让主区网格立即生效
    let path = newtab_path(&app).ok_or_else(|| AppError::internal("无法获取数据目录"))?;
    generate(&db, &path)
        .await
        .map_err(crate::error::CommandError::from)?;
    Ok(())
}

/// 保存快捷访问核心（吃连接池，便于测试）：URL scheme 校验/规范化 + upsert。
/// P2：新标签页是带 IPC 桥（invoke_key）的特权页面，若允许 javascript: 等
/// scheme 渲染成 <a href>，点击即在特权上下文执行任意命令。
pub(crate) async fn save_quick_sites_inner(
    pool: &sqlx::SqlitePool,
    sites: Vec<serde_json::Value>,
) -> Result<(), AppError> {
    let mut cleaned: Vec<serde_json::Value> = Vec::with_capacity(sites.len());
    for site in &sites {
        let mut s = site.clone();
        let url = s
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| AppError::validation("快捷站点缺少 url"))?;
        let norm = quick_sites::normalize_quick_site_url(url).map_err(AppError::validation)?;
        s["url"] = serde_json::Value::String(norm);
        cleaned.push(s);
    }
    let json = serde_json::to_string(&cleaned).map_err(|e| AppError::internal(e.to_string()))?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('quick_sites', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(pool)
    .await
    .map_err(|e| AppError::db(e.to_string()))?;
    log::info!("[newtab] 快捷访问已保存 {} 条", cleaned.len());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 建一个跑完整迁移的真实库（settings/history/bookmarks/feed_items 表齐全）
    async fn temp_db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::init(dir.path().to_path_buf()).await.unwrap();
        (dir, db)
    }

    /// 直写一行 settings（测试造数用；绕开命令层）
    async fn seed_setting(pool: &sqlx::SqlitePool, key: &str, value: &str) {
        sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn generate_writes_both_pages_with_seeded_data() {
        let (dir, db) = temp_db().await;
        seed_setting(&db.pool, "search_engine", "bing").await;
        seed_setting(&db.pool, "font_size", "18").await;
        seed_setting(
            &db.pool,
            "quick_sites",
            r#"[{"icon":"🌐","name":"例站","url":"https://example.com"},
                {"icon":"💥","name":"危险","url":"javascript:alert(1)"}]"#,
        )
        .await;
        // 历史两条 + 书签一条（generate 应渲染进 HTML）
        sqlx::query("INSERT INTO history (url, title, visit_count, last_visit_at) VALUES ('https://a.com', '站点A', 1, datetime('now'))")
            .execute(&db.pool).await.unwrap();
        sqlx::query("INSERT INTO history (url, title, visit_count, last_visit_at) VALUES ('https://b.com', '站点B', 2, datetime('now'))")
            .execute(&db.pool).await.unwrap();
        sqlx::query("INSERT INTO bookmarks (title, url, folder_id, sort_order, created_at) VALUES ('书签甲', 'https://bm.com', NULL, 0, datetime('now'))")
            .execute(&db.pool).await.unwrap();

        let path = dir.path().join("newtab").join("index.html");
        generate_inner(&db.pool, &path).await.unwrap();

        // 两页均落盘且非空
        assert!(path.is_file(), "index.html 应生成");
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(!html.is_empty());
        let worldnav = dir.path().join("newtab").join("world-nav.html");
        assert!(worldnav.is_file(), "world-nav.html 应生成");
        // 种子数据渲染进启动页
        assert!(html.contains("https://a.com"), "最近访问应嵌入");
        assert!(html.contains("https://bm.com"), "书签应嵌入");
        assert!(html.contains("https://example.com"), "快捷访问应嵌入");
        // P2 安全：危险 scheme 数据不得进入 HTML
        assert!(
            !html.contains("javascript:alert"),
            "javascript: 必须被过滤"
        );
        let _ = db;
    }

    #[tokio::test]
    async fn generate_defaults_when_settings_missing() {
        let (dir, db) = temp_db().await;
        let path = dir.path().join("newtab").join("index.html");
        generate_inner(&db.pool, &path).await.unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        // 默认搜索引擎来自 constants.rs（唯一来源），未知/缺省回退默认引擎
        assert!(
            html.contains(crate::constants::search::url_template(
                crate::constants::search::DEFAULT_ENGINE
            )),
            "缺省应回退默认搜索引擎模板"
        );
        // 默认快捷访问来自 render::default_quick_sites
        assert!(!html.is_empty());
    }

    #[tokio::test]
    async fn todo_roundtrip_and_defaults() {
        let (_dir, db) = temp_db().await;
        // 缺键默认 "[]"
        assert_eq!(todo_list_inner(&db.pool).await, "[]");
        // 写入读回
        todo_save_inner(&db.pool, r#"[{"text":"买奶","done":false}]"#)
            .await
            .unwrap();
        assert_eq!(
            todo_list_inner(&db.pool).await,
            r#"[{"text":"买奶","done":false}]"#
        );
        // 覆盖写（upsert 不重复插行）
        todo_save_inner(&db.pool, "[]").await.unwrap();
        assert_eq!(todo_list_inner(&db.pool).await, "[]");
    }

    #[tokio::test]
    async fn todo_save_rejects_oversize_and_non_array() {
        let (_dir, db) = temp_db().await;
        // 超 100KiB 拒绝
        let big = format!("[\"{}\"]", "x".repeat(100 * 1024));
        assert!(todo_save_inner(&db.pool, &big).await.is_err(), "超大应拒绝");
        // 非数组 JSON 拒绝
        assert!(todo_save_inner(&db.pool, r#"{"a":1}"#).await.is_err());
        assert!(todo_save_inner(&db.pool, "\"str\"").await.is_err());
        // 非法 JSON 拒绝
        assert!(todo_save_inner(&db.pool, "not json").await.is_err());
        // 拒绝后库里无残值
        assert_eq!(todo_list_inner(&db.pool).await, "[]");
    }

    #[tokio::test]
    async fn save_quick_sites_normalizes_and_persists() {
        let (_dir, db) = temp_db().await;
        let sites = vec![
            serde_json::json!({"icon": "🌐", "name": "例站", "url": "example.com"}),
            serde_json::json!({"icon": "🔒", "name": "安全站", "url": "https://safe.com"}),
        ];
        save_quick_sites_inner(&db.pool, sites).await.unwrap();
        let saved = todo_list_inner_value(&db.pool, "quick_sites").await;
        let arr: Vec<serde_json::Value> = serde_json::from_str(&saved).unwrap();
        assert_eq!(arr.len(), 2);
        // 无 scheme 自动补 https://
        assert_eq!(arr[0]["url"], "https://example.com");
        assert_eq!(arr[1]["url"], "https://safe.com");
    }

    #[tokio::test]
    async fn save_quick_sites_rejects_missing_url_and_dangerous_scheme() {
        let (_dir, db) = temp_db().await;
        // 缺 url 字段
        let bad = vec![serde_json::json!({"icon": "🌐", "name": "无址站"})];
        assert!(save_quick_sites_inner(&db.pool, bad).await.is_err());
        // 危险 scheme
        let bad = vec![serde_json::json!({"icon": "💥", "name": "注入", "url": "javascript:alert(1)"})];
        assert!(save_quick_sites_inner(&db.pool, bad).await.is_err());
        // 拒绝后库里无残值
        let row: Option<String> =
            sqlx::query_scalar("SELECT value FROM settings WHERE key = 'quick_sites'")
                .fetch_optional(&db.pool)
                .await
                .unwrap();
        assert!(row.is_none(), "被拒写入不得落库");
    }

    /// 读任意 settings 键（测试断言辅助）
    async fn todo_list_inner_value(pool: &sqlx::SqlitePool, key: &str) -> String {
        let row: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten();
        row.unwrap_or_default()
    }
}

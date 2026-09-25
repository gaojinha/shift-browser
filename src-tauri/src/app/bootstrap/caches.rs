//! 启动期缓存/配置载入：用户脚本注入缓存、settings 预热、权限白名单、UA、devtools 开关
//! （setup 的子步骤；全部在锁屏之后、首个 webview 创建之前完成）

/// 预热各类缓存与白名单（全部 block_on 一次性载入，热路径不再查库）
pub(super) fn warmup_caches(app: &tauri::App) {
    use tauri::Manager;

    // 用户脚本注入缓存（启动时刷新，确保首个页面注入前就绪——inject 路径只读缓存，不能 await）
    {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::db::userscripts::refresh_cache(&db.pool));
        log::info!("[ShiftBrowser] 用户脚本注入缓存已刷新");
    }

    // 性能 P0：settings 缓存预热（消除热路径 N+1 查库）
    {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::db::settings_cache::warmup(&db.pool));
    }

    // 权限白名单载入内存（安全 #2 · 2026-09-22）
    {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::perms::load_allowlist(&db.pool));
    }
    // User-Agent 配置载入（方案 B · 2026-09-22）
    {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::useragent::load(&db.pool));
    }
    // 开发者工具开关载入（安全 #6 · 2026-09-22，默认关）
    {
        let db = app.state::<crate::db::Db>();
        if let Ok(Some((v,))) = tauri::async_runtime::block_on(async {
            sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'enable_devtools'")
                .fetch_optional(&db.pool)
                .await
        }) {
            crate::engine::webview::DEVTOOLS_ENABLED
                .store(v == "true", std::sync::atomic::Ordering::Relaxed);
        }
    }
}

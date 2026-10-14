//! 启动期缓存/配置载入：用户脚本注入缓存、settings 预热、权限白名单、UA、devtools 开关
//! （setup 的子步骤；锁屏前后台 spawn，不占 GTK 主线程）

/// 预热各类缓存与白名单（一次并发载入，热路径不再查库）
/// 总超时 800 毫秒：任一子任务超时/失败只记日志，不阻断启动（启动优化实施）
/// 纯异步：调用方在后台 spawn 里直接 await，内部绝不用 block_on
/// （block_on 在 tokio worker 里会 panic，见 2026-09-27 启动实测）。
pub(super) async fn warmup_caches(app: &tauri::AppHandle) {
    use tauri::Manager;

    let db = app.state::<crate::db::Db>();
    let pool = db.pool.clone();
    let started = std::time::Instant::now();
    let work = async {
        tokio::join!(
            // 用户脚本注入缓存（启动时刷新，确保首个页面注入前就绪）
            async {
                crate::db::userscripts::refresh_cache(&pool).await;
                log::info!("[ShiftBrowser] 用户脚本注入缓存已刷新");
            },
            // settings 缓存预热（消除热路径 N+1 查库）
            crate::db::settings_cache::warmup(&pool),
            // 权限白名单载入内存
            crate::perms::load_allowlist(&pool),
            // User-Agent 配置载入
            crate::useragent::load(&pool),
            // 开发者工具开关载入（默认关）
            async {
                if let Ok(Some((v,))) = sqlx::query_as::<_, (String,)>(
                    "SELECT value FROM settings WHERE key = 'enable_devtools'",
                )
                .fetch_optional(&pool)
                .await
                {
                    crate::engine::webview::DEVTOOLS_ENABLED
                        .store(v == "true", std::sync::atomic::Ordering::Relaxed);
                }
            },
            // 下载目录覆盖值载入（空串/无记录 = 用系统默认）
            async {
                if let Ok(Some((v,))) = sqlx::query_as::<_, (String,)>(
                    "SELECT value FROM settings WHERE key = 'download_dir'",
                )
                .fetch_optional(&pool)
                .await
                {
                    crate::engine::platform::set_download_dir_override(&v);
                }
            },
        );
    };
    if tokio::time::timeout(std::time::Duration::from_millis(800), work)
        .await
        .is_err()
    {
        log::warn!("[ShiftBrowser] 缓存预热超时（800ms），未完成项后台不再补（热路径回源查库）");
    }
    log::info!(
        "[ShiftBrowser] 缓存预热完成，用时 {}ms",
        started.elapsed().as_millis()
    );
}

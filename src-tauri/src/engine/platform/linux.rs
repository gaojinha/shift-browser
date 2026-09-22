//! Linux/WebKitGTK 平台实现（P0-2）
//!
//! 收纳原散落在 `tabs.rs` / `lib.rs` 的 WebKitGTK/GTK 平台代码：
//! - Cookie 持久化初始化
//! - WebKit 内存压力治理
//! - 内容清理（Cookie/缓存/存储）
//! - 下载目录解析
//!
//! 注意：本模块可自由 `use webkit2gtk/gtk`，业务模块不应。

use webkit2gtk::{CookieManagerExt, CookiePersistentStorage, WebContext, WebContextExt};

/// 初始化 Cookie 持久化（必须在任何 web process 创建前调用）
/// 返回 cookie 文件路径（供日志/加固使用）
pub fn init_cookie_persistence(data_home: &std::path::Path) -> Option<std::path::PathBuf> {
    let _ = gtk::init();
    let context = WebContext::default()?;
    let cm = context.cookie_manager()?;
    let cookie_path = data_home.join("com.shiftbrowser.app").join("cookies.sqlite");
    if let Some(dir) = cookie_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    cm.set_persistent_storage(
        cookie_path.to_str().unwrap_or("cookies.sqlite"),
        CookiePersistentStorage::Sqlite,
    );
    Some(cookie_path)
}

/// WebKit 内存压力治理（必须在任何 WebContext 使用前设置）
/// 与原 lib.rs 实现一致：上限取系统内存 40%，阈值 0.33/0.5/0.9，轮询 1s
pub fn setup_memory_pressure(total_mb: u32) {
    use webkit2gtk::{MemoryPressureSettings, WebsiteDataManager};
    let limit = ((total_mb as f64) * 0.4) as u32;
    let mut mps = MemoryPressureSettings::new();
    mps.set_memory_limit(limit);
    mps.set_conservative_threshold(0.33);
    mps.set_strict_threshold(0.5);
    mps.set_kill_threshold(0.9);
    mps.set_poll_interval(1.0);
    WebsiteDataManager::set_memory_pressure_settings(&mut mps);
    log::info!(
        "[perf] WebKit 内存压力治理已启用：上限 {} MB（系统总内存 {} MB 的 40%）",
        limit,
        total_mb
    );
}

/// 清除浏览数据（Cookie/缓存/存储）——必须在主线程调用
pub fn clear_web_data(cookies: bool, cache: bool, storage: bool) {
    use gtk::glib;
    use webkit2gtk::WebsiteDataManagerExtManual;
    if let Some(ctx) = WebContext::default() {
        if cookies {
            if let Some(cm) = ctx.cookie_manager() {
                cm.delete_all_cookies();
            }
        }
        if cache {
            ctx.clear_cache();
        }
        if storage {
            if let Some(dm) = ctx.website_data_manager() {
                dm.clear(
                    webkit2gtk::WebsiteDataTypes::LOCAL_STORAGE
                        | webkit2gtk::WebsiteDataTypes::SESSION_STORAGE
                        | webkit2gtk::WebsiteDataTypes::INDEXEDDB_DATABASES
                        | webkit2gtk::WebsiteDataTypes::DOM_CACHE,
                    glib::TimeSpan::from_seconds(0),
                    None::<&gtk::gio::Cancellable>,
                    |_| {},
                );
            }
        }
    }
}

/// 系统下载目录
pub fn download_dir() -> Option<std::path::PathBuf> {
    gtk::glib::user_special_dir(gtk::glib::UserDirectory::Downloads)
}

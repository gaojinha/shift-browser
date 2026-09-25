//! 启动期平台防护：敏感文件加固 / WebKit 内存压力治理 / 窗口透明（setup 的子步骤）

/// 安全（#1 · 2026-09-22）：敏感文件（cookie 库/WAL）启动即加固到 0600，
/// 并每 120s 重扫一次——WebKit 运行时会按 umask 重建这些文件。
#[cfg(unix)]
pub(super) fn spawn_sensitive_file_guard(app: &tauri::App) {
    use tauri::Manager;
    if let Ok(dir) = app.path().app_data_dir() {
        crate::harden_sensitive_files(&dir);
        let dir2 = dir.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
                crate::harden_sensitive_files(&dir2);
            }
        });
    }
}

/// 性能：WebKitGTK 内存压力治理。
/// 实测痛点：9 个标签时全部 WebKit 进程合计 3.75 GB（每标签一个渲染进程、
/// 单个 240-500MB，WebKitGTK 默认不主动回收）。
/// 方案：设置内存压力阈值，超过后 WebKit 分级回收（保守→严格→杀进程）。
/// 必须在使用任何 WebContext 之前设置（WebsiteDataManager 的静态全局设置）。
#[cfg(target_os = "linux")]
pub(super) fn setup_memory_pressure() {
    // P0-2：平台代码下沉到 engine::platform::linux
    let total_mb: u32 = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        })
        .map(|kb| (kb / 1024) as u32)
        .unwrap_or(8192);
    crate::engine::platform::linux::setup_memory_pressure(total_mb);
}

/// 2026-09-19：GNOME 49 对无装饰窗口强制四角圆角，但 Tauri/wry 只把 WebView
/// 背景设为透明，GTK 窗口本身没设透明——圆角外露出黑色窗口背景（四个黑角）。
/// 手动设置 RGBA visual + app_paintable，让窗口真正透明，圆角外透出桌面。
#[cfg(target_os = "linux")]
pub(super) fn apply_window_transparency(app: &tauri::App) -> Result<(), String> {
    use gtk::prelude::*;
    use tauri::Manager;
    if let Some(win) = app.get_window("main") {
        if let Ok(gtk_win) = win.gtk_window() {
            if let Some(screen) = gtk::prelude::WidgetExt::screen(&gtk_win) {
                if let Some(visual) = screen.rgba_visual() {
                    gtk_win.set_visual(Some(&visual));
                }
                // Wayland 下 set_visual 无效（X11 概念），GTK 默认还会画主题背景色——
                // 用 CSS 强制窗口背景透明，圆角外才能透出桌面
                let css = gtk::CssProvider::new();
                css.load_from_data(b"window { background-color: transparent; }")
                    .map_err(|e| format!("CSS 加载失败: {}", e))?;
                gtk::StyleContext::add_provider_for_screen(
                    &screen,
                    &css,
                    gtk::STYLE_PROVIDER_PRIORITY_USER,
                );
                log::info!(
                    "[ShiftBrowser] 窗口透明设置：rgba_visual={}",
                    screen.rgba_visual().is_some()
                );
            }
            gtk_win.set_app_paintable(true);
            gtk_win.queue_draw();
            log::info!(
                "[ShiftBrowser] 窗口透明已启用（app_paintable={}）",
                gtk_win.is_app_paintable()
            );
        }
    }
    Ok(())
}

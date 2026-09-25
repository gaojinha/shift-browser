//! 系统托盘装配（setup 的子步骤）：ksni 托盘服务 + 关窗缩托盘 + GTK 主线程动作排空

use tauri::Emitter;
use tauri::Manager;

/// 系统托盘（关闭 = 缩托盘）。
/// 见 tray.rs。ksni 菜单闭包跑在 D-Bus 线程，只往 mpsc 投动作，
/// 由 glib 定时器在 GTK 主线程排空执行（绝不跨线程碰 GTK）。
pub(super) fn setup_tray(app: &tauri::App) {
    let (tray_tx, tray_rx) = std::sync::mpsc::channel::<crate::tray::TrayAction>();

    // 起托盘服务（ksni 的 spawn 是 async，交给 tauri runtime）
    tauri::async_runtime::spawn(async move {
        use ksni::TrayMethods;
        match crate::tray::ShiftTray::new(tray_tx).spawn().await {
            Ok(handle) => {
                log::info!("[tray] 系统托盘已启动");
                // Handle 必须存活到进程结束，drop 掉托盘图标就没了
                std::mem::forget(handle);
            }
            Err(e) => log::warn!("[tray] 托盘启动失败（无宿主扩展？）: {}", e),
        }
    });

    // 关窗 = 缩托盘：拦截主窗口 delete-event
    if let Some(w) = app.get_window("main") {
        if let Ok(gtk_win) = w.gtk_window() {
            use gtk::prelude::*;
            let app_de = app.handle().clone();
            gtk_win.connect_delete_event(move |_, _| {
                use std::sync::atomic::Ordering;
                if crate::tray::REAL_QUIT.load(Ordering::SeqCst) {
                    // 真退出：放行，窗口关闭 → 应用退出
                    gtk::glib::Propagation::Proceed
                } else {
                    // 缩托盘：隐藏窗口，阻止关闭
                    if let Some(win) = app_de.get_window("main") {
                        let _ = win.hide();
                    }
                    log::info!("[tray] 窗口已缩到托盘（未退出）");
                    gtk::glib::Propagation::Stop
                }
            });
        }
    }

    // GTK 主线程排空托盘动作（120ms 一轮，开销可忽略）
    let app_tray = app.handle().clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
        use std::sync::atomic::Ordering;
        while let Ok(action) = tray_rx.try_recv() {
            match action {
                crate::tray::TrayAction::ToggleWindow => {
                    if let Some(w) = app_tray.get_window("main") {
                        if w.is_visible().unwrap_or(false) {
                            let _ = w.hide();
                        } else {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                }
                crate::tray::TrayAction::OpenDownloads => {
                    let _ = app_tray.emit(crate::contracts::events::TRAY_OPEN_DOWNLOADS, ());
                }
                crate::tray::TrayAction::OpenPasswords => {
                    let _ = app_tray.emit(crate::contracts::events::TRAY_OPEN_PASSWORDS, ());
                }
                crate::tray::TrayAction::Quit => {
                    crate::tray::REAL_QUIT.store(true, Ordering::SeqCst);
                    if let Some(w) = app_tray.get_window("main") {
                        let _ = w.close();
                    }
                }
            }
        }
        gtk::glib::ControlFlow::Continue
    });
}

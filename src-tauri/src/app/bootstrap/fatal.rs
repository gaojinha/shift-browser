//! 启动期致命错误弹窗（从 `bootstrap.rs` 拆出）。
//!
//! Vue UI 尚不可用时（如数据库初始化失败），只能走 GTK 原生层弹窗；
//! setup 在 GTK 主线程执行，`run()` 模态阻塞，用户确认后调用方返回 Err 退出。

/// 致命错误提示（Linux：GTK 原生模态窗）
#[cfg(target_os = "linux")]
pub fn show_fatal_dialog(app: &tauri::AppHandle, msg: &str) {
    use gtk::prelude::*;
    use tauri::Manager;
    let parent = app.get_window("main").and_then(|w| w.gtk_window().ok());
    let dialog = gtk::MessageDialog::new(
        parent.as_ref(),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Close,
        msg,
    );
    dialog.set_title("Shift Browser 启动失败");
    dialog.run();
    dialog.close();
}

/// 非 Linux 平台暂无 GUI，致命信息落 stderr
#[cfg(not(target_os = "linux"))]
pub fn show_fatal_dialog(_app: &tauri::AppHandle, msg: &str) {
    eprintln!("[ShiftBrowser] 致命错误: {msg}");
}

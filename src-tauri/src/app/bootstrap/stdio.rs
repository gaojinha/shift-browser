//! stdio 落盘（从 `bootstrap.rs` 拆出）。
//!
//! 必须在 CEF 初始化**之前**调用：CEF 子进程继承 stderr，Chromium 的崩溃线索
//! （CHECK failed / renderer crash / GPU fallback）都从这里出来。

/// 把 stdout/stderr 重定向到 `<app_data>/logs/stdio.log`（追加，超 5MB 轮转一份 .1）。
pub fn redirect_stdio_to_file(app_data_dir: &std::path::Path) {
    use std::os::fd::IntoRawFd;

    let dir = app_data_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("stdio.log");
    // 简易轮转：只留一份上一版，避免无限增长
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > 5 * 1024 * 1024 {
        let _ = std::fs::rename(&path, dir.join("stdio.log.1"));
    }
    let Ok(f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let fd = f.into_raw_fd();
    // SAFETY: dup2 是异步信号安全的系统调用；成功后 fd 已复制，原 fd 可关。
    unsafe {
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
        libc::close(fd);
    }
    log::info!("[ShiftBrowser] stdout/stderr 已落盘: {path:?}");
}

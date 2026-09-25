//! 全局 panic 钩子（P0-4，2026-09-24）
//!
//! 全应用无 `catch_unwind`：WebKit 网页进程崩溃有 UI 兜底，浏览器自身线程
//! panic 却只会让进程静默消失，用户无法提供任何证据。钩子统一记录
//! 线程/位置/载荷/backtrace 到 stderr、日志，并在数据目录追加 `panic.log`。

/// panic 日志目录（setup 中 app_data_dir 确定后注入）；未注入前 panic 至少落 stderr + 日志
pub(crate) static PANIC_LOG_DIR: std::sync::OnceLock<std::path::PathBuf> =
    std::sync::OnceLock::new();

/// 安装全局 panic 钩子（最早安装：任何后续初始化（含 GTK/WebKit）中 panic 都有证据）
pub(crate) fn install() {
    std::panic::set_hook(Box::new(|info| {
        let thread_name = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<非字符串 panic 载荷>".to_string());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[panic] unix_ts={ts} thread={thread_name} location={location}\n          payload: {payload}\n{backtrace}"
        );
        // 1. stderr：logger 尚未初始化的最早启动阶段也能保留
        eprintln!("{line}");
        // 2. tauri-plugin-log（初始化后进入正式日志文件）
        log::error!("{line}");
        // 3. 数据目录 panic.log 追加落盘（用户事后可提供）
        if let Some(dir) = PANIC_LOG_DIR.get() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("panic.log"))
            {
                let _ = writeln!(f, "{line}\n");
            }
        }
    }));
}

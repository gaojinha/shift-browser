//! 致命信号兜底（2026-10-19）
//!
//! Rust panic 由 panic_hook 兜住；但 GTK/WebKit 原生崩溃（SIGSEGV/SIGABRT/SIGBUS/SIGILL）
//! 会直接终结进程，panic hook 根本轮不到——进程静默消失，用户拿不出任何证据。
//!
//! 这里装最小信号处理器：只做 async-signal-safe 的 write(2)，把信号名写到 stderr
//! 和数据目录 `crash.log`，随后恢复默认处理并重新抛出（保留 core dump / 系统崩溃记录原语义）。

#[cfg(unix)]
mod imp {
    use std::os::fd::{IntoRawFd, RawFd};
    use std::sync::atomic::{AtomicI32, Ordering};

    /// 预先打开的 crash.log 文件描述符；-1 = 未就绪（仅写 stderr）
    static CRASH_FD: AtomicI32 = AtomicI32::new(-1);

    /// 信号名：静态字节串，信号上下文里不做任何分配
    fn name(sig: libc::c_int) -> &'static [u8] {
        match sig {
            libc::SIGSEGV => b"[crash] FATAL SIGSEGV\n",
            libc::SIGABRT => b"[crash] FATAL SIGABRT\n",
            libc::SIGBUS => b"[crash] FATAL SIGBUS\n",
            libc::SIGILL => b"[crash] FATAL SIGILL\n",
            _ => b"[crash] FATAL signal\n",
        }
    }

    /// 信号上下文里只做 async-signal-safe 的 write(2)：不碰堆、锁、dladdr。
    /// 调用栈交给 core dump / journalctl（backtrace_symbols_fd 会取加载器锁，
    /// 崩溃瞬间若正持锁会把“崩溃”拖成“卡死”，反倒拿不到证据）。
    unsafe fn emit(fd: RawFd, sig: libc::c_int) {
        let msg = name(sig);
        libc::write(fd, msg.as_ptr() as *const libc::c_void, msg.len());
    }

    extern "C" fn handler(sig: libc::c_int) {
        let fd = CRASH_FD.load(Ordering::Relaxed);
        // SAFETY: 仅调用 POSIX async-signal-safe 接口
        unsafe {
            emit(libc::STDERR_FILENO, sig);
            if fd >= 0 {
                emit(fd, sig);
            }
            // 恢复默认处理并重新抛出：崩溃语义不变，系统 core dump 照常
            libc::signal(sig, libc::SIG_DFL);
            libc::raise(sig);
        }
    }

    /// 安装致命信号处理器（须在 GTK/WebKit 初始化之前调用）
    pub(crate) fn install() {
        // SAFETY: 仅登记处理函数，不触碰 Rust 运行时状态
        unsafe {
            for sig in [libc::SIGSEGV, libc::SIGABRT, libc::SIGBUS, libc::SIGILL] {
                libc::signal(sig, handler as *const () as libc::sighandler_t);
            }
        }
    }

    /// 打开 crash.log 并保留 fd（setup 中 app_data_dir 确定后调用）
    pub(crate) fn set_log_dir(dir: &std::path::Path) {
        if let Ok(file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("crash.log"))
        {
            CRASH_FD.store(file.into_raw_fd(), Ordering::Relaxed);
        }
    }
}

#[cfg(unix)]
pub(crate) use imp::{install, set_log_dir};

#[cfg(not(unix))]
pub(crate) fn install() {}

#[cfg(not(unix))]
pub(crate) fn set_log_dir(_dir: &std::path::Path) {}

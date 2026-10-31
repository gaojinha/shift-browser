//! 统一错误上报机制（F-ERR-01）
//!
//! 注意：项目已有 `crate::error`（`CommandError` 错误码体系，面向 Tauri 命令返回值）。
//! 本模块是**补充**：面向运行期异常的上报与诊断，不改动既有错误码。
//!
//! 目标：**任何异常都能回答四个问题**——哪个模块、什么操作、什么参数、卡在哪。
//!
//! 设计要点：
//! 1. `module` 用点号分层（`cef.navigate` / `db.bookmarks` / `gtk.clipboard`），
//!    便于按模块统计和过滤；
//! 2. `ctx` 是键值对（url / tab / id / path …），排查时不用再猜输入；
//! 3. 全部追加到 `<app_data>/errors.log`，同时在主日志打 error 级；
//! 4. 内存里保留最近 200 条（诊断面板直接读，不依赖文件 IO）；
//! 5. 提供 `guard()`：把可能 panic 的闭包包起来，panic 不穿过 FFI 边界
//!    （CEF 回调是 `extern "C"`，panic 穿过去是 UB，会直接杀死进程且无栈）。
//!
//! 注：`guard`/`diagnostics_text`/`write_diagnostics` 等函数仅在 `cef-engine`
//! 分支（`shell.rs`）中调用，默认配置下触发 dead_code 警告，故在此屏蔽。

#![allow(dead_code)]

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

/// 错误日志文件（setup 中 app_data_dir 确定后注入）
static ERR_LOG_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();

/// 最近错误环形缓冲（诊断面板用）
static RECENT: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

/// 每个模块的错误计数（诊断面板显示「哪个模块最爱出错」）
static COUNTS: OnceLock<Mutex<std::collections::HashMap<&'static str, u64>>> = OnceLock::new();

const RECENT_CAP: usize = 200;

fn recent() -> &'static Mutex<VecDeque<String>> {
    RECENT.get_or_init(|| Mutex::new(VecDeque::with_capacity(RECENT_CAP)))
}

fn counts() -> &'static Mutex<std::collections::HashMap<&'static str, u64>> {
    COUNTS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 注入错误日志目录（与 panic.log 同目录）
pub(crate) fn set_log_dir(dir: &std::path::Path) {
    let _ = ERR_LOG_DIR.set(dir.to_path_buf());
}

/// 一条带上下文的错误
#[derive(Debug, Clone)]
pub struct AppError {
    /// 模块路径，点号分层：`cef.navigate` / `db.bookmarks`
    pub module: &'static str,
    /// 人话描述：发生了什么
    pub detail: String,
    /// 键值上下文：url / tab / id / path …
    pub ctx: Vec<(&'static str, String)>,
}

impl AppError {
    pub fn new(module: &'static str, detail: impl Into<String>) -> Self {
        Self {
            module,
            detail: detail.into(),
            ctx: Vec::new(),
        }
    }

    /// 追加一条上下文（链式）
    pub fn ctx(mut self, key: &'static str, value: impl ToString) -> Self {
        self.ctx.push((key, value.to_string()));
        self
    }

    /// 落盘 + 打日志 + 进环形缓冲。返回自身便于 `?` 链路收尾。
    pub fn report(&self) {
        let ts = local_time_string();
        let ctx = if self.ctx.is_empty() {
            String::new()
        } else {
            let kv: Vec<String> = self.ctx.iter().map(|(k, v)| format!("{k}={v:?}")).collect();
            format!(" ctx{{{}}}", kv.join(", "))
        };
        let line = format!(
            "[{ts}] module={} detail={:?}{ctx}",
            self.module, self.detail
        );

        // 1. 主日志（errata：error 级，会被 tauri-plugin-log 收进主日志文件）
        log::error!("[err] {line}");

        // 2. 环形缓冲（诊断面板）
        {
            let mut r = recent().lock().unwrap_or_else(|e| e.into_inner());
            if r.len() >= RECENT_CAP {
                r.pop_front();
            }
            r.push_back(line.clone());
        }

        // 3. 模块计数
        {
            let mut c = counts().lock().unwrap_or_else(|e| e.into_inner());
            *c.entry(self.module).or_insert(0) += 1;
        }

        // 4. 落盘 errors.log（失败也不抛：错误记录本身不能再制造错误）
        if let Some(dir) = ERR_LOG_DIR.get() {
            let path = dir.join("errors.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }
}

/// 最近错误（诊断面板用，最新的在前）
pub(crate) fn recent_errors(limit: usize) -> Vec<String> {
    let r = recent().lock().unwrap_or_else(|e| e.into_inner());
    r.iter().rev().take(limit).cloned().collect()
}

/// 各模块错误计数（诊断面板用）
pub(crate) fn error_counts() -> Vec<(String, u64)> {
    let c = counts().lock().unwrap_or_else(|e| e.into_inner());
    let mut v: Vec<(String, u64)> = c.iter().map(|(k, n)| ((*k).to_string(), *n)).collect();
    v.sort_by_key(|x| std::cmp::Reverse(x.1));
    v
}

/// 把可能 panic 的代码包起来，panic 不向调用方（尤其 FFI 边界）扩散。
///
/// CEF 回调是 `extern "C"`，Rust panic 穿过 FFI 是未定义行为，通常表现为
/// 进程直接消失、无栈无日志。所有 CEF handler 的方法体都该经它执行。
pub(crate) fn guard<R>(
    module: &'static str,
    op: &'static str,
    f: impl FnOnce() -> R,
    fallback: R,
) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(v) => v,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "<非字符串 panic>".to_string());
            AppError::new(module, format!("panic 被拦截: {detail}"))
                .ctx("op", op)
                .report();
            fallback
        }
    }
}

/// 本地时间字符串（不引 chrono，沿用项目既有做法：直接读系统时区偏移太麻烦，
/// 这里用秒级时间戳 + 可读格式的折中——错误排查靠时序，不要求绝对精确）
fn local_time_string() -> String {
    // SAFETY: localtime_r 是线程安全的，buf 在栈上
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec
        )
    }
}

pub(crate) fn diagnostics_text() -> String {
    let mut out = String::from("=== ShiftBrowser 诊断快照 ===\n");
    out.push_str(&format!("生成时间: {}\n", local_time_string()));
    out.push_str(&format!("进程 PID: {}\n", std::process::id()));
    out.push_str(&format!(
        "线程: {}\n",
        std::thread::current().name().unwrap_or("?")
    ));

    let counts = error_counts();
    out.push_str(&format!("\n错误计数（共 {} 个模块）:\n", counts.len()));
    if counts.is_empty() {
        out.push_str("  （无）\n");
    } else {
        for (m, n) in counts {
            out.push_str(&format!("  {m}: {n}\n"));
        }
    }

    out.push_str("\n最近错误（最新在前，最多 30 条）:\n");
    let recent = recent_errors(30);
    if recent.is_empty() {
        out.push_str("  （无）\n");
    } else {
        for e in recent {
            out.push_str(&format!("  {e}\n"));
        }
    }
    out
}

/// 把诊断快照写到 `<app_data>/diagnostics.txt`，返回路径
pub(crate) fn write_diagnostics() -> Option<std::path::PathBuf> {
    let dir = ERR_LOG_DIR.get()?;
    let path = dir.join("diagnostics.txt");
    std::fs::write(&path, diagnostics_text()).ok()?;
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctx_chain_keeps_order() {
        let e = AppError::new("test.mod", "boom")
            .ctx("url", "https://a.com")
            .ctx("tab", 3);
        assert_eq!(e.module, "test.mod");
        assert_eq!(e.ctx.len(), 2);
        assert_eq!(e.ctx[0].0, "url");
        assert_eq!(e.ctx[1].1, "3");
    }

    #[test]
    fn guard_returns_fallback_on_panic() {
        let v = guard("test.guard", "op", || -> i32 { panic!("故意炸") }, -1);
        assert_eq!(v, -1);
    }

    #[test]
    fn guard_passes_through_normal_value() {
        let v = guard("test.guard", "op", || 42, -1);
        assert_eq!(v, 42);
    }

    #[test]
    fn recent_errors_capped_and_newest_first() {
        for i in 0..(RECENT_CAP + 5) {
            AppError::new("test.cap", format!("e{i}")).report();
        }
        let r = recent_errors(3);
        assert_eq!(r.len(), 3);
        assert!(r[0].contains(&format!("e{}", RECENT_CAP + 4)));
    }
}

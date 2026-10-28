//! CEF UI 线程投递辅助（F-CEF-24a）
//!
//! 提供把闭包投递到 CEF UI 线程的两个入口：
//! - [`cef_ui`]：fire-and-forget，已在 CEF UI 线程则原地执行，否则 post；
//! - [`cef_ui_call`]：同步调回传，已在 CEF UI 线程则原地执行返回，否则 post + mpsc 一次性回传。
//!
//! ## 死锁铁律（正确性前提，不是优化）
//!
//! 两个辅助的 `on_ui_thread()` 短路是**正确性前提**：CEF 回调链里
//! （on_window_created / on_after_created / handle_action 等，本就在 CEF UI 线程）
//! 调 `cef_ui_call` 若不自短路，会 post 给自己、自己的循环被自己阻塞，必死锁。
//! 因此 CEF UI 线程上跑的任务里绝不能再经本函数回头阻塞等
//! GTK 主线程 / tokio（短路天然保证）。
//!
//! 投递走 [`UiExecutor`]：默认实现 [`CefUiExecutor`]（真 CEF
//! `cef::post_task`）；单测注入进程内 stub，不依赖真 CEF 初始化。
//!
//! 本模块先交付后接线：24b/24c 才把 17 个 thread_local 与组 A 12 个入口
//! 切到 `cef_ui`/`cef_ui_call`（本卡禁改 shell.rs / browser.rs / toolbar.rs /
//! cef_cmd.rs），整模块暂无业务引用——scoped dead_code 豁免（同
//! `groups_ui.rs` 交付期），**接线完成后删掉下一行 `#![allow]`**。
#![allow(dead_code)]

use std::sync::{mpsc, Arc, Mutex};

use cef::{wrap_task, ImplTask, Task, ThreadId, WrapTask};
// wrap_task! 展开的 Clone 实现调 `add_ref()`（cef::rc::Rc trait 方法），
// 需在调用点作用域导入 trait 才能解析（cef crate 内部同款 `pub use rc::Rc as _;`）。
use cef::rc::Rc as _;

/// 可注入的「CEF UI 线程」执行器。
///
/// 真机实现 [`CefUiExecutor`]（`cef::currently_on` / `cef::post_task`）；
/// 单测用进程内 stub 替换，脱离真 CEF 验证短路与回传逻辑。
pub trait UiExecutor: Send + Sync {
    /// 当前是否已在 CEF UI 线程（短路依据）。
    fn on_ui_thread(&self) -> bool;
    /// 把一个任务投递到 CEF UI 线程执行（fire-and-forget，不等待）。
    fn post(&self, f: Box<dyn FnOnce() + Send>);
}

/// 默认实现：真 CEF UI 线程。
#[derive(Default)]
pub struct CefUiExecutor;

impl UiExecutor for CefUiExecutor {
    fn on_ui_thread(&self) -> bool {
        cef::currently_on(ThreadId::UI) == 1
    }

    fn post(&self, f: Box<dyn FnOnce() + Send>) {
        let mut task = UiPostTask::new(Arc::new(Mutex::new(Some(f))));
        // post_task 返回 0 = 未入队（CEF 未初始化/已销毁）。任务只在 CEF 初始化后
        // 才会被投递，正常运行不会失败；若真失败，闭包随 task 释放，
        // `cef_ui_call` 侧表现为回传通道错误（见该函数说明）。
        cef::post_task(ThreadId::UI, Some(&mut task));
    }
}

// 投递闭包的共享容器：Arc（wrap_task! 生成的 Clone 对字段调 .clone()，
// Box<dyn FnOnce> 不 Clone）+ Mutex（execute(&self) 只有共享引用，
// take() 保证恰好执行一次；任务执行完毕后由 CEF 释放）。
type ClosureBox = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

// wrap_task! 会生成 Clone/Rc/ImplTask/WrapTask 实现；
// 任务执行完毕后由 CEF 释放，Option 不会被二次取用。
wrap_task! {
    struct UiPostTask {
        closure: ClosureBox,
    }

    impl Task {
        fn execute(&self) {
            // 锁毒化降级为跳过（毒化即本任务已炸，回传侧按通道错误处理）。
            if let Ok(mut guard) = self.closure.lock() {
                if let Some(f) = guard.take() {
                    f();
                }
            }
        }
    }
}

/// 核心逻辑（executor 可注入，供单测）；公开入口 [`cef_ui`] 用默认实现。
fn cef_ui_with<E: UiExecutor + ?Sized>(ex: &E, f: impl FnOnce() + Send + 'static) {
    if ex.on_ui_thread() {
        f();
    } else {
        ex.post(Box::new(f));
    }
}

/// 投递到 CEF UI 线程：已在 CEF UI 线程则原地执行，否则 post（fire-and-forget）。
///
/// 用于「触发后不等结果」的 UI 操作（刷面板、跑工具栏 JS 等）。
pub fn cef_ui(f: impl FnOnce() + Send + 'static) {
    cef_ui_with(&CefUiExecutor, f)
}

/// 同步调核心逻辑（executor 可注入，供单测）；公开入口 [`cef_ui_call`] 用默认实现。
fn cef_ui_call_with<R, E>(ex: &E, f: impl FnOnce() -> R + Send + 'static) -> R
where
    R: Send + 'static,
    E: UiExecutor + ?Sized,
{
    if ex.on_ui_thread() {
        return f();
    }
    let (tx, rx) = mpsc::channel();
    ex.post(Box::new(move || {
        let r = f();
        // 接收端已放弃等待时丢弃结果（通道 Send 错误静默）。
        let _ = tx.send(r);
    }));
    // 超时兜底：CEF UI 线程若被 CEF 内部长时间占用，无限 recv 会让调用方
    // （通常是 GTK 主线程）永久卡死，表面就是整窗「无响应」。
    match rx.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(r) => r,
        // 所有发送端被 drop 且未回传 = post 未入队（CEF 未初始化/已销毁）。
        // 正常运行不走到这里；走到即调用方在 CEF 就绪前误用（编程错误）。
        // f 已移入投递闭包无法原地降级执行，只能显式失败。
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("cef_ui_call: 等 CEF UI 线程超过 10s（线程被占用或已死锁）")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("cef_ui_call: 任务未被 CEF UI 线程执行（CEF 未初始化或已销毁）")
        }
    }
}

/// 在 CEF UI 线程同步执行并等待结果：已在 CEF UI 线程则原地执行返回，
/// 否则 post + mpsc 一次性回传。
///
/// 用于「必须拿结果」的入口（open 拿 tab_id、`cef_open_window` 等）。
///
/// **死锁铁律**：上面的 `on_ui_thread()` 短路是正确性前提——CEF 回调链里
/// 调本函数若不自短路，会 post 给自己、自己的循环被自己阻塞，必死锁。
pub fn cef_ui_call<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    cef_ui_call_with(&CefUiExecutor, f)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// 进程内测试执行器，脱离真 CEF：
    /// - `on_ui` 控制短路分支；
    /// - `run_inline=true`：post 时立即执行（模拟 UI 线程当场取走）；
    /// - 否则入队 FIFO，由测试调 [`drain`](InlineExecutor::drain) 模拟 UI 循环取走。
    struct InlineExecutor {
        on_ui: bool,
        run_inline: bool,
        posts: AtomicUsize,
        queue: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
    }

    impl InlineExecutor {
        fn new(on_ui: bool, run_inline: bool) -> Self {
            Self {
                on_ui,
                run_inline,
                posts: AtomicUsize::new(0),
                queue: Mutex::new(Vec::new()),
            }
        }

        /// 按 FIFO 执行已入队任务（模拟 CEF UI 线程循环取走）。
        fn drain(&self) {
            let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
            let tasks = std::mem::take(&mut *q);
            drop(q);
            for f in tasks {
                f();
            }
        }

        fn post_count(&self) -> usize {
            self.posts.load(Ordering::SeqCst)
        }
    }

    impl UiExecutor for InlineExecutor {
        fn on_ui_thread(&self) -> bool {
            self.on_ui
        }

        fn post(&self, f: Box<dyn FnOnce() + Send>) {
            self.posts.fetch_add(1, Ordering::SeqCst);
            if self.run_inline {
                f();
            } else {
                let mut q = self.queue.lock().unwrap_or_else(|e| e.into_inner());
                q.push(f);
            }
        }
    }

    /// 1) 短路：on_ui_thread()==true → 原地执行，post 0 次。
    #[test]
    fn cef_ui_short_circuits_on_ui_thread() {
        let ex = InlineExecutor::new(true, false);
        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();
        cef_ui_with(&ex, move || ran2.store(true, Ordering::SeqCst));
        assert!(ran.load(Ordering::SeqCst), "UI 线程上应原地执行");
        assert_eq!(ex.post_count(), 0, "UI 线程上不得 post（防自 post 死锁回归）");
    }

    /// 2) 非 UI 线程：恰好 post 1 次，取走前不执行、取走后执行。
    #[test]
    fn cef_ui_posts_once_off_ui_thread() {
        let ex = InlineExecutor::new(false, false);
        let ran = Arc::new(AtomicBool::new(false));
        let ran2 = ran.clone();
        cef_ui_with(&ex, move || ran2.store(true, Ordering::SeqCst));
        assert_eq!(ex.post_count(), 1);
        assert!(!ran.load(Ordering::SeqCst), "入队任务取走前不得执行");
        ex.drain();
        assert!(ran.load(Ordering::SeqCst));
    }

    /// 3) 回传：cef_ui_call 返回投递闭包的正确值（mpsc 一次性回传）。
    #[test]
    fn cef_ui_call_returns_value() {
        let ex = InlineExecutor::new(false, true);
        let r: i32 = cef_ui_call_with(&ex, || 41 + 1);
        assert_eq!(r, 42);
        assert_eq!(ex.post_count(), 1);
    }

    /// 4) cef_ui_call 同样短路：on_ui_thread()==true → 原地执行返回，post 0 次。
    #[test]
    fn cef_ui_call_short_circuits_on_ui_thread() {
        let ex = InlineExecutor::new(true, false);
        let r: &str = cef_ui_call_with(&ex, || "原地执行");
        assert_eq!(r, "原地执行");
        assert_eq!(ex.post_count(), 0, "UI 线程上不得 post（防自 post 死锁回归）");
    }

    /// 5) 顺序：连续 post 两个任务，UI 循环按 FIFO 取走，执行顺序不变。
    #[test]
    fn cef_ui_posts_keep_fifo_order() {
        let ex = InlineExecutor::new(false, false);
        let order = Arc::new(Mutex::new(Vec::new()));
        let o1 = order.clone();
        cef_ui_with(&ex, move || {
            let mut g = o1.lock().unwrap_or_else(|e| e.into_inner());
            g.push("first");
        });
        let o2 = order.clone();
        cef_ui_with(&ex, move || {
            let mut g = o2.lock().unwrap_or_else(|e| e.into_inner());
            g.push("second");
        });
        ex.drain();
        assert_eq!(*order.lock().unwrap_or_else(|e| e.into_inner()), vec!["first", "second"]);
    }
}

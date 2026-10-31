//! 对 `Browser` 句柄的轻量操作（从 `browser.rs` 拆出）。
//!
//! 这两个函数的共同点：只依赖 `Browser`/`Frame` 句柄，不碰本模块的静态表
//! （`BROWSERS`/`CLIENTS_ALIVE`/`PENDING_SIZE`），放一起便于后续扩展
//! （如 `back`/`forward`/`reload` 也可归到这里）。

use cef::*;

/// 让浏览器获得焦点（地址栏 ↔ 页面切换、标签切换时调用）。
pub fn focus(browser: &Browser) {
    if let Some(host) = browser.host() {
        host.set_focus(1);
    }
}

/// 在页面主框架执行 JS（忽略返回值）。
///
/// 这是 P2 快照 / Agent 动作的注入口；真正的"带返回值"版本需要接
/// `execute_java_script` 的 result callback（见规划第五节），当前接口只保证下发。
#[allow(dead_code)]
pub fn eval_js(browser: &Browser, script: &str) {
    let Some(frame) = browser.main_frame() else {
        return;
    };
    let code = CefString::from(script);
    let script_url = CefString::from("shift://inject");
    frame.execute_java_script(Some(&code), Some(&script_url), -1);
}

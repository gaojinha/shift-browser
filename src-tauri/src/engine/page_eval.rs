//! 页面 JS 执行抽象（P1-5 步 1：WebKit/CEF 双引擎统一注入口）。
//!
//! 动机：`content_snapshot` / Agent 4 动作（click/fill/read/submit）原本
//! 硬绑 `webkit2gtk::WebView`（`with_webview` + `eval_js_with`），CEF 路径
//! 完全没有注 JS 收回结果的通道（`cef::Frame::execute_java_script` 在
//! cef 154.5.0 绑定里只有 fire-and-forget，无 visitor/回调参数）。
//!
//! 本 trait 只抽象"注一段 JS 回去收字符串结果"这一层，脚本内容（
//! `SNAPSHOT_JS` / `build_action_js` 产物）保持引擎无关的纯字符串模板。
//!
//! 线程铁律（WebKit/CEF 各自实现遵守，本文件不强制）：
//! - WebKit 回调在 GTK 主线程，发起方绝不能在 `with_webview` 闭包内
//!   `recv_timeout`（主线程阻塞 → 回调永不触发 → 死锁）；
//! - CEF 回调在 CEF UI 线程（当前实现里即 GTK 主线程），同理。
//!   两个实现都是"只发起、不等待"：结果经 `std::sync::mpsc::Sender` 异步
//!   送回，调用方在闭包外等待。

use std::sync::mpsc::Sender;

/// CEF 侧 JS 结果回传前缀（`console.log` 通道）。
///
/// 注入的 JS 把结果编码成 `<前缀><req_id>:<OK|ERR>:<urlencoded>` 打给
/// `console.log`，`DisplayHandler::on_console_message` 按这个前缀把事件路由
/// 到 `CefEvent::EvalResult`（而不是普通 `Console`），消费方再按 req_id
/// 匹配 pending 表。前缀带下划线 + 大写命名空间，站点误撞概率极低；就算
/// 误撞，消费方匹配不到 pending 的 req_id 直接丢弃，无安全面（eval 脚本
/// 全是固定模板，用户输入只经 `build_action_js` 的 JSON 序列化进脚本体）。
/// 注：`CE_EVAL_PREFIX` 本身被 `cef` 侧（默认配置下不编译）引用，WebKit 默认
/// 构建里无使用方，scoped 豁免（契约常量，保留）。
#[allow(dead_code)]
pub const CE_EVAL_PREFIX: &str = "__shift_eval__:";

/// 页面 JS 执行器：执行 `script`，把字符串结果经 `tx` 异步送回（成功 = JS
/// 返回值转字符串；失败 = 引擎侧执行错误）。
///
/// 实现必须保证：
/// - `script` 执行是"发起即返回"，绝不在实现内部等待结果；
/// - 结果恰好发送一次（引擎保证回调只触发一次，失败也发 `Err`，不发两次）。
///
/// 本 trait 是 CEF/WebKit 统一接口的过渡预留：WebKit 现有调用点仍直接用
/// `engine::webview::eval_js_with`（行为零改动优先，未走 trait），CEF 侧
/// 调用方（`content_snapshot_cef` / `execute_agent_action_cef`，仅
/// `--features cef-engine` 编译）会按 `&dyn PageEval` 派发——`trait`/`impl`
/// 本身在两种配置下都不存在使用方，scoped `allow(dead_code)`（同
/// `browser.rs` 里 `eval_js` 的口径），调用方真正接入后删掉。
#[allow(dead_code)]
pub trait PageEval {
    /// 执行脚本，结果经 `tx` 异步送回。`script` 必须是返回字符串（或可被
    /// 引擎转成字符串）的表达式；两个引擎实现都按"JS 返回值 → 字符串"处理。
    fn eval_js_with(&self, script: &str, tx: Sender<Result<String, String>>);
}

/// WebKitGTK 实现：内部就是现有 `engine::webview::eval_js_with`，行为零变化。
/// 调用方需在 `wv.with_webview(move |pwv| WebKitPageEval(&pwv.inner()).eval_js_with(...))`
/// 里发起，闭包外 `recv_timeout` 等待。
///
/// `#[allow(dead_code)]`：WebKit 现有调用点仍直接用 `eval_js_with`（未走本
/// trait，行为零改动优先），本实现是 CEF/WebKit 统一接口的过渡预留——
/// 调用方接入后删掉（同 `browser.rs` 里 `eval_js` 的口径，不是遮羞）。
#[allow(dead_code)]
pub struct WebKitPageEval<'a> {
    wv: &'a webkit2gtk::WebView,
}

impl<'a> WebKitPageEval<'a> {
    #[allow(dead_code)]
    pub fn new(wv: &'a webkit2gtk::WebView) -> Self {
        Self { wv }
    }
}

impl PageEval for WebKitPageEval<'_> {
    fn eval_js_with(&self, script: &str, tx: Sender<Result<String, String>>) {
        crate::engine::webview::eval_js_with(self.wv, script, move |res| {
            let _ = tx.send(res);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 可空实现：`eval_js_with` 永远发 `Err`。用于 CEF 侧链路单测——
    /// 不依赖真实 WebView/CefString，把"发起 → 收结果"这条 trait 契约
    /// 本身跑通。
    struct NoopEval;
    impl PageEval for NoopEval {
        fn eval_js_with(&self, _script: &str, tx: Sender<Result<String, String>>) {
            let _ = tx.send(Err("noop：测试用，永远失败".to_string()));
        }
    }

    /// 恒等实现：把 `script` 原样当"结果"发回。验证 trait 的正常成功路径。
    struct EchoEval;
    impl PageEval for EchoEval {
        fn eval_js_with(&self, script: &str, tx: Sender<Result<String, String>>) {
            let _ = tx.send(Ok(script.to_string()));
        }
    }

    #[test]
    fn trait_contract_error_path() {
        let (tx, rx) = std::sync::mpsc::channel();
        NoopEval.eval_js_with("任意脚本", tx);
        let res = rx.recv_timeout(std::time::Duration::from_secs(1));
        assert!(res.is_ok(), "失败路径也必须发一次结果（引擎契约）");
        assert!(res.unwrap().is_err());
    }

    #[test]
    fn trait_contract_success_path() {
        let (tx, rx) = std::sync::mpsc::channel();
        EchoEval.eval_js_with("1+1", tx);
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .unwrap(),
            "1+1"
        );
    }
}

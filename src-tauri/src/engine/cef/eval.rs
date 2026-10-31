//! CEF 侧 `PageEval` 实现（P1-5 步 1）：console 前缀路由回收 JS 结果。
//!
//! 机制：`cef::Frame::execute_java_script`（cef 154.5.0 绑定）无回调参数，
//! 只能 fire-and-forget。本模块把用户 JS 包成
//! ```js
//! (() => {
//!   const __shiftEvalRun = () => {
//!     const __shiftEvalResult = (() => { <用户JS> })();
//!     try {
//!       console.log("__shift_eval__:<req_id>:OK:" +
//!         encodeURIComponent(JSON.stringify(__shiftEvalResult === undefined ? null : __shiftEvalResult)));
//!     } catch (e) {
//!       console.log("__shift_eval__:<req_id>:ERR:" + encodeURIComponent(String(e)));
//!     }
//!   };
//!   try { __shiftEvalRun(); }
//!   catch (e) { console.log("__shift_eval__:<req_id>:ERR:" + encodeURIComponent(String(e))); }
//! })()
//! ```
//! 结果经 `console.log` 回打 → `CefTabDisplayHandler::on_console_message`
//! （`browser.rs`）按 `CE_EVAL_PREFIX` 前缀路由到 `CefEvent::EvalResult` →
//! `cef_cmd::dispatch_eval_result` 匹配 pending 表里对应 req_id 的 sender，
//! 解开 `urlencoded` 字符串后经 `std::sync::mpsc::Sender` 交回调用方。
//!
//! 线程模型：
//! - `CefPageEval::eval_js_with` 要求调用方在 **CEF UI 线程**发起（拿
//!   `BROWSERS` 锁 + `execute_java_script` 都在 UI 线程）；
//! - 结果回收在**任意线程**：`dispatch_eval_result` 只做 HashMap 查改，
//!   不碰 `Browser` 句柄，`on_console_message`（UI 线程）里只 `send` 事件
//!   立即返回，不阻塞。
//!
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};

use cef::{ImplBrowser, ImplFrame};

use super::browser::browsers;
use crate::engine::page_eval::{PageEval, CE_EVAL_PREFIX};

/// req_id 自增（全局唯一，不可猜；站点误打前缀也匹配不到 pending 直接丢）。
static REQ_ID: AtomicU64 = AtomicU64::new(1);

/// 挂起的 eval 请求：`req_id → 结果发送端`。消费方（`dispatch_eval_result`）
/// 匹配到就发结果并删掉；调用方 `recv_timeout` 超时后 sender 被 drop，
/// 迟到的结果 `send` 返回 `Err(_)` 静默丢弃（`Sender` 单发语义）。
/// 挂起结果类型（type 别名，规避 clippy type_complexity）。
type EvalWaiter = Sender<Result<String, String>>;
type EvalPending = Mutex<std::collections::HashMap<u64, EvalWaiter>>;

static PENDING: OnceLock<EvalPending> = OnceLock::new();

fn pending() -> &'static EvalPending {
    PENDING.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// CEF 引擎的 `PageEval` 实现：按 `tab_id` 定位 `BROWSERS` 里的浏览器句柄。
/// 单栈窗口模式下固定是 `shell::CONTENT_ID`。
pub struct CefPageEval {
    tab_id: String,
}

impl CefPageEval {
    pub fn new(tab_id: impl Into<String>) -> Self {
        Self {
            tab_id: tab_id.into(),
        }
    }

    fn inject(&self, wrapped_script: &str) {
        // 不走 shell::lock_state（那是 shell 侧锁，带自死锁 thread_local 检测，
        // 本模块不持任何其他锁，直接拿 BROWSERS 原锁即可，也避免 engine::cef
        // 反向依赖 cef_cmd 的模块别名）。
        let frame = {
            let m = browsers().lock().unwrap_or_else(|e| e.into_inner());
            m.get(&self.tab_id).and_then(|b| b.main_frame())
        };
        if frame.is_none() {
            log::debug!("[cef-eval] 找不到 tab {} 的 frame，跳过注入", self.tab_id);
        }
        if let Some(frame) = frame {
            let code = cef::CefString::from(wrapped_script);
            frame.execute_java_script(Some(&code), Some(&cef::CefString::from("shift://eval")), -1);
        }
    }
}

/// 纯字符串契约：注入脚本必须把 `req_id`/`CE_EVAL_PREFIX` 完整嵌进去，
/// 且外层有兜底 try/catch（脚本首行炸时 ERR 也必发）。不碰 CEF 句柄，
/// 直接测"包装函数"本身（提出来单测，而不是藏在 `eval_js_with` 里
/// 依赖"空注入"路径）。
fn wrap_eval_script(script: &str, req_id: u64) -> String {
    let prefix = CE_EVAL_PREFIX;
    format!(
        "(() => {{ try {{ const __shiftEvalResult = (() => {{ {script} }})(); \
         try {{ console.log({prefix:?} + {req_id} + ':OK:' + \
         encodeURIComponent(JSON.stringify(__shiftEvalResult === undefined ? null : __shiftEvalResult))); }} \
         catch (e) {{ console.log({prefix:?} + {req_id} + ':ERR:' + encodeURIComponent(String(e))); }} }} \
         catch (e) {{ console.log({prefix:?} + {req_id} + ':ERR:' + encodeURIComponent(String(e))); }} }})();"
    )
}

impl PageEval for CefPageEval {
    fn eval_js_with(&self, script: &str, tx: Sender<Result<String, String>>) {
        let req_id = REQ_ID.fetch_add(1, Ordering::Relaxed);
        // 先登记 pending 再注入：console 回调可能极快回来（同步 JS 场景），
        // 顺序颠倒会丢结果。
        {
            let mut m = pending().lock().unwrap_or_else(|e| e.into_inner());
            m.insert(req_id, tx);
        }
        let wrapped = wrap_eval_script(script, req_id);
        self.inject(&wrapped);
    }
}

/// 消费 `CefEvent::EvalResult`：按 req_id 匹配 pending 表，命中就发结果
/// （OK 解开 urlencoded；ERR 原样记错误文本），匹配不到直接丢弃。
/// 由 `cef_cmd` 在事件消费循环里调用，线程不敏感（只动 pending 锁）。
pub fn dispatch_eval_result(payload: &str) {
    // payload = "<req_id>:<OK|ERR>:<urlencoded>"
    let (id_part, rest) = match payload.split_once(':') {
        Some(x) => x,
        None => return, // 格式不符，丢弃
    };
    let req_id: u64 = match id_part.parse() {
        Ok(v) => v,
        Err(_) => return,
    };
    let rest = rest.to_string();
    let Some(tx) = pending()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&req_id)
    else {
        return; // 非本进程发起的 eval（站点误撞前缀）或已超时，静默丢弃
    };
    let result = match rest.split_once(':') {
        Some(("OK", encoded)) => {
            let decoded = urlencoding_decode(encoded);
            Ok(decoded)
        }
        Some(("ERR", msg)) => {
            let decoded = urlencoding_decode(msg);
            Err(format!("CEF 页面 JS 执行失败: {decoded}"))
        }
        _ => Err("CEF eval 结果格式异常".to_string()),
    };
    let _ = tx.send(result);
}

/// `encodeURIComponent` 的 Rust 侧解码（只处理字面字符，不引入外部依赖）。
/// 覆盖 `SNAPSHOT_JS`/agent 动作结果的实际字符集（ASCII + 中文 + JSON 引号）；
/// 极端字符（`%` 本身被编码成 `%25`）按 RFC 3986 逐段解。
fn urlencoding_decode(encoded: &str) -> String {
    let bytes = encoded.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 契约守卫：注入脚本的字节结构稳定（前缀 + req_id + OK/ERR 分支都在）。
    /// 不依赖真实 CEF 句柄——`CefPageEval::inject` 拿不到 frame 只记 debug 就返回，
    /// 单测里走这条"空注入"路径验证 pending 登记/清理，不碰 CefString。
    ///
    /// 注意：`REQ_ID` 是全进程共享的 `AtomicU64`（多个测试 / 多次调用都自增它），
    /// 单测**不能假设固定初值**，必须先 fetch 占位再派发。
    fn reserve_req_id() -> u64 {
        REQ_ID.fetch_add(1, Ordering::Relaxed)
    }

    #[test]
    fn pending_registered_then_cleaned_on_dispatch() {
        let (tx, rx) = std::sync::mpsc::channel();
        let next_req_id = reserve_req_id();
        // 直接把预占的 req_id 走 CefPageEval 的 pending 登记逻辑（不走完整
        // eval_js_with，避免脚本里再自增一次 REQ_ID 与这里错位）：
        pending().lock().unwrap().insert(next_req_id, tx);

        assert_eq!(
            pending().lock().unwrap().len(),
            1,
            "发起后 pending 表应登记一项"
        );

        dispatch_eval_result(&format!("{next_req_id}:OK:{}", urlencode("42")));

        let res = rx.recv_timeout(std::time::Duration::from_secs(1));
        assert_eq!(res.unwrap().unwrap(), "42");
        assert_eq!(
            pending().lock().unwrap().len(),
            0,
            "消费后 pending 表应清空"
        );
        // 再发一条未知 req_id：必须静默丢弃，不 panic 不发消息
        dispatch_eval_result("999:OK:42");
        assert_eq!(pending().lock().unwrap().len(), 0);
    }

    /// 手工 encode（只测 `dispatch_eval_result` 用到的字符集，不引依赖）。
    fn urlencode(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    out.push(b as char)
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        out
    }

    #[test]
    fn wrap_eval_script_embeds_prefix_and_req_id() {
        let js = wrap_eval_script("1+1", 42);
        assert!(js.contains(CE_EVAL_PREFIX), "必须嵌入回传前缀: {js}");
        assert!(js.contains("42"), "必须嵌入 req_id: {js}");
        assert!(js.contains("1+1"), "必须原样嵌入用户脚本: {js}");
        // 外层兜底 try/catch：脚本首行炸时 ERR 也必发（数一下外层最左边
        // 的 "try {" 与最右边对应的 "}" 配平即可，不逐字节比对）
        assert!(
            js.starts_with("(() => { try {"),
            "外层兜底 try/catch 缺失: {js}"
        );
        assert!(js.ends_with("})();"), "外层 IIFE 收尾不对: {js}");
    }

    #[test]
    fn urlencoding_decode_roundtrip() {
        // encodeURIComponent 不解码的字符保持原样，其余按 %XX 解
        assert_eq!(urlencoding_decode("hello"), "hello");
        assert_eq!(urlencoding_decode("%E4%BD%A0"), "你"); // 你 = E4 BD A0
        assert_eq!(urlencoding_decode("%25"), "%");
    }
    #[test]
    fn err_path_produces_error() {
        let (tx, rx) = std::sync::mpsc::channel();
        let next_req_id = reserve_req_id();
        pending().lock().unwrap().insert(next_req_id, tx);
        dispatch_eval_result(&format!("{next_req_id}:ERR:{}", urlencode("boom")));
        let res = rx.recv_timeout(std::time::Duration::from_secs(1));
        assert!(res.is_ok(), "ERR 也必须发一次结果（引擎契约）");
        let msg = res.unwrap().unwrap_err();
        assert!(msg.contains("boom"), "错误文本要带原始原因: {msg}");
    }
}

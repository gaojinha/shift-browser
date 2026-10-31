//! console 消息前缀路由（P1-5，从 `browser.rs` 拆出以守住 400 行静态卡）。
//!
//! 单一职责：判定一条 console 消息是否属于 eval 内部回传通道（`CE_EVAL_PREFIX`
//! 前缀），产出对应的 `CefEvent`。同一条判定只在这份实现（契约铁律：
//! 同一类判定只许一份实现，shell.rs 里的前缀分流复用 `CE_EVAL_PREFIX` 常量）。

use super::events::CefEvent;
use crate::engine::page_eval::CE_EVAL_PREFIX;

/// 把一条 console 消息路由成 `CefEvent`。
///
/// 前缀命中 → `EvalResult`（消费方按 req_id 匹配 pending 表）；
/// 不命中 → 普通 `Console`（调试面板展示）。
pub fn route_console_message(level: &str, raw: &str) -> CefEvent {
    match raw.strip_prefix(CE_EVAL_PREFIX) {
        Some(rest) => CefEvent::EvalResult {
            payload: rest.to_string(),
        },
        None => CefEvent::Console {
            level: level.to_string(),
            message: raw.to_string(),
        },
    }
}

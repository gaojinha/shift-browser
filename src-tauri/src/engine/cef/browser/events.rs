//! CEF 标签页事件（从 `browser.rs` 拆出）。
//!
//! 独立的理由：`CefEvent` 是 CEF 回调线程与 Tauri 侧之间的唯一消息契约，
//! 消费方在 `cef_cmd.rs` / `shell.rs`，不依赖 `Browser` 句柄，单独成文件更清楚。

/// 标签页事件：从 CEF 回调发出，交给 Tauri 侧消费。
///
/// `cef_poll_events` 会把事件转成字符串给前端调试；`shell.rs` 消费
/// `AfterCreated` / `LoadStart` / `LoadEnd` / `TitleChanged` / `BeforeClose`。
/// 部分变体字段目前只透传给调试接口，未被打包/界面读取，scoped 放行 dead_code。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum CefEvent {
    /// 浏览器创建完成（此时才能安全调用导航类接口）
    AfterCreated,
    /// 页面标题变化
    TitleChanged(String),
    /// 地址变化（含重定向）
    UrlChanged(String),
    /// 开始加载
    LoadStart(String),
    /// 加载完成
    LoadEnd(String),
    /// 加载失败
    LoadError {
        url: String,
        code: i32,
        text: String,
    },
    /// 页面 console 输出
    Console { level: String, message: String },
    /// 即将关闭
    BeforeClose,
}

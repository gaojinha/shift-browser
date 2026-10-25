//! CEF（Chromium）引擎后端 —— Linux 换核（2026-10-20 起）
//!
//! 目标：用真 Chromium 内核替代 WebKitGTK，让 TLS/JA4、HTTP/2、JS 指纹
//! 天然与 Chrome 一致，不再依赖应用层伪装。
//!
//! 启用方式：`cargo build --features cef-engine`
//! 默认关闭，WebKitGTK 路径完全不受影响。
//!
//! 关键前提（P0 实测结论）：
//! - CEF 必须走 **X11 子窗口**嵌入（`WindowInfo::set_as_child`），
//!   因此进程必须在 CEF 初始化**之前**把 GTK 锁到 X11（`GDK_BACKEND=x11` + 提前 `gtk::init()`）；
//! - CEF 回调是 `extern "C"`，**绝不能让 panic 穿过 FFI 边界**，一律错误返回。

pub mod app;
pub mod browser;
pub mod shell;
pub mod toolbar;

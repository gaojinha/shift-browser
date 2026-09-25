//! shift:// 自定义协议 + 翻译 IPC（R2-1b 抽出 · R1 二次拆分 2026-09-24）
//!
//! 本文件只放三块逻辑共享的 CSP/nonce 原语与嵌入脚本，主体拆分：
//! 1. [`protocol`]：shift:// 协议响应（main 前端资源 / __translate__ 旧通道 /
//!    newtab / worldnav / 未知路径 404）；
//! 2. [`translate`]：翻译 IPC（text/progress/whole-page/batch）+ 原生桥
//!    native_translate_handle（CSP 免疫新通道，extensions.rs 调用）。
//!
//! 共享件：SHIFT_CSP / nonce 注入 / valid_callback / write_translate_result /
//! translate_rate_limited / PROGRESS_THROTTLE / WHOLE_PAGE_TRANSLATE_JS。

// 本模块经 lib.rs 的 #[path = "proto/shift.rs"] 挂载，子模块文件需显式 path
// （否则 2018 edition 会按父目录解析到 proto/protocol.rs）
#[path = "shift/protocol.rs"]
mod protocol;
#[path = "shift/translate.rs"]
mod translate;

// 外部调用面（lib.rs 协议注册、extensions 原生桥）
pub(crate) use protocol::handle;
pub(crate) use translate::native_translate_handle;
// valid_callback 仅 lib_tests 的 JS 注入用例经 proto_shift:: 直接断言
#[allow(unused_imports)]
pub(crate) use translate::valid_callback;
// 注：fresh_script_nonce/inject_script_nonce/csp_with_script_nonce/WHOLE_PAGE_TRANSLATE_JS
// 在本文件定义且本身就是 pub(crate)，tabs 翻译命令与 lib_tests 直接经 proto_shift:: 访问。

/// shift:// 自定义协议页面的 CSP 模板（主窗口/新标签页/世界导航页统一）
/// 注意：Tauri 的 security.csp 只注入 tauri:// 与 http://asset.localhost，
/// 自定义协议（shift://）不走注入，必须在这里手动加响应头。
/// release 构建在响应期把 script-src 的 'unsafe-inline' 替换为 per-request
/// 'nonce-xxx'（见 inject_script_nonce），内联脚本须带同一 nonce 才执行——
/// HTML 注入型 XSS 拿不到响应 nonce，其注入脚本被 WebKit 拒绝。
/// style-src 保留 'unsafe-inline'（大量内联样式/属性，脚本 nonce 是本次收面目标）；
/// object-src 'none' + base-uri 'self' + connect-src 白名单 收紧注入面。
const SHIFT_CSP: &str = "default-src 'self' shift: data:; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https: http:; connect-src 'self' ipc: http://ipc.localhost https://wttr.in https://api.open-meteo.com https://ipinfo.io; font-src 'self' data:; media-src 'self' https: http:; frame-src 'self' https: http:; object-src 'none'; base-uri 'self'";

/// 生成一次响应专用的 CSP nonce（128bit 随机，标准 base64；字符集不含引号/尖括号）
pub(crate) fn fresh_script_nonce() -> String {
    use base64::Engine;
    let raw: [u8; 16] = rand::random();
    base64::engine::general_purpose::STANDARD.encode(raw)
}

/// 把 nonce 注入文档内可信内联脚本起始标签。
///
/// 只匹配精确 `<script>`：外部脚本（`<script type="module" src=...>`）由
/// script-src 'self' 授权，无需 nonce。shift:// 三页（main/newtab/worldnav）
/// 的内联脚本起始标签均为无属性 `<script>`，且模板内 JS 不出现该字面量。
pub(crate) fn inject_script_nonce(html: &str, nonce: &str) -> String {
    html.replace("<script>", &format!("<script nonce=\"{nonce}\">"))
}

/// 用 per-response nonce 替换 CSP 模板 script-src 里的 'unsafe-inline'；
/// style-src 的 'unsafe-inline' 保持不动（只替换 script-src 前缀）。
pub(crate) fn csp_with_script_nonce(nonce: &str) -> String {
    SHIFT_CSP.replace(
        "script-src 'self' 'unsafe-inline'",
        &format!("script-src 'self' 'nonce-{nonce}'"),
    )
}

/// 整页翻译注入脚本（编译期嵌入，运行时 eval 到 child webview）
/// （命令实现见 tabs/commands/nav_cmds.rs 的 translate_current_page —— 那里可访问 TabManager 内部字段）
pub(crate) const WHOLE_PAGE_TRANSLATE_JS: &str = include_str!("../whole_page_translate.js");

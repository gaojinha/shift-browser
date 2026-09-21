//! ShiftBrowser Web 进程扩展（阶段 3.2 · 真扩展体系 · 阶段 A 原型）
//!
//! 由 WebKitGTK 从「web extensions 目录」加载（lib.rs 已 `set_web_extensions_directory`，
//! 指向 exe 所在目录）。入口：导出的 `webkit_web_extension_initialize`。
//!
//! 能力：连接 `WebKitWebPage::send-request` —— 请求出网前可读 URL、返回 TRUE 取消该请求。
//! 这是网络层拦截的最小证明（阶段 B 会把这里的"含标记即取消"换成真规则引擎）。
//!
//! 说明：webkit2gtk-sys 2.0.2 未覆盖 WebProcessExtension API，故手写最小 FFI。
//! cdylib 允许未定义符号，加载时由宿主 Web 进程解析（无需链接 webkit2gtk）。

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_void};

type GCallback = *const c_void;

extern "C" {
    /// glib：连接信号（返回 handler id；失败返回 0）
    fn g_signal_connect_data(
        instance: *mut c_void,
        detailed_signal: *const c_char,
        c_handler: GCallback,
        data: *mut c_void,
        destroy_data: GCallback,
        connect_flags: c_int,
    ) -> u64;
    /// webkit2gtk：取请求的 URI（返回引擎内部字符串，勿释放）
    fn webkit_uri_request_get_uri(request: *mut c_void) -> *const c_char;
}

const SIG_PAGE_CREATED: &[u8] = b"page-created\0";
const SIG_SEND_REQUEST: &[u8] = b"send-request\0";

/// 原型拦截标记：URL 含此片段即取消（阶段 B 换成真规则引擎）
const BLOCK_MARK: &str = "shift-ext-block-test";

/// `send-request` 回调：请求出网前触发；返回 1 (TRUE) = 取消该资源加载
unsafe extern "C" fn on_send_request(
    _page: *mut c_void,
    request: *mut c_void,
    _redirected_response: *mut c_void,
    _user_data: *mut c_void,
) -> c_int {
    if request.is_null() {
        return 0;
    }
    let uri_ptr = webkit_uri_request_get_uri(request);
    if uri_ptr.is_null() {
        return 0;
    }
    let uri = CStr::from_ptr(uri_ptr).to_string_lossy().to_string();
    eprintln!("[shift-ext] send-request: {}", uri);
    if uri.contains(BLOCK_MARK) {
        eprintln!("[shift-ext] 拦截（取消）: {}", uri);
        return 1; // TRUE = 取消
    }
    0
}

/// `page-created` 回调：新页面创建 → 连接它的 send-request
unsafe extern "C" fn on_page_created(
    _extension: *mut c_void,
    page: *mut c_void,
    _user_data: *mut c_void,
) {
    eprintln!("[shift-ext] page-created");
    if page.is_null() {
        return;
    }
    g_signal_connect_data(
        page,
        SIG_SEND_REQUEST.as_ptr() as *const c_char,
        on_send_request as *const c_void,
        std::ptr::null_mut(),
        std::ptr::null(),
        0,
    );
}

/// 扩展入口：WebKit 加载本 .so 后调用
#[no_mangle]
pub unsafe extern "C" fn webkit_web_extension_initialize(extension: *mut c_void) {
    eprintln!("[shift-ext] Web 进程扩展已加载（阶段A 原型）");
    if extension.is_null() {
        eprintln!("[shift-ext] extension 指针为空，跳过");
        return;
    }
    g_signal_connect_data(
        extension,
        SIG_PAGE_CREATED.as_ptr() as *const c_char,
        on_page_created as *const c_void,
        std::ptr::null_mut(),
        std::ptr::null(),
        0,
    );
}

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
    /// webkit2gtk：取请求的 SoupMessageHeaders
    fn webkit_uri_request_get_http_headers(request: *mut c_void) -> *mut c_void;
    /// libsoup3：替换请求头
    fn soup_message_headers_replace(
        headers: *mut c_void,
        name: *const c_char,
        value: *const c_char,
    );
    /// libsoup3：读请求头（校验用）
    fn soup_message_headers_get_one(headers: *mut c_void, name: *const c_char) -> *const c_char;
}

/// GPC 头只打一次日志（避免每请求刷屏）
static GPC_LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 网络层 GPC：给请求加 `Sec-GPC: 1`（W3C Global Privacy Control）。
/// 原先只注入了 JS 属性 `navigator.globalPrivacyControl`，**缺请求头**——这里补上。
fn apply_gpc_header(request: *mut c_void) {
    unsafe {
        let headers = webkit_uri_request_get_http_headers(request);
        if headers.is_null() {
            return;
        }
        soup_message_headers_replace(
            headers,
            b"Sec-GPC\0".as_ptr() as *const c_char,
            b"1\0".as_ptr() as *const c_char,
        );
        if !GPC_LOGGED.swap(true, Ordering::Relaxed) {
            let v = soup_message_headers_get_one(headers, b"Sec-GPC\0".as_ptr() as *const c_char);
            let vs = if v.is_null() {
                "(null)".to_string()
            } else {
                CStr::from_ptr(v).to_string_lossy().to_string()
            };
            eprintln!("[shift-ext] 已注入 Sec-GPC: {}（网络层 GPC）", vs);
        }
    }
}

const SIG_PAGE_CREATED: &[u8] = b"page-created\0";
const SIG_SEND_REQUEST: &[u8] = b"send-request\0";

/// 内置自检标记：URL 含此片段即取消（与规则文件无关，便于冒烟验证扩展活着）
const BLOCK_MARK: &str = "shift-ext-block-test";

/// 规则文件（应用数据目录；每行一个 URL 子串，`#` 开头为注释）。
/// 阶段 B 最小版：网络层按静态规则拦截；后续由主程序写入该文件（如同步广告拦截词库）。
fn rules_path() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    Some(format!(
        "{}/.local/share/com.shiftbrowser.app/shift-ext-rules.txt",
        home
    ))
}

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

static RULES: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
/// 规则文件最近一次加载的 mtime（纳秒；0 = 未加载）
static RULES_MTIME: AtomicU64 = AtomicU64::new(0);

fn rules_cell() -> &'static RwLock<Vec<String>> {
    RULES.get_or_init(|| RwLock::new(Vec::new()))
}

fn file_mtime_ns(path: &str) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// 从文件重载规则（mtime 未变则跳过；无文件则清空）
fn reload_rules() {
    let Some(p) = rules_path() else {
        return;
    };
    let mtime = file_mtime_ns(&p);
    if mtime == 0 || mtime == RULES_MTIME.load(Ordering::Relaxed) {
        return;
    }
    let mut out = Vec::new();
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            for line in text.lines() {
                let l = line.trim();
                if !l.is_empty() && !l.starts_with('#') {
                    out.push(l.to_string());
                }
            }
            eprintln!("[shift-ext] 已加载 {} 条规则（{}）", out.len(), p);
        }
        Err(_) => eprintln!("[shift-ext] 规则文件读取失败（{}），清空规则", p),
    }
    if let Ok(mut g) = rules_cell().write() {
        *g = out;
    }
    RULES_MTIME.store(mtime, Ordering::Relaxed);
}

/// 后台每 2 秒检查规则文件是否更新（热更新：改规则免重启浏览器）
fn spawn_rules_watcher() {
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_secs(2));
        reload_rules();
        reload_headers();
    });
}

/// 该 URL 是否应拦截：内置标记 或 命中任一规则
fn should_block(uri: &str) -> bool {
    if uri.contains(BLOCK_MARK) {
        return true;
    }
    match rules_cell().read() {
        Ok(g) => g.iter().any(|r| uri.contains(r.as_str())),
        Err(_) => false,
    }
}

// ===== 可配置请求头注入（阶段3.2）=====

/// 请求头文件（应用数据目录；每行 `Name: Value`，`#` 注释）
fn headers_path() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    Some(format!(
        "{}/.local/share/com.shiftbrowser.app/shift-ext-headers.txt",
        home
    ))
}

static HEADERS: OnceLock<RwLock<Vec<(String, String)>>> = OnceLock::new();
static HEADERS_MTIME: AtomicU64 = AtomicU64::new(0);

fn headers_cell() -> &'static RwLock<Vec<(String, String)>> {
    HEADERS.get_or_init(|| RwLock::new(Vec::new()))
}

/// 从文件重载请求头（mtime 未变则跳过）
fn reload_headers() {
    let Some(p) = headers_path() else {
        return;
    };
    let mtime = file_mtime_ns(&p);
    if mtime == 0 || mtime == HEADERS_MTIME.load(Ordering::Relaxed) {
        return;
    }
    let mut out = Vec::new();
    match std::fs::read_to_string(&p) {
        Ok(text) => {
            for line in text.lines() {
                let l = line.trim();
                if l.is_empty() || l.starts_with('#') {
                    continue;
                }
                if let Some((n, v)) = l.split_once(':') {
                    let (n, v) = (n.trim(), v.trim());
                    if !n.is_empty() {
                        out.push((n.to_string(), v.to_string()));
                    }
                }
            }
            eprintln!("[shift-ext] 已加载 {} 条自定义请求头（{}）", out.len(), p);
        }
        Err(_) => eprintln!("[shift-ext] 无自定义请求头文件（{}）", p),
    }
    if let Ok(mut g) = headers_cell().write() {
        *g = out;
    }
    HEADERS_MTIME.store(mtime, Ordering::Relaxed);
}

/// 注入自定义请求头（GPC 之外的用户配置头）
fn apply_custom_headers(request: *mut c_void) {
    let Ok(g) = headers_cell().read() else {
        return;
    };
    if g.is_empty() {
        return;
    }
    unsafe {
        let headers = webkit_uri_request_get_http_headers(request);
        if headers.is_null() {
            return;
        }
        for (n, v) in g.iter() {
            let (Ok(nc), Ok(vc)) = (
                std::ffi::CString::new(n.as_str()),
                std::ffi::CString::new(v.as_str()),
            ) else {
                continue;
            };
            soup_message_headers_replace(headers, nc.as_ptr(), vc.as_ptr());
        }
    }
}

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
    // 网络层 GPC：给每个出网请求加 Sec-GPC: 1
    apply_gpc_header(request);
    // 自定义请求头（用户配置）
    apply_custom_headers(request);
    let uri_ptr = webkit_uri_request_get_uri(request);
    if uri_ptr.is_null() {
        return 0;
    }
    let uri = CStr::from_ptr(uri_ptr).to_string_lossy().to_string();
    if should_block(&uri) {
        eprintln!("[shift-ext] 拦截（取消）: {}", uri);
        return 1; // TRUE = 取消
    }
    // 未命中：不打每条（避免刷屏），只统计
    COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    0
}

/// 放行计数（用于"扩展在干活"的轻量可观测）
static COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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
    // 首载规则 + 请求头 + 起后台守护线程（每 2s 查 mtime，改了免重启）
    reload_rules();
    reload_headers();
    spawn_rules_watcher();
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

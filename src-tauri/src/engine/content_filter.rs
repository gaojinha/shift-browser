//! 内容过滤器（网络层规则拦截）——引擎级广告/追踪拦截
//!
//! 背景：项目原有的广告拦截是「CSS 隐藏 + JS 文本关键词扫描」，都发生在**页面渲染之后**，
//! 广告请求该发还是发。WebKit 的内容过滤器在**网络层**拦截请求（Safari 同款机制），
//! 规则由引擎编译成高效匹配器并落盘缓存，效率与覆盖面都高一个量级。
//!
//! 实现要点：
//! - gtk-rs 无绑定 → 手写 FFI（new / save / save_finish / add_filter / remove_filter_by_id）
//! - 编译是异步的（GAsyncReadyCallback），回调在主上下文触发，编译结果缓存为 GObject 引用
//! - 编译只需一次（规则内置），各 webview 创建/重注入时把缓存好的 filter 挂到其
//!   UserContentManager 上即可

use std::ffi::{c_char, c_void, CString};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use gtk::glib;
use gtk::glib::translate::ToGlibPtr;
use webkit2gtk::UserContentManager;

/// 过滤器标识（WebKit 用它做磁盘缓存 key）
const FILTER_ID: &str = "shift-adblock";

/// 广告拦截开关：由注入流程（extensions::collect_enabled_items）同步过来，
/// 供内容过滤器层判断是否挂载（避免在这一层再查库）
static ENABLED: AtomicBool = AtomicBool::new(false);
/// 最近一次提交编译的规则字节数（日志用）
static RULES_LEN: AtomicUsize = AtomicUsize::new(0);

/// 规则编译产物的落盘目录（启动时注册）
static CACHE_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
/// 应用句柄：编译完成后用它触发一次重新注入，把新过滤器挂到已开标签上
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
/// 已提交编译（或正在编译）的规则指纹：避免同一份规则重复编译
static COMPILED_HASH: Mutex<Option<u64>> = Mutex::new(None);
/// 待编译规则（注入流程写入，主线程侧消费）
static PENDING_RULES: Mutex<Option<String>> = Mutex::new(None);

/// 已编译的内容过滤器原始指针（WebKitUserContentFilter*），跨 webview 复用。
/// 存 `usize` 而非 `glib::Object`：`glib::Object` 含裸指针，不满足 Send/Sync，
/// 而 GObject 本身是引用计数的；这里保存的是我们自己持有的那一份引用（故意不释放——
/// 编译只在启动时发生一次，泄漏量恒为一个对象），因此指针生命周期覆盖整个进程。
static CACHED: OnceLock<Mutex<Option<usize>>> = OnceLock::new();

fn cached() -> &'static Mutex<Option<usize>> {
    CACHED.get_or_init(|| Mutex::new(None))
}

// ===== WebKitGTK 内容过滤器 C API（手写 FFI）=====
extern "C" {
    fn webkit_user_content_filter_store_new(storage_path: *const c_char) -> *mut c_void;

    fn webkit_user_content_filter_store_save(
        store: *mut c_void,
        identifier: *const c_char,
        source: *mut c_void, // GBytes*
        cancellable: *mut c_void,
        callback: Option<unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void)>,
        user_data: *mut c_void,
    );

    fn webkit_user_content_filter_store_save_finish(
        store: *mut c_void,
        result: *mut c_void,
        error: *mut *mut c_void,
    ) -> *mut c_void; // WebKitUserContentFilter*

    fn webkit_user_content_manager_add_filter(manager: *mut c_void, filter: *mut c_void);
    fn webkit_user_content_manager_remove_filter_by_id(
        manager: *mut c_void,
        filter_id: *const c_char,
    );
}

/// 编译完成回调（主上下文线程）：取回 filter 并缓存
unsafe extern "C" fn on_save_done(
    source: *mut c_void,
    result: *mut c_void,
    _user_data: *mut c_void,
) {
    let mut err: *mut c_void = std::ptr::null_mut();
    let filter = webkit_user_content_filter_store_save_finish(source, result, &mut err);
    if !err.is_null() {
        log::error!("[engine] 内容过滤器编译失败（规则不合法），本次拦截层未启用");
        return;
    }
    if filter.is_null() {
        log::error!("[engine] 内容过滤器编译返回空");
        return;
    }
    // 保存原始指针（持有引用，故意不释放：生命周期覆盖整个进程）
    *cached().lock().unwrap_or_else(|e| e.into_inner()) = Some(filter as usize);
    log::info!(
        "[engine] 内容过滤器已编译并缓存（规则 {} 字节）",
        RULES_LEN.load(Ordering::Relaxed)
    );
    // 编译完成 → 触发一次重新注入，把新过滤器挂到已打开的标签页上
    // （加/删关键词后无需重启即可生效；规则未变时不会再触发编译，不会成环）
    if let Some(app) = APP.get() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            crate::extensions::re_inject(&app).await;
        });
    }
}

/// 编译并缓存内容过滤器。**必须在主线程调用**（WebKit GObject + 主上下文回调）。
/// `cache_dir` 是规则编译产物的落盘目录（WebKit 自己管理文件）。
pub fn init(cache_dir: &Path, rules_json: &str) {
    if let Err(e) = std::fs::create_dir_all(cache_dir) {
        log::error!("[engine] 内容过滤器缓存目录创建失败: {}", e);
        return;
    }
    let Ok(path_c) = CString::new(cache_dir.to_string_lossy().as_bytes()) else {
        log::error!("[engine] 内容过滤器缓存路径含非法字符");
        return;
    };
    let Ok(id_c) = CString::new(FILTER_ID) else {
        return;
    };
    RULES_LEN.store(rules_json.len(), Ordering::Relaxed);
    unsafe {
        let store = webkit_user_content_filter_store_new(path_c.as_ptr());
        if store.is_null() {
            log::error!("[engine] 内容过滤器 store 创建失败");
            return;
        }
        // store 为 transfer-full（我们持有引用）。异步操作内部会自行持有引用，
        // 这里不主动 unref（一次编译、一个对象，简单且安全）。
        let bytes = glib::Bytes::from(rules_json.as_bytes());
        // 显式标注 ffi 指针类型（否则 `as *mut c_void` 转换处类型推断不完整）
        let bytes_ptr: *mut glib::ffi::GBytes = bytes.to_glib_none().0;
        webkit_user_content_filter_store_save(
            store,
            id_c.as_ptr(),
            bytes_ptr as *mut c_void,
            std::ptr::null_mut(),
            Some(on_save_done),
            std::ptr::null_mut(),
        );
        // glib::Bytes 为引用计数对象：异步保存期间由 WebKit 持有引用。
        // 这里不主动释放（编译只在启动时发生一次，代价可忽略），换取绝对安全。
        std::mem::forget(bytes);
    }
    log::info!("[engine] 内容过滤器编译已提交（异步）");
}

/// 注册应用句柄与规则缓存目录（启动时调用一次）
pub fn register_app(app: tauri::AppHandle, cache_dir: std::path::PathBuf) {
    let _ = APP.set(app);
    let _ = CACHE_DIR.set(cache_dir);
}

/// 记录一份候选规则；返回 true 表示与已编译规则不同、需要重新编译。
/// 由注入流程（extensions::collect_enabled_items）调用。
pub fn remember_rules(rules: &str) -> bool {
    let hash = rules_hash(rules);
    {
        let guard = COMPILED_HASH.lock().unwrap_or_else(|e| e.into_inner());
        if *guard == Some(hash) {
            return false;
        }
    }
    *PENDING_RULES.lock().unwrap_or_else(|e| e.into_inner()) = Some(rules.to_string());
    true
}

/// 消费待编译规则并在主线程编译（如有）。由注入流程在主线程闭包内调用。
pub fn compile_pending() {
    let rules = PENDING_RULES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take();
    let Some(rules) = rules else {
        return;
    };
    let Some(cache_dir) = CACHE_DIR.get() else {
        // 还没注册缓存目录（启动早期）：规则放回待编译队列，等注册后再编
        log::debug!("[engine] 缓存目录未注册，规则暂存待编译");
        *PENDING_RULES.lock().unwrap_or_else(|e| e.into_inner()) = Some(rules);
        return;
    };
    // 先记录指纹：编译是异步的，避免在飞行中重复提交
    *COMPILED_HASH.lock().unwrap_or_else(|e| e.into_inner()) = Some(rules_hash(&rules));
    init(cache_dir, &rules);
}

/// 规则指纹（用于判断是否需要重新编译）
fn rules_hash(rules: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rules.hash(&mut h);
    h.finish()
}

/// 广告拦截开关（供过滤器层判断，避免重复查库）
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// 把已编译的过滤器挂到某个 webview 的 UserContentManager 上。
/// 关闭广告拦截时反向移除。**须在主线程调用**。
pub fn apply_to_manager(manager: &UserContentManager) {
    if !is_enabled() {
        remove_from_manager(manager);
        return;
    }
    let guard = cached().lock().unwrap_or_else(|e| e.into_inner());
    let Some(filter_ptr) = guard.as_ref() else {
        // 还在编译中或编译失败：本次挂载跳过（后续 re_inject 会再尝试）
        log::debug!("[engine] 内容过滤器尚未就绪，跳过挂载");
        return;
    };
    let manager_ptr: *mut webkit2gtk::ffi::WebKitUserContentManager = manager.to_glib_none().0;
    unsafe {
        webkit_user_content_manager_add_filter(
            manager_ptr as *mut c_void,
            *filter_ptr as *mut c_void,
        );
    }
    log::debug!("[engine] 内容过滤器已挂载到 webview");
}

/// 从 UserContentManager 上移除内容过滤器（关闭广告拦截时）
fn remove_from_manager(manager: &UserContentManager) {
    let Ok(id_c) = CString::new(FILTER_ID) else {
        return;
    };
    let manager_ptr: *mut webkit2gtk::ffi::WebKitUserContentManager = manager.to_glib_none().0;
    unsafe {
        webkit_user_content_manager_remove_filter_by_id(manager_ptr as *mut c_void, id_c.as_ptr());
    }
}

/// 内容过滤器状态（给前端/自检展示）
#[allow(dead_code)] // 供后续「隐私/拦截自检面板」与自检命令接入
pub fn status() -> serde_json::Value {
    let ready = cached().lock().unwrap_or_else(|e| e.into_inner()).is_some();
    serde_json::json!({
        "ready": ready,
        "enabled": is_enabled(),
        "rulesBytes": RULES_LEN.load(Ordering::Relaxed),
        "ruleCount": BUILTIN_URL_RULES.len(),
    })
}

/// 内置拦截规则（网络层）：广告网络 / 追踪 / 统计域名。
/// 用简单正则（前后 `.*` 子串匹配）——避免点号转义在 JSON 里产生歧义，
/// 且这些品牌串足够独特，误伤概率极低。
const BUILTIN_URL_RULES: &[&str] = &[
    // 国际广告网络
    ".*doubleclick.*",
    ".*googlesyndication.*",
    ".*googleadservices.*",
    ".*adservice.google.*",
    ".*pagead.*",
    ".*adnxs.*",
    ".*adsrvr.*",
    ".*rubiconproject.*",
    ".*pubmatic.*",
    ".*openx.*",
    ".*casalemedia.*",
    ".*criteo.*",
    ".*taboola.*",
    ".*outbrain.*",
    ".*smartadserver.*",
    ".*amazon-adsystem.*",
    ".*adsafeprotected.*",
    ".*moatads.*",
    ".*serving-sys.*",
    ".*adform.*",
    ".*bidswitch.*",
    ".*sharethrough.*",
    ".*teads.*",
    ".*33across.*",
    ".*gumgum.*",
    ".*smaato.*",
    ".*yieldmo.*",
    ".*360yield.*",
    ".*spotx.*",
    ".*springserve.*",
    // 追踪 / 统计 / 分析
    ".*google-analytics.*",
    ".*googletagmanager.*",
    ".*googletagservices.*",
    ".*scorecardresearch.*",
    ".*quantserve.*",
    ".*quantcount.*",
    ".*hotjar.*",
    ".*clarity.ms.*",
    ".*mixpanel.*",
    ".*segment.io.*",
    ".*amplitude.com.*",
    ".*branch.io.*",
    ".*adjust.com.*",
    ".*appsflyer.*",
    ".*matomo.*",
    ".*piwik.*",
    ".*crazyegg.*",
    ".*mouseflow.*",
    ".*fullstory.*",
    ".*bugsnag.*",
    ".*newrelic.*",
    // 社交像素 / 广告投放
    ".*connect.facebook.*",
    ".*facebook.com/tr.*",
    ".*analytics.twitter.*",
    ".*ads.linkedin.*",
    ".*px.adsrvr.*",
    ".*bat.bing.*",
    // 国内广告 / 统计
    ".*pos.baidu.*",
    ".*cpro.baidu.*",
    ".*cbjs.baidu.*",
    ".*hm.baidu.*",
    ".*union.cnzz.*",
    ".*cnzz.com.*",
    ".*umeng.com.*",
    ".*umengcloud.*",
    ".*mob.alibaba.*",
    ".*tanx.com.*",
    ".*alimama.*",
    ".*mmstat.*",
    ".*admaster.*",
    ".*miaozhen.*",
    ".*talkingdata.*",
    ".*growingio.*",
    ".*sensorsdata.*",
    ".*zhugeio.*",
];

/// 生成 WebKit 内容过滤器规则 JSON（`[{trigger, action}]`，仅内置规则）
#[allow(dead_code)] // 生产路径走 build_rules_with_keywords；此函数供单测与后续自检使用
pub fn build_rules() -> String {
    build_rules_with_keywords(&[])
}

/// 自定义关键词上限（防规则表爆炸）
const MAX_CUSTOM_KEYWORDS: usize = 200;

/// 生成规则 JSON：内置规则 + 用户自定义关键词（关键词也走网络层拦截）
pub fn build_rules_with_keywords(custom: &[String]) -> String {
    let mut patterns: Vec<String> = BUILTIN_URL_RULES.iter().map(|s| s.to_string()).collect();
    for kw in custom.iter().take(MAX_CUSTOM_KEYWORDS) {
        if let Some(p) = keyword_to_pattern(kw) {
            patterns.push(p);
        }
    }
    let mut out = String::with_capacity(patterns.len() * 90 + 2);
    out.push('[');
    for (i, pattern) in patterns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // pattern 只含 `.` `*` 与字母数字汉字连字符，不会产生 JSON 非法转义
        out.push_str(&format!(
            r#"{{"trigger":{{"url-filter":"{}"}},"action":{{"type":"block"}}}}"#,
            pattern
        ));
    }
    out.push(']');
    out
}

/// 关键词 → 规则模式：只保留字母/数字/汉字与 `.` `-`，剔除其它字符
/// （防止用户输入的正则元字符造成非法规则或过度拦截）
fn keyword_to_pattern(keyword: &str) -> Option<String> {
    let cleaned: String = keyword
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    if cleaned.chars().count() < 2 {
        return None;
    }
    Some(format!(".*{}.*", cleaned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_json_is_well_formed() {
        let json = build_rules();
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("规则 JSON 必须能被解析");
        let arr = parsed.as_array().expect("规则必须是数组");
        assert_eq!(arr.len(), BUILTIN_URL_RULES.len());
        for item in arr {
            assert_eq!(item["action"]["type"], "block");
            assert!(item["trigger"]["url-filter"].is_string());
        }
    }

    #[test]
    fn rules_use_valid_regex_chars() {
        // 规则以 .* 开头的子串匹配；不含零宽断言等 WebKit 内容过滤器限制的构造
        for p in BUILTIN_URL_RULES {
            assert!(p.starts_with(".*"), "规则应以 .* 开头: {}", p);
            assert!(!p.contains("(?="), "不应包含零宽断言: {}", p);
            assert!(!p.contains("(?!"), "不应包含负向断言: {}", p);
        }
    }

    #[test]
    fn keyword_rules_are_sanitized_and_appended() {
        let json = build_rules_with_keywords(&[
            "广告".to_string(),    // 有效
            "a*b".to_string(),     // 清洗为 ab，长度 2 → 有效
            "x".to_string(),       // 太短 → 丢弃
            "".to_string(),        // 空 → 丢弃
            "(?=bad)".to_string(), // 元字符被剔除，剩余 bad → 有效
        ]);
        let parsed: serde_json::Value =
            serde_json::from_str(&json).expect("带关键词的规则 JSON 必须可解析");
        let arr = parsed.as_array().expect("规则必须是数组");
        // 内置 + 3 个有效关键词
        assert_eq!(arr.len(), BUILTIN_URL_RULES.len() + 3);
        assert!(json.contains(".*广告.*"));
        assert!(json.contains(".*ab.*"));
        // 正则元字符不得进入规则
        assert!(!json.contains("a*b"));
        assert!(!json.contains("?="));
    }
}

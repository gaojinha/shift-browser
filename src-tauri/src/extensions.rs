// 扩展管理（WebKitGTK UserContentManager 用户脚本/样式注入）
// WebKitGTK 不支持真正的 Chrome 扩展 API，MVP 方案：
// 扩展 = 目录（含 manifest.json + content.js/content.css），
// 注入到所有 http/https 页面（allow_list 限定，不污染 tauri://localhost 的 Vue UI）
// P1-3 拆分（2026-09-22）：广告拦截独立成子模块
#[path = "extensions/adblock.rs"]
pub mod adblock;
#[path = "extensions/manager.rs"]
pub mod manager;
pub use manager::*;
#[path = "extensions/privacy_cmd.rs"]
pub mod privacy_cmd;
pub use privacy_cmd::*;
#[path = "extensions/privacy.rs"]
pub mod privacy;
pub use privacy::*;

use crate::db::extensions as ext_db;
use crate::db::Db;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, State};

// ---- 隐私防护统计（让防护"看得见"）----
/// URL 追踪参数清洗次数（每次剥离参数 +1）
pub static CLEANED_PARAMS: AtomicU64 = AtomicU64::new(0);
/// 隐私防护脚本注入次数（每注入一个 webview +1）
pub static PRIVACY_INJECTIONS: AtomicU64 = AtomicU64::new(0);
/// 广告拦截脚本注入次数
pub static ADBLOCK_INJECTIONS: AtomicU64 = AtomicU64::new(0);

/// 隐私防护统计（前端展示用）
#[derive(Debug, Serialize, Clone)]

/// 读取扩展目录内容
#[derive(Deserialize)]
struct ContentScript {
    #[serde(default)]
    js: Vec<String>,
    #[serde(default)]
    css: Vec<String>,
}

/// 扩展目录解析结果：(name, version, description, 合并后的 js, 合并后的 css, js 文件数, css 文件数)
type ExtensionDirResult = (
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    usize,
    usize,
);

#[cfg(target_os = "linux")]
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
    UserStyleLevel, UserStyleSheet, WebViewExt,
};


/// 在主线程把所有 child webview 的 UserContentManager 清空并注入扩展
/// UserContentManager 是 webview 级（非 WebContext 级），必须逐个 webview 注入
/// 右键翻译菜单脚本（编译期嵌入）
/// 复用 shift://__translate__ 协议（translate.rs + whole_page_translate.js），
/// 补上一直缺失的 UI 入口：选中文字右键 → 翻译选中/翻译整页。
const CONTEXT_TRANSLATE_JS: &str = include_str!("context_translate.js");

/// 选区缓存监听（见 page_selwatch.js 文件头）。
///
/// 为什么必须在这里也注册一遍（2026-09-27 修「翻译选中」首击必失败）：
/// - `WebviewBuilder::initialization_script`（create.rs）在本机这条构建路径
///   实测不生效；
/// - page_load.rs 里的即时 eval 时机脆弱，且每次 re_inject 都先
///   `remove_all_scripts()`，老标签/后退恢复页面上监听过一次也可能丢；
/// - 作为**常驻 UserScript**（document_start / AllFrames / allow "*"）加进
///   注入清单后，之后每次导航都由 WebKit 自动带上，跟广告拦截/隐私脚本同寿命。
///
/// 脚本自身幂等（__sbSelWatch），与另外两条注入路径重复执行也无害。
const SELWATCH_JS: &str = include_str!("page_selwatch.js");

/// 内置隐私防护脚本（P0：GPC + 反指纹随机化 + WebRTC 防泄露）
///
/// 设计要点（依据 Brave/Tor 的公开实践）：
/// - **随机化路线**：Canvas/WebGL/Audio 加入"同源稳定、跨源不同"的确定性噪声，
///   避免出现"每源都变"这种可被识别的加噪特征（UCSD IMC25 论文指出该识别手法）。
/// - **GPC**：W3C 规范除 Sec-GPC 请求头外，还有 navigator.globalPrivacyControl 属性；
///   WebKitGTK 未暴露请求头改写 API，故用 JS 属性表达（主流合规网站可识别）。
/// - **WebRTC**：过滤 icecandidate 中的 host 候选（内网 IP 泄露主因），保留公网候选，
///   不整体禁用 WebRTC（避免破坏视频会议类网站）。
const PRIVACY_JS: &str = include_str!("../resources/js/privacy.js");

/// 严格档隐私脚本（在标准档基础上追加，对标 Brave Aggressive / Tor 的部分做法）
///
/// 与标准档的分工：
/// - 标准档：只做"收敛 + 加噪"，尽量不破坏网站功能（日常默认）
/// - 严格档：额外**移除高危/高熵 API**，牺牲部分功能换取更小指纹面
///
/// 严格档会禁用：WebUSB / Web Bluetooth / Web Serial / WebHID / Gamepad / 传感器 API，
/// 归一化语音合成音色列表，并把高精度计时降级（抗时序侧信道指纹）。
const STRICT_PRIVACY_JS: &str = include_str!("../resources/js/strict_privacy.js");

/// 隐私防护自测脚本（阶段 2）：在页面里核对各项防护是否**真的生效**。
///
/// 设计：只读 + 只返回 JSON，不改页面状态。核对 `__shiftPrivacyFeatures` 标记
/// （PRIVACY_JS / STRICT_PRIVACY_JS 注入时写入）+ 少量行为探测（GPC 属性、
/// RTCPeerConnection 补丁标记、hardwareConcurrency 归一值）。
const PRIVACY_SELFTEST_JS: &str = include_str!("../resources/js/privacy_selftest.js");

fn inject_extensions(
    app: &AppHandle,
    items: Vec<(Option<String>, Option<String>)>,
    webmcp_on: bool,
) {
    // 注意：主窗口是 Window 类型（tabs.rs 用 get_window("main")），
    // get_webview_window("main") 会返回 None 导致注入被跳过
    let Some(window) = app.get_window("main") else {
        log::info!("[extensions] 主窗口不可用，跳过注入");
        return;
    };
    let webviews = window.webviews();
    log::info!("[extensions] 注入到 {} 个 webview", webviews.len());
    let items_owned = items.clone();
    // 用户脚本（阶段3.2）：读注入缓存（同步，不碰 DB/runtime——inject 可能跑在 tokio 上下文里，
    // block_on 会 panic "Cannot start a runtime from within a runtime"）。
    // 缓存由用户脚本 CRUD 后 + 启动时刷新。
    let user_scripts: Vec<(String, Vec<String>)> = crate::db::userscripts::cached_scripts();
    log::info!("[extensions] 用户脚本缓存 {} 个", user_scripts.len());
    // 翻译原生桥要 AppHandle（闭包需 'static，不能借用 app，故闭包外先克隆）
    let app_for_bridge_all = app.clone();
    let _ = app.run_on_main_thread(move || {
        // 引擎层内容过滤器：如有规则变更，在主线程编译（编译完成会回来触发一次重新注入）
        crate::engine::content_filter::compile_pending();
        // 只注入 child 标签页（已跳过主 webview），child 都是 http/https 外部网页。
        // allow_list 必须用 ["*"]（匹配所有页面）：空数组 &[] 会被 WebKit 当作"空匹配列表"，
        // 一个页面都不注入（UserScript 从此从未生效——2026-09-19 事故根因，广告拦截/隐私/翻译全失效）。
        // 不能用 "http://*" 这类格式（WebKitGTK 通配符识别有坑，曾导致不注入）。
        let allow_list: &[&str] = &["*"];
        for wv in &webviews {
            // 跳过主 webview（label="main"，是 Vue UI），只注入 child 标签页（label="tab-*"）
            // remove_all_scripts 会误删 Tauri 主 webview 的 UserScript（Vue UI 桥接），导致界面空白
            if wv.label() == "main" {
                continue;
            }
            let items_clone = items_owned.clone();
            let user_scripts_clone = user_scripts.clone();
            let app_for_bridge = app_for_bridge_all.clone();
            let label_for_bridge = wv.label().to_string();
            let _ = wv.with_webview(move |pwv| {
                let webview = pwv.inner();
                let Some(manager) = webview.user_content_manager() else {
                    return;
                };
                crate::engine::content_filter::apply_to_manager(&manager);
                // 翻译原生桥（CSP 免疫 · 2026-09-21）：注册脚本消息处理器。
                // 站点 CSP 的 img-src 会拦 `img.src='shift://__translate__/…'`，故翻译脚本改走
                // 原生消息（不受 CSP 约束）。按 webview label 去重，避免重复连接累积。
                {
                    use std::collections::HashSet;
                    use std::sync::{Mutex, OnceLock};
                    static BRIDGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
                    let first = BRIDGED
                        .get_or_init(|| Mutex::new(HashSet::new()))
                        .lock()
                        .map(|mut s| s.insert(label_for_bridge.clone()))
                        .unwrap_or(false);
                    if first && manager.register_script_message_handler("shiftTranslate") {
                        let app_msg = app_for_bridge.clone();
                        let label_msg = label_for_bridge.clone();
                        manager.connect_script_message_received(
                            Some("shiftTranslate"),
                            move |_mgr, res| {
                                if let Some(v) = res.js_value() {
                                    let raw = v.to_string();
                                    crate::proto_shift::native_translate_handle(&app_msg, &label_msg, &raw);
                                }
                            },
                        );
                        log::info!("[translate-native] 原生桥已注册: {}", label_for_bridge);
                    }
                }
                manager.remove_all_scripts();
                manager.remove_all_style_sheets();
                // 选区缓存监听：**常驻** document_start UserScript（所有 frame、
                // 所有页面）。必须紧跟 remove_all_scripts 重新登记，否则下一次
                // re_inject 又会把它清掉——这正是「翻译选中」首击读不到选区的根因。
                // 当前已加载的页面 UserScript 追不生效，下面立刻 eval 补一次（幂等）。
                let selwatch = UserScript::new(
                    SELWATCH_JS,
                    UserContentInjectedFrames::AllFrames,
                    UserScriptInjectionTime::Start,
                    allow_list,
                    &[],
                );
                manager.add_script(&selwatch);
                crate::engine::webview::eval_js(&webview, SELWATCH_JS);
                // WebMCP polyfill 试验（阶段 1.5）：document_start 注入（网站脚本
                // feature-detect 前），捕获 registerTool 声明。可开关，默认关。
                if webmcp_on {
                    let polyfill = UserScript::new(
                        crate::engine::webmcp::WEBMCP_POLYFILL_JS,
                        UserContentInjectedFrames::AllFrames,
                        UserScriptInjectionTime::Start,
                        allow_list,
                        &[],
                    );
                    manager.add_script(&polyfill);
                    // 即时注入：已加载的标签立即生效（幂等，重复无害）
                    crate::engine::webview::eval_js(&webview, crate::engine::webmcp::WEBMCP_POLYFILL_JS);
                }
                // 用户脚本（阶段3.2）：按各自 matches 限定页面（空则全站）；与扩展脚本同一套机制
                for (code, patterns) in &user_scripts_clone {
                    let allow: Vec<&str> = if patterns.is_empty() {
                        vec!["*"]
                    } else {
                        patterns.iter().map(|s| s.as_str()).collect()
                    };
                    let script = UserScript::new(
                        code,
                        UserContentInjectedFrames::AllFrames,
                        UserScriptInjectionTime::End,
                        &allow,
                        &[],
                    );
                    manager.add_script(&script);
                    // 即时注入：已加载的标签立即生效（脚本自带幂等保护）
                    crate::engine::webview::eval_js(&webview, code);
                }
                for (js, css) in &items_clone {
                    if let Some(js) = js {
                        let script = UserScript::new(
                            js,
                            UserContentInjectedFrames::AllFrames,
                            UserScriptInjectionTime::End,
                            allow_list,
                            &[],
                        );
                        manager.add_script(&script);
                        // 即时注入：UserScript 只对"之后导航"的页面生效，已加载的标签不会
                        // 自动执行脚本（右键翻译菜单等注入因此对老标签无效）。这里对当前
                        // 页面再 evaluate 一次，让已打开的标签也能立即生效。
                        // 脚本自带幂等保护（__shiftPrivacyInstalled / __shiftTranslateMenu），
                        // 页面加载时已执行过则直接 return，重复注入无害。
                        // 引擎层统一封装（失败仅 debug：页面未就绪时属正常，UserScript 兜底）
                        crate::engine::webview::eval_js(&webview, js);
                    }
                    if let Some(css) = css {
                        let sheet = UserStyleSheet::new(
                            css,
                            UserContentInjectedFrames::AllFrames,
                            UserStyleLevel::User,
                            allow_list,
                            &[],
                        );
                        manager.add_style_sheet(&sheet);
                    }
                }
            });
        }
        log::info!("[extensions] 注入完成: {} 个扩展", items.len());
    });
}


/// 保存加噪白名单（域名数组）并立即重新注入
#[tauri::command]


/// 启动时注入所有启用扩展（在 setup 阶段调用，db 已就绪）
pub fn inject_on_startup(app: &AppHandle, db: &Db) {
    let (items, webmcp_on) = match tauri::async_runtime::block_on(adblock::collect_enabled_items(db)) {
        Ok(v) => v,
        Err(e) => {
            log::error!("[extensions] 启动注入失败: {}", e);
            return;
        }
    };
    if !items.is_empty() {
        log::info!("[extensions] 启动注入 {} 个启用扩展", items.len());
        inject_extensions(app, items, webmcp_on);
    }
}

/// 重新注入全部启用扩展 + 广告拦截（新标签创建后调用，确保新 webview 也有扩展）
pub async fn re_inject(app: &AppHandle) {
    let db = app.state::<Db>();
    match adblock::collect_enabled_items(&db).await {
        Ok((items, webmcp_on)) => {
            log::info!("[extensions] re_inject: {} 个注入项", items.len());
            inject_extensions(app, items.clone(), webmcp_on);
            // 延迟重试 evaluate：页面加载中 evaluate 会失败（document 未就绪）。
            // page_load_finished 事件对部分页面（如 understandingwar.org）不触发，
            // 所以不能只靠 Finished 补注入——这里 1s/3s/8s 重试，页面加载完必然命中。
            // 脚本自带幂等保护，重复 evaluate 无害。
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                for delay in [1u64, 3, 8] {
                    tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
                    evaluate_inject_retry(&app2, items.clone());
                }
            });
        }
        Err(e) => log::error!("[extensions] re_inject 失败: {}", e),
    }
}

/// 延迟重试：对所有 child webview evaluate 注入脚本 + 探针（确认脚本是否执行）
fn evaluate_inject_retry(app: &AppHandle, items: Vec<(Option<String>, Option<String>)>) {
    let Some(window) = app.get_window("main") else {
        return;
    };
    let webviews = window.webviews();
    let items_owned = items.clone();
    let _ = app.run_on_main_thread(move || {
        for wv in &webviews {
            if wv.label() == "main" {
                continue;
            }
            let items_clone = items_owned.clone();
            let _ = wv.with_webview(move |pwv| {
                let webview = pwv.inner();
                for (js, _css) in &items_clone {
                    if let Some(js) = js {
                        crate::engine::webview::eval_js(&webview, js);
                    }
                }
                // 探针：确认翻译菜单脚本是否已在页面执行（UserScript 生效与否立见分晓）
                crate::engine::webview::eval_js_with(
                    &webview,
                    "(function(){return 'probe: tr='+(typeof window.__shiftTranslateMenu)+' proto='+location.protocol+' ready='+document.readyState;})()",
                    move |r| match r {
                        Ok(v) => log::info!("[extensions] 延迟探针: {}", v),
                        Err(e) => log::info!("[extensions] 延迟探针失败（页面未就绪）: {}", e),
                    },
                );
            });
        }
    });
}

/// 页面加载完成后对单个 webview 即时注入（on_page_load Finished 时调用）
///
/// 背景：UserScript 只对"之后导航"的页面生效；若 re_inject 发生在页面加载中途
/// （创建标签时页面刚起步），已加载页面不会执行脚本，右键翻译菜单等注入就失效了。
/// 页面加载完成时 document 已就绪，evaluate 必然成功，这里补上即时注入。
/// 各脚本自带幂等保护（__shiftPrivacyInstalled / __shiftTranslateMenu / __shiftAdblockInstalled），
/// 与 UserScript 重复执行无害。
pub async fn inject_to_webview(app: &AppHandle, label: &str) {
    let db = app.state::<Db>();
    let (items, _webmcp_on) = match adblock::collect_enabled_items(&db).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("[extensions] inject_to_webview 收集注入项失败: {}", e);
            return;
        }
    };
    let Some(window) = app.get_window("main") else {
        return;
    };
    let Some(wv) = window.get_webview(label) else {
        log::debug!("[extensions] inject_to_webview: webview {} 不存在", label);
        return;
    };
    let _ = wv.with_webview(move |pwv| {
        let webview = pwv.inner();
        for (js, _css) in &items {
            if let Some(js) = js {
                let js_len = js.len();
                crate::engine::webview::eval_js_with(&webview, js, move |r| match r {
                    Ok(_) => {
                        log::info!("[extensions] 页面加载后即时注入成功 ({} 字节)", js_len)
                    }
                    Err(e) => log::info!("[extensions] 页面加载后即时注入失败: {}", e),
                });
            }
        }
        // 诊断探针：确认翻译菜单脚本是否真的在页面里执行了
        crate::engine::webview::eval_js_with(
            &webview,
            "(function(){return 'probe: __shiftTranslateMenu='+(typeof window.__shiftTranslateMenu)+' proto='+location.protocol;})()",
            move |r| match r {
                Ok(v) => log::info!("[extensions] 注入探针: {}", v),
                Err(e) => log::info!("[extensions] 注入探针失败: {}", e),
            },
        );
    });
}

#[cfg(test)]
#[path = "extensions/tests.rs"]
mod tests;

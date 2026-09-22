// 扩展管理（WebKitGTK UserContentManager 用户脚本/样式注入）
// WebKitGTK 不支持真正的 Chrome 扩展 API，MVP 方案：
// 扩展 = 目录（含 manifest.json + content.js/content.css），
// 注入到所有 http/https 页面（allow_list 限定，不污染 tauri://localhost 的 Vue UI）
// P1-3 拆分（2026-09-22）：广告拦截独立成子模块
#[path = "extensions/adblock.rs"]
pub mod adblock;
pub use adblock::*;

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
pub struct PrivacyStats {
    pub cleaned_params: u64,
    pub privacy_injections: u64,
    pub adblock_injections: u64,
}

/// 读取隐私防护统计
#[tauri::command]
pub fn get_privacy_stats() -> PrivacyStats {
    PrivacyStats {
        cleaned_params: CLEANED_PARAMS.load(Ordering::Relaxed),
        privacy_injections: PRIVACY_INJECTIONS.load(Ordering::Relaxed),
        adblock_injections: ADBLOCK_INJECTIONS.load(Ordering::Relaxed),
    }
}

/// 重置隐私防护统计
#[tauri::command]
pub fn reset_privacy_stats() {
    CLEANED_PARAMS.store(0, Ordering::Relaxed);
    PRIVACY_INJECTIONS.store(0, Ordering::Relaxed);
    ADBLOCK_INJECTIONS.store(0, Ordering::Relaxed);
    log::info!("[privacy] 统计已重置");
}

// ---- 隐私防护自测（阶段 2「信任需要证明」）----

/// 单项自测结果
#[derive(Debug, Serialize, Clone)]
pub struct PrivacyCheck {
    /// 检查项标识（inject/gpc/canvas/...）
    pub id: String,
    /// 中文名
    pub label: String,
    /// 结果：pass / fail / skip
    pub status: String,
    /// 说明
    pub detail: String,
}

/// 隐私防护自测报告
#[derive(Debug, Serialize, Clone)]
pub struct PrivacySelfTestReport {
    /// 当前档位：off / standard / strict
    pub shield_level: String,
    /// 反指纹模式：noise（加噪）/ uniform（统一化）
    pub fp_mode: String,
    /// 活动标签域名
    pub hostname: String,
    /// 该域名是否在加噪白名单里
    pub whitelisted: bool,
    /// 检查项总数
    pub total: usize,
    /// 通过数
    pub passed: usize,
    /// 逐项结果
    pub checks: Vec<PrivacyCheck>,
}

fn mk_check(id: &str, label: &str, status: &str, detail: &str) -> PrivacyCheck {
    PrivacyCheck {
        id: id.to_string(),
        label: label.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
    }
}

/// 域名是否命中加噪白名单（与 PRIVACY_JS 内 NOISE_ON 判定同规则）
fn is_whitelisted(hostname: &str, whitelist: &[String]) -> bool {
    let h = hostname.trim().to_lowercase();
    if h.is_empty() {
        return false;
    }
    whitelist.iter().any(|w| {
        let d = w
            .to_lowercase()
            .trim_start_matches("*.")
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or("")
            .split(':')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if d.is_empty() {
            return false;
        }
        h == d || h.ends_with(&format!(".{}", d))
    })
}

/// 由页面自测 JSON + 后端信息组装报告（纯函数，便于单测）。
pub fn build_privacy_selftest(
    level: &str,
    hostname: &str,
    whitelisted: bool,
    page_json: &str,
) -> PrivacySelfTestReport {
    let v: serde_json::Value =
        serde_json::from_str(page_json).unwrap_or(serde_json::Value::Null);
    let installed = v["installed"].as_bool().unwrap_or(false);
    let feat = &v["features"];
    let has = |k: &str| feat[k].as_bool().unwrap_or(false);
    // 非网页标签（如 tauri:// 新标签页）不会注入防护脚本，此时统一标 skip
    // 而非 fail——避免"新标签页自测一片红"的误导。
    let non_web = {
        let p = v["protocol"].as_str().unwrap_or("");
        !p.is_empty() && !p.starts_with("http")
    };
    let off = level == "off" || non_web;
    let skip_reason = if non_web {
        "当前标签不是网页（请先打开一个网页再自测）"
    } else {
        "隐私防护未开启"
    };

    let mut checks: Vec<PrivacyCheck> = Vec::new();

    // 1) 脚本注入
    if off {
        checks.push(mk_check("inject", "防护脚本注入", "skip", skip_reason));
    } else if installed {
        checks.push(mk_check("inject", "防护脚本注入", "pass", "防护脚本已在页面生效"));
    } else {
        checks.push(mk_check(
            "inject",
            "防护脚本注入",
            "fail",
            "未检测到防护脚本（页面可能未注入或已跳转）",
        ));
    }

    // 2) GPC 信号
    if off {
        checks.push(mk_check("gpc", "GPC 隐私信号", "skip", skip_reason));
    } else if has("gpc") {
        checks.push(mk_check(
            "gpc",
            "GPC 隐私信号",
            "pass",
            "navigator.globalPrivacyControl = true",
        ));
    } else {
        checks.push(mk_check("gpc", "GPC 隐私信号", "fail", "GPC 信号未生效"));
    }

    // 3-5) 加噪类（白名单站点跳过）
    for (id, label) in [
        ("canvas", "Canvas 指纹加噪"),
        ("webgl", "WebGL 参数归一化"),
        ("audio", "AudioContext 加噪"),
    ] {
        if off {
            checks.push(mk_check(id, label, "skip", skip_reason));
        } else if whitelisted {
            checks.push(mk_check(id, label, "skip", "当前站点在白名单，已跳过加噪"));
        } else if has(id) {
            checks.push(mk_check(id, label, "pass", "已启用"));
        } else {
            checks.push(mk_check(id, label, "fail", "未生效"));
        }
    }

    // 6-9) 常开归一化类
    for (id, label) in [
        ("hardware", "硬件信息归一化"),
        ("webrtc", "WebRTC 内网 IP 防泄露"),
        ("webgpu", "WebGPU 指纹防护"),
        ("misc", "杂项指纹归一化"),
    ] {
        if off {
            checks.push(mk_check(id, label, "skip", skip_reason));
        } else if has(id) {
            checks.push(mk_check(id, label, "pass", "已启用"));
        } else {
            checks.push(mk_check(id, label, "fail", "未生效"));
        }
    }

    let total = checks.len();
    let passed = checks.iter().filter(|c| c.status == "pass").count();
    PrivacySelfTestReport {
        shield_level: level.to_string(),
        fp_mode: feat["fp_mode"].as_str().unwrap_or("noise").to_string(),
        hostname: hostname.to_string(),
        whitelisted,
        total,
        passed,
        checks,
    }
}

/// 隐私防护自测：在活动标签页核对各项防护是否真的生效（阶段 2「信任需要证明」）。
#[tauri::command]
pub async fn privacy_selftest(
    app: AppHandle,
    state: State<'_, crate::tabs::TabManager>,
) -> Result<PrivacySelfTestReport, String> {
    let db = app.state::<Db>();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE key IN \
         ('privacy_shield_level','privacy_shield','privacy_whitelist')",
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let kv: std::collections::HashMap<String, String> = rows.into_iter().collect();
    let level = match kv.get("privacy_shield_level") {
        Some(l) => l.clone(),
        None => match kv.get("privacy_shield").map(|s| s.as_str()) {
            Some("false") => "off".to_string(),
            _ => "standard".to_string(),
        },
    };
    let whitelist: Vec<String> = kv
        .get("privacy_whitelist")
        .and_then(|v| serde_json::from_str(v).ok())
        .unwrap_or_default();

    let id = state
        .runtime_snapshot()
        .1
        .ok_or_else(|| "没有活动标签".to_string())?;
    let window = app
        .get_window("main")
        .ok_or_else(|| "主窗口不存在".to_string())?;
    let label = format!("tab-{}", id);
    let wv = window
        .get_webview(&label)
        .ok_or_else(|| "活动标签的 webview 不存在".to_string())?;

    // 发起在 with_webview 内、等待在闭包外——避免主线程死锁
    let (tx, rx) = std::sync::mpsc::channel();
    wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), PRIVACY_SELFTEST_JS, move |res| {
            let _ = tx.send(res);
        });
    })
    .map_err(|e| format!("自测脚本执行失败: {}", e))?;
    let raw = rx
        .recv_timeout(std::time::Duration::from_secs(6))
        .map_err(|_| "自测超时（页面无响应）".to_string())?
        .map_err(|e| format!("自测脚本执行失败: {}", e))?;

    let v: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
    let hostname = v["hostname"].as_str().unwrap_or("").to_string();
    let whitelisted = is_whitelisted(&hostname, &whitelist);

    Ok(build_privacy_selftest(&level, &hostname, whitelisted, &raw))
}

#[cfg(target_os = "linux")]
use webkit2gtk::{
    UserContentInjectedFrames, UserContentManagerExt, UserScript, UserScriptInjectionTime,
    UserStyleLevel, UserStyleSheet, WebViewExt,
};

/// 返回给前端的数据（含注入内容摘要，便于显示）
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ExtensionInfo {
    pub id: i64,
    pub name: String,
    pub path: String,
    pub enabled: bool,
    pub version: Option<String>,
    pub description: Option<String>,
    pub js_count: usize,
    pub css_count: usize,
    pub created_at: String,
}

#[derive(Deserialize)]
struct Manifest {
    name: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    content_scripts: Vec<ContentScript>,
}

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

/// 读取扩展目录内容
fn read_extension_dir(path: &str) -> Result<ExtensionDirResult, String> {
    // path 可以是目录（含 manifest.json）或 manifest.json 文件本身
    let manifest_path = if path.ends_with("manifest.json") {
        path.to_string()
    } else {
        format!("{}/manifest.json", path)
    };
    let manifest_str = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("读取 manifest.json 失败: {}", e))?;
    let manifest: Manifest = serde_json::from_str(&manifest_str)
        .map_err(|e| format!("解析 manifest.json 失败: {}", e))?;

    // 扩展目录 = manifest.json 所在目录
    let base_dir = match manifest_path.rfind('/') {
        Some(idx) => manifest_path[..idx].to_string(),
        None => ".".to_string(),
    };

    let mut js_sources: Vec<String> = Vec::new();
    let mut css_sources: Vec<String> = Vec::new();
    // 安全：manifest 里的脚本/样式路径必须解析后仍落在扩展目录内，
    // 防止恶意/被篡改的 manifest 声明 "../../../.ssh/id_rsa" 之类路径越界读任意文件。
    //
    // 两道防线：
    //  1) 词法：拒绝含 ".."（ParentDir）组件的路径；Path::starts_with 是纯词法比较、
    //     不解析 ".."，若只靠 starts_with，声明 "../../../home/gao/.ssh/id_rsa" 可绕过越界读取。
    //  2) 物理（symlink 兜底）：文件真实存在时做 canonicalize，解析符号链接后
    //     再校验「真实路径」仍落在「真实 base」内——防止扩展包在目录里埋一个
    //     symlink 指向 /home/gao/.ssh，再让 manifest 引用该链接名间接越界。
    //     文件不存在（如首次注入尚未生成）时退回到已通过词法校验的相对路径，
    //     因为相对路径 + 无 ".." + 以 base 开头，词法上已确定无法越界。
    let base = std::path::Path::new(&base_dir);
    // 真实 base：解析 base 自身可能的 symlink（防御扩展目录本身是链接指向外部的场景）
    let base_real = base.canonicalize().unwrap_or_else(|_| base.to_owned());
    for cs in &manifest.content_scripts {
        for js in &cs.js {
            let p = base.join(js);
            // 拒绝含 ".." 组件的路径
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("脚本路径含 ..（拒绝）: {}", js));
            }
            // 词法越界
            if !p.starts_with(base) {
                return Err(format!("脚本路径越界（拒绝）: {}", js));
            }
            // symlink 兜底：解析真实路径后再校验一次
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("脚本路径经符号链接越界（拒绝）: {}", js));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取脚本 {} 失败: {}", js, e))?;
            js_sources.push(content);
        }
        for css in &cs.css {
            let p = base.join(css);
            if p.components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!("样式路径含 ..（拒绝）: {}", css));
            }
            if !p.starts_with(base) {
                return Err(format!("样式路径越界（拒绝）: {}", css));
            }
            // symlink 兜底（与 js 分支保持一致）：解析真实路径后校验仍落在真实 base 内
            if let Ok(p_real) = p.canonicalize() {
                if !p_real.starts_with(&base_real) {
                    return Err(format!("样式路径经符号链接越界（拒绝）: {}", css));
                }
            }
            let content =
                std::fs::read_to_string(&p).map_err(|e| format!("读取样式 {} 失败: {}", css, e))?;
            css_sources.push(content);
        }
    }

    let js = if js_sources.is_empty() {
        None
    } else {
        Some(js_sources.join("\n;\n"))
    };
    let css = if css_sources.is_empty() {
        None
    } else {
        Some(css_sources.join("\n"))
    };

    Ok((
        manifest.name,
        manifest.version,
        manifest.description,
        js,
        css,
        js_sources.len(),
        css_sources.len(),
    ))
}

/// 右键翻译菜单脚本（编译期嵌入）
/// 复用 shift://__translate__ 协议（translate.rs + whole_page_translate.js），
/// 补上一直缺失的 UI 入口：选中文字右键 → 翻译选中/翻译整页。
const CONTEXT_TRANSLATE_JS: &str = include_str!("context_translate.js");

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

/// 在主线程把所有 child webview 的 UserContentManager 清空并注入扩展
/// UserContentManager 是 webview 级（非 WebContext 级），必须逐个 webview 注入
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
                                    crate::native_translate_handle(&app_msg, &label_msg, &raw);
                                }
                            },
                        );
                        log::info!("[translate-native] 原生桥已注册: {}", label_for_bridge);
                    }
                }
                manager.remove_all_scripts();
                manager.remove_all_style_sheets();
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
pub async fn set_privacy_whitelist(
    db: State<'_, Db>,
    app: AppHandle,
    domains: Vec<String>,
) -> Result<(), String> {
    // 清洗：去空、去协议/路径、去重、限长
    let mut cleaned: Vec<String> = Vec::new();
    for d in domains {
        let s = d
            .trim()
            .to_lowercase()
            .replace("https://", "")
            .replace("http://", "");
        let s = s.split('/').next().unwrap_or("").trim().to_string();
        if s.is_empty() {
            continue;
        }
        // 简单域名格式校验（含点且无空格）
        if !s.contains('.') || s.contains(' ') {
            continue;
        }
        if !cleaned.iter().any(|x| x == &s) {
            cleaned.push(s);
        }
    }
    log::info!(
        "[extensions] set_privacy_whitelist: {} 个域名",
        cleaned.len()
    );
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_whitelist', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 读取 WebMCP polyfill 试验开关（阶段 1.5，默认关）
#[tauri::command]
pub async fn get_webmcp_polyfill(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'webmcp_polyfill'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|(v,)| v == "true").unwrap_or(false))
}

/// 设置 WebMCP polyfill 试验开关（存设置 + 立即重新注入）
#[tauri::command]
pub async fn set_webmcp_polyfill(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_webmcp_polyfill: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('webmcp_polyfill', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(if enabled { "true" } else { "false" })
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 读取加噪白名单
#[tauri::command]
pub async fn get_privacy_whitelist(db: State<'_, Db>) -> Result<Vec<String>, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_whitelist'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row
        .and_then(|(v,)| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default())
}

/// 隐私防护档位设置（off / standard / strict），存设置 + 立即重新注入
#[tauri::command]
pub async fn set_privacy_shield_level(
    db: State<'_, Db>,
    app: AppHandle,
    level: String,
) -> Result<(), String> {
    let level = match level.as_str() {
        "off" | "standard" | "strict" => level,
        other => return Err(format!("未知的隐私防护档位: {}", other)),
    };
    log::info!("[extensions] set_privacy_shield_level: {}", level);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield_level', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&level)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    // 同步旧布尔键，避免两处状态不一致
    let bool_val = if level == "off" { "false" } else { "true" };
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(bool_val)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 隐私防护档位查询（默认 standard）
#[tauri::command]
pub async fn get_privacy_shield_level(db: State<'_, Db>) -> Result<String, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield_level'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    if let Some((l,)) = row {
        return Ok(l);
    }
    let srow: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(match srow.map(|r| r.0).as_deref() {
        Some("false") => "off".to_string(),
        _ => "standard".to_string(),
    })
}

/// 归一化反指纹模式（阶段 2）：只接受 noise / uniform，其余（含 None）回落 noise。
pub fn normalize_fp_mode(v: Option<&str>) -> &'static str {
    match v {
        Some("uniform") => "uniform",
        _ => "noise",
    }
}

/// 反指纹模式设置（noise 加噪 / uniform 统一化），存设置 + 立即重新注入
#[tauri::command]
pub async fn set_privacy_fp_mode(
    db: State<'_, Db>,
    app: AppHandle,
    mode: String,
) -> Result<(), String> {
    let mode = match mode.as_str() {
        "noise" | "uniform" => mode,
        other => return Err(format!("未知的反指纹模式: {}", other)),
    };
    log::info!("[extensions] set_privacy_fp_mode: {}", mode);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_fp_mode', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&mode)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 反指纹模式查询（默认 noise）
#[tauri::command]
pub async fn get_privacy_fp_mode(db: State<'_, Db>) -> Result<String, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_fp_mode'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(normalize_fp_mode(row.map(|r| r.0).as_deref()).to_string())
}

/// 隐私防护开关（存设置 + 立即重新注入）
#[tauri::command]
pub async fn set_privacy_shield_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_privacy_shield_enabled: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('privacy_shield', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 隐私防护当前状态（前端初始化用；默认开启）
#[tauri::command]
pub async fn get_privacy_shield_enabled(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'privacy_shield'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0 != "false").unwrap_or(true))
}

/// 广告拦截开关（存设置 + 重新注入）
#[tauri::command]
pub async fn set_adblock_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    enabled: bool,
) -> Result<(), String> {
    log::info!("[extensions] set_adblock_enabled: {}", enabled);
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_enabled', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(enabled.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 广告拦截当前状态（给前端初始化用）
#[tauri::command]
pub async fn get_adblock_enabled(db: State<'_, Db>) -> Result<bool, String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_enabled'")
            .fetch_optional(&db.pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(row.map(|r| r.0 == "true").unwrap_or(true))
}

/// 读取用户自定义广告关键词（不含内置）
#[tauri::command]
pub async fn get_adblock_keywords(db: State<'_, Db>) -> Result<Vec<String>, String> {
    Ok(adblock::load_custom_keywords(&db).await)
}

/// 保存用户自定义广告关键词并重新注入（立刻对已开标签生效，刷新页面后看到效果）
#[tauri::command]
pub async fn set_adblock_keywords(
    db: State<'_, Db>,
    app: AppHandle,
    keywords: Vec<String>,
) -> Result<(), String> {
    let cleaned: Vec<String> = keywords
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let json = serde_json::to_string(&cleaned).map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] set_adblock_keywords: {} 个自定义词",
        cleaned.len()
    );
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES ('adblock_keywords', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(&json)
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 加载扩展（目录或 manifest.json 路径）
#[tauri::command]
pub async fn load_extension(
    db: State<'_, Db>,
    app: AppHandle,
    path: String,
) -> Result<ExtensionInfo, String> {
    log::info!("[extensions] load_extension: path={}", path);
    let (name, version, description, js, css, js_count, css_count) = read_extension_dir(&path)?;
    let id = ext_db::create(&db, &name, &path)
        .await
        .map_err(|e| e.to_string())?;
    log::info!(
        "[extensions] 已加载扩展: id={} name={} js={} css={}",
        id,
        name,
        js.as_ref().map(|s| s.len()).unwrap_or(0),
        css.as_ref().map(|s| s.len()).unwrap_or(0)
    );
    // 注入（重新注入全部启用扩展，保持一致性）
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(ExtensionInfo {
        id,
        name,
        path,
        enabled: true,
        version,
        description,
        js_count,
        css_count,
        created_at: String::new(),
    })
}

/// 扩展列表
#[tauri::command]
pub async fn list_extensions(db: State<'_, Db>) -> Result<Vec<ExtensionInfo>, String> {
    let exts = ext_db::list(&db).await.map_err(|e| e.to_string())?;
    log::info!("[extensions] list_extensions: {} 条", exts.len());
    let mut infos = Vec::new();
    for ext in exts {
        let (version, description, js_count, css_count) = match read_extension_dir(&ext.path) {
            Ok((_, v, d, _, _, jc, cc)) => (v, d, jc, cc),
            Err(e) => {
                log::warn!("[extensions] 读取扩展 {} 失败: {}", ext.name, e);
                (None, None, 0, 0)
            }
        };
        infos.push(ExtensionInfo {
            id: ext.id,
            name: ext.name,
            path: ext.path,
            enabled: ext.enabled,
            version,
            description,
            js_count,
            css_count,
            created_at: ext.created_at,
        });
    }
    Ok(infos)
}

/// 启用/禁用扩展（重新注入）
#[tauri::command]
pub async fn set_extension_enabled(
    db: State<'_, Db>,
    app: AppHandle,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    log::info!(
        "[extensions] set_extension_enabled: id={} enabled={}",
        id,
        enabled
    );
    ext_db::set_enabled(&db, id, enabled)
        .await
        .map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

/// 删除扩展（重新注入）
#[tauri::command]
pub async fn delete_extension(db: State<'_, Db>, app: AppHandle, id: i64) -> Result<(), String> {
    log::info!("[extensions] delete_extension: id={}", id);
    ext_db::delete(&db, id).await.map_err(|e| e.to_string())?;
    let (items, webmcp_on) = adblock::collect_enabled_items(&db).await?;
    inject_extensions(&app, items, webmcp_on);
    Ok(())
}

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
mod tests {
    use super::*;

    #[test]
    fn build_js_includes_builtin_and_custom() {
        let js = adblock::build_adblock_js(&["自定义词".to_string()]);
        assert!(js.contains("'关闭广告'"), "应包含内置词");
        assert!(js.contains("'自定义词'"), "应包含自定义词");
        assert!(!js.contains("__SB_KEYWORDS__"), "模板占位符应被替换");
    }

    #[test]
    fn build_js_escapes_quotes_and_dedups() {
        let js = adblock::build_adblock_js(&["it's".to_string(), "关闭广告".to_string()]);
        assert!(js.contains(r"'it\'s'"), "单引号应转义");
        assert_eq!(js.matches("'关闭广告'").count(), 1, "内置词不应重复");
    }

    #[test]
    fn privacy_selftest_standard_all_pass() {
        let page = r#"{"installed":true,"features":{"gpc":true,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":true,"misc":true}}"#;
        let r = build_privacy_selftest("standard", "example.com", false, page);
        assert_eq!(r.total, 9);
        assert_eq!(r.passed, 9, "标准档全开应全过");
        assert!(r.checks.iter().all(|c| c.status == "pass"));
    }

    #[test]
    fn privacy_selftest_whitelist_skips_noise_only() {
        let page = r#"{"installed":true,"features":{"gpc":true,"canvas":false,"webgl":false,"audio":false,"hardware":true,"webrtc":true,"webgpu":true,"misc":true}}"#;
        let r = build_privacy_selftest("standard", "example.com", true, page);
        assert_eq!(r.passed, 6, "白名单下加噪三项跳过，其余 6 项通过");
        let skip: Vec<&str> = r
            .checks
            .iter()
            .filter(|c| c.status == "skip")
            .map(|c| c.id.as_str())
            .collect();
        assert_eq!(skip, vec!["canvas", "webgl", "audio"]);
    }

    #[test]
    fn privacy_selftest_off_all_skip() {
        let page = r#"{"installed":false,"features":{}}"#;
        let r = build_privacy_selftest("off", "example.com", false, page);
        assert!(r.checks.iter().all(|c| c.status == "skip"), "关闭档应全部跳过");
        assert_eq!(r.passed, 0);
    }

    #[test]
    fn privacy_selftest_non_web_all_skip() {
        let page = r#"{"installed":false,"features":{},"protocol":"tauri:"}"#;
        let r = build_privacy_selftest("standard", "", false, page);
        assert!(
            r.checks.iter().all(|c| c.status == "skip"),
            "非网页标签（tauri://）应全部跳过，不误报失败"
        );
        assert_eq!(r.passed, 0);
    }

    #[test]
    fn whitelist_matches_domain_and_subdomain() {
        let wl = vec!["example.com".to_string(), "*.foo.cn".to_string()];
        assert!(is_whitelisted("example.com", &wl));
        assert!(is_whitelisted("www.example.com", &wl));
        assert!(is_whitelisted("a.b.foo.cn", &wl));
        assert!(!is_whitelisted("notexample.com", &wl));
        assert!(!is_whitelisted("", &wl));
    }

    #[test]
    fn normalize_fp_mode_rules() {
        assert_eq!(normalize_fp_mode(Some("uniform")), "uniform");
        assert_eq!(normalize_fp_mode(Some("noise")), "noise");
        // 非法值/缺失一律回落 noise（默认加噪）
        assert_eq!(normalize_fp_mode(Some("bogus")), "noise");
        assert_eq!(normalize_fp_mode(None), "noise");
    }
}

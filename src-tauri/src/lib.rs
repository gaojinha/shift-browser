mod agent_cmd;
mod ai;
mod audit;
mod content_cmd;
mod db;
mod default_browser;
mod engine;
mod extensions;
mod feed;
mod gpc;
mod auto_tasks;
mod api;
mod mcp_cmd;
mod netmon;
mod newtab;
mod perms;
mod reader;
mod selfcheck;
mod tabs;
mod tls;
mod translate;
mod tray;

use db::{
    bookmarks, downloads, history, passwords, sessions, settings, tab_groups, users, workspaces, Db,
};
use tabs::TabManager;
use tauri::Emitter;
use tauri::Manager;

/// shift:// 自定义协议页面的 CSP（主窗口/新标签页/世界导航页统一）
/// 注意：Tauri 的 security.csp 只注入 tauri:// 与 http://asset.localhost，
/// 自定义协议（shift://）不走注入，必须在这里手动加响应头。
/// script-src 允许 'unsafe-inline'：newtab/world-nav 是本地生成的可信内联脚本；
/// object-src 'none' + base-uri 'self' + connect-src 收紧 防注入面。
const SHIFT_CSP: &str = "default-src 'self' shift: data:; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https: http:; connect-src 'self' ipc: http://ipc.localhost https://wttr.in https://api.open-meteo.com https://ipinfo.io; font-src 'self' data:; media-src 'self' https: http:; frame-src 'self' https: http:; object-src 'none'; base-uri 'self'";





/// 整页翻译注入脚本（编译期嵌入，运行时 eval 到 child webview）
/// （命令实现见 tabs.rs 的 translate_current_page —— 那里可访问 TabManager 内部字段）
pub(crate) const WHOLE_PAGE_TRANSLATE_JS: &str = include_str!("whole_page_translate.js");

/// 隐私脱敏（2026-09-22）：日志里的 URL 只保留 scheme://host，
/// 去掉路径/查询/片段——URL 路径常含搜索词、文档名、token 等敏感信息。
pub fn redact_url(u: &str) -> String {
    match url::Url::parse(u) {
        Ok(p) => match p.host_str() {
            Some(h) => format!("{}://{}", p.scheme(), h),
            None => format!("{}://[内部]", p.scheme()),
        },
        Err(_) => "<非URL>".to_string(),
    }
}

/// 敏感文件权限加固（安全 · 2026-09-22）：Cookie 库/侧车 WAL 按 umask 落盘时
/// 常见 664（组/其他用户可读），会话 Cookie 泄露风险。应用数据目录虽为 0700 兜底，
/// 但纵深防御要求文件自身也为 0600。运行时 WebKit 可能重建这些文件，故需周期重扫。
#[cfg(unix)]
pub fn harden_sensitive_files(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    const SENSITIVE: &[&str] = &[
        "cookies",
        "cookies.sqlite",
        "cookies.bak",
        "cookies-wal",
        "cookies-shm",
        "cookies.sqlite-wal",
        "cookies.sqlite-shm",
        "shift-browser.db-wal",
        "shift-browser.db-shm",
    ];
    for name in SENSITIVE {
        let p = dir.join(name);
        if p.exists() {
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
    // 日志目录同样收紧（隐私：日志虽已脱敏，仍不应组/其他可读）
    let logs = dir.join("logs");
    if logs.exists() {
        let _ = std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700));
        if let Ok(rd) = std::fs::read_dir(&logs) {
            for e in rd.flatten() {
                let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

/// 启动锁屏中标志：setup 弹出 GTK 密码对话框期间为 true。
/// 前端 onMounted 在锁屏期间调用 Db/TabManager 命令会全部失败并弹"创建/加载标签页失败"
/// toast（反复重试），必须能查询到锁屏状态并静默等待 app-unlocked。
/// 用原子变量而非 Db/State：锁屏期间 Db 尚未 manage，命令参数不能依赖 State。
static LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// translate-progress 事件节流器：同一 webview 至少间隔 300ms 才转发一次，
/// 防止整页翻译高频上报刷爆 UI 事件通道（2026-09-19 安全修复）
static PROGRESS_THROTTLE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
> = std::sync::OnceLock::new();

/// 翻译请求速率限制器：同一 webview 每分钟最多 30 次翻译请求，
/// 防恶意网页批量驱动浏览器外发翻译（2026-09-19）
static TRANSLATE_RATE: std::sync::OnceLock<
    std::sync::Mutex<
        std::collections::HashMap<String, std::collections::VecDeque<std::time::Instant>>,
    >,
> = std::sync::OnceLock::new();

/// 返回 true = 超频应拒绝
fn translate_rate_limited(label: &str) -> bool {
    const MAX: usize = 30;
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
    let cell =
        TRANSLATE_RATE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = cell.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    let q = map
        .entry(label.to_string())
        .or_insert_with(std::collections::VecDeque::new);
    while let Some(&t) = q.front() {
        if now.duration_since(t) > WINDOW {
            q.pop_front();
        } else {
            break;
        }
    }
    if q.len() >= MAX {
        return true;
    }
    q.push_back(now);
    false
}

/// 查询是否处于启动锁屏状态（前端初始化前调用，不依赖 Db）
#[tauri::command]
fn is_locked() -> bool {
    LOCKED.load(std::sync::atomic::Ordering::SeqCst)
}

/// 前端日志桥：把前端关键节点的执行情况写进 Rust 日志，
/// 便于排查"前端白屏/事件收不到/初始化不执行"这类看不到 console 的问题。
#[tauri::command]
fn front_log(msg: String) {
    log::info!("[front] {}", msg);
}

/// 一键自检：汇总版本/存储/运行时/能力状态（只读，不修改任何状态）
#[tauri::command]
async fn selfcheck_cmd(app: tauri::AppHandle) -> Result<selfcheck::SelfCheckReport, String> {
    let report = selfcheck::run(&app).await;
    log::info!(
        "[selfcheck] 健康={} 版本项={} 存储项={} 运行时项={} 能力项={}",
        report.healthy,
        report.versions.len(),
        report.storage.len(),
        report.runtime.len(),
        report.capabilities.len()
    );
    // 关键环境事实写入日志（排查问题时第一手资料：版本/引擎/路径）
    let env_line: Vec<String> = report
        .versions
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 环境: {}", env_line.join(" | "));
    // 能力状态也记一行（过滤器是否就绪、拦截是否开启、隐私档位）
    let cap_line: Vec<String> = report
        .capabilities
        .iter()
        .map(|i| format!("{}={}", i.name, i.detail))
        .collect();
    log::info!("[selfcheck] 能力: {}", cap_line.join(" | "));
    for item in report
        .versions
        .iter()
        .chain(report.storage.iter())
        .chain(report.runtime.iter())
        .chain(report.capabilities.iter())
    {
        if !item.ok {
            log::warn!("[selfcheck] 异常项: {} - {}", item.name, item.detail);
        }
    }
    Ok(report)
}

/// 简易 percent-decode（处理 shift:// query 参数，避免引入 url crate）
/// 注意：只处理 %XX 和 + → 空格，满足 shift:// 内部用
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h1 = (bytes[i + 1] as char).to_digit(16);
                let h2 = (bytes[i + 2] as char).to_digit(16);
                if let (Some(a), Some(b)) = (h1, h2) {
                    out.push((a * 16 + b) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// 把翻译结果写到对应 webview 的 window.__shift_translate_result_<callback>
/// 菜单 JS 轮询这个变量拿到结果（shift:// Image 请求不读响应体，必须走 window 变量）
fn write_translate_result(
    app: &tauri::AppHandle,
    webview_label: &str,
    callback: &str,
    payload: &serde_json::Value,
) {
    // P2：callback 白名单双保险（入口已校验，这里再挡一道）——
    // callback 被拼进 JS 变量名 eval 到特权 webview，恶意 callback 可注入任意 JS
    if !valid_callback(callback) {
        log::warn!("[translate] 非法 callback，拒绝写入: {:?}", callback);
        return;
    }
    let Some(wv) = app.get_webview(webview_label) else {
        log::warn!("[translate] 找不到 webview: {}", webview_label);
        return;
    };
    // 序列化为 JS 表达式：window.__shift_translate_result_<cb> = <json>;
    let json_str = payload.to_string();
    let js = format!(
        "window.__shift_translate_result_{} = {};",
        callback, json_str
    );
    if let Err(e) = wv.eval(&js) {
        log::warn!("[translate] eval 结果失败: {}", e);
    } else {
        log::debug!("[translate] 已写入结果 callback={}", callback);
    }
}

/// P2：callback 参数白名单——只允许 [A-Za-z0-9_]，长度 1~64。
/// callback 会被拼进 JS 变量名并 eval 到 webview，必须严格白名单，
/// 否则恶意页面可通过 shift://__translate__ 端点注入任意 JS
fn valid_callback(cb: &str) -> bool {
    !cb.is_empty() && cb.len() <= 64 && cb.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// 原生桥翻译处理（CSP 免疫 · 2026-09-21）
///
/// 背景：翻译脚本原先用 `img.src = 'shift://__translate__/…'` 调后端，但**图片加载受站点
/// CSP 的 img-src 约束**——主流站点普遍带 CSP，导致整页/划词翻译在页面层被浏览器直接拒绝
/// （`Refused to load shift://… because it does not appear in the img-src directive`）。
///
/// 修复：改用 **WebKit 原生脚本消息处理器**（`window.webkit.messageHandlers.shiftTranslate`）——
/// 原生消息不走页面 CSP。JS 发 JSON 字符串：`{"path":"/batch","params":{…},"callback":"__sb_tr_x"}`，
/// 结果仍用 `write_translate_result` 写回页面全局变量（与旧通道一致）。
///
/// 与旧通道**并存**：老站点（无 CSP）继续走 `shift://`，双通道更稳。
pub(crate) fn native_translate_handle(app: &tauri::AppHandle, webview_label: &str, raw: &str) {
    let v: serde_json::Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[translate-native] 消息 JSON 非法: {}", e);
            return;
        }
    };
    // path 规范化：旧通道是 `shift://__translate__/` + path（前缀带斜杠），故 JS 传的 path
    // 可能**不带**前导斜杠（如 "whole-page"）——统一补上，避免"未知路径"。
    let path_raw = v.get("path").and_then(|x| x.as_str()).unwrap_or("");
    let path = if path_raw.starts_with('/') {
        path_raw.to_string()
    } else {
        format!("/{}", path_raw)
    };
    let callback = v
        .get("callback")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let params: std::collections::HashMap<String, String> = v
        .get("params")
        .and_then(|x| x.as_object())
        .map(|o| {
            o.iter()
                .map(|(k, val)| (k.clone(), val.as_str().unwrap_or("").to_string()))
                .collect()
        })
        .unwrap_or_default();

    if path != "/progress" && !valid_callback(&callback) {
        log::warn!("[translate-native] 非法 callback 拒绝: {:?}", callback);
        return;
    }
    if path != "/progress" && translate_rate_limited(webview_label) {
        let payload = serde_json::json!({ "error": "翻译请求过于频繁，请稍后再试" });
        write_translate_result(app, webview_label, &callback, &payload);
        return;
    }
    let app_clone = app.clone();
    let label = webview_label.to_string();
    match path.as_str() {
        "/text" => {
            let text = params.get("text").cloned().unwrap_or_default();
            if text.len() > 5000 {
                let payload = serde_json::json!({ "error": "待翻译文本过长（>5000 字符）" });
                write_translate_result(&app_clone, &label, &callback, &payload);
                return;
            }
            tauri::async_runtime::spawn(async move {
                let (engine, timeout) = match app_clone.try_state::<Db>() {
                    Some(db) => (
                        translate::current_engine(db.inner()).await,
                        translate::current_timeout(db.inner()).await,
                    ),
                    None => ("google".to_string(), 15),
                };
                let payload =
                    match translate::translate_with(&text, "zh-CN", &engine, timeout).await {
                        Ok(t) => serde_json::json!({ "text": t }),
                        Err(e) => serde_json::json!({ "error": e }),
                    };
                write_translate_result(&app_clone, &label, &callback, &payload);
            });
        }
        "/batch" => {
            let texts_json = params.get("texts").cloned().unwrap_or_default();
            log::info!(
                "[translate-native] 批量翻译请求: label={} bytes={}",
                label,
                texts_json.len()
            );
            tauri::async_runtime::spawn(async move {
                let texts: Vec<String> = match serde_json::from_str(&texts_json) {
                    Ok(v) => v,
                    Err(e) => {
                        let payload = serde_json::json!({ "error": format!("参数解析失败: {}", e) });
                        write_translate_result(&app_clone, &label, &callback, &payload);
                        return;
                    }
                };
                if texts.len() > 20 || texts.iter().any(|t| t.len() > 2000) {
                    let payload =
                        serde_json::json!({ "error": "批量翻译参数超限（≤20 段、每段 ≤2000 字符）" });
                    write_translate_result(&app_clone, &label, &callback, &payload);
                    return;
                }
                let mut out: Vec<String> = vec![String::new(); texts.len()];
                let (engine, target, timeout) = match app_clone.try_state::<Db>() {
                    Some(db) => (
                        translate::current_engine(db.inner()).await,
                        translate::current_target(db.inner()).await,
                        translate::current_timeout(db.inner()).await,
                    ),
                    None => ("google".to_string(), "zh-CN".to_string(), 15),
                };
                for chunk_start in (0..texts.len()).step_by(5) {
                    let chunk_end = (chunk_start + 5).min(texts.len());
                    let mut handles = Vec::new();
                    for (offset, t) in texts[chunk_start..chunk_end].iter().enumerate() {
                        let i = chunk_start + offset;
                        let t = t.clone();
                        let eng = engine.clone();
                        let tgt = target.clone();
                        handles.push((
                            i,
                            tauri::async_runtime::spawn(async move {
                                translate::translate_with(&t, &tgt, &eng, timeout).await
                            }),
                        ));
                    }
                    for (i, h) in handles {
                        if let Ok(Ok(t)) = h.await {
                            out[i] = t;
                        }
                    }
                }
                let payload = serde_json::json!({ "texts": out });
                write_translate_result(&app_clone, &label, &callback, &payload);
            });
        }
        "/whole-page" => {
            log::info!("[translate-native] 整页翻译请求: label={}", label);
            let label2 = label.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(wv) = app_clone.get_webview(&label2) {
                    if let Err(e) = wv.eval(WHOLE_PAGE_TRANSLATE_JS) {
                        let payload =
                            serde_json::json!({ "error": format!("脚本注入失败: {}", e) });
                        write_translate_result(&app_clone, &label2, &callback, &payload);
                    }
                }
            });
        }
        "/progress" => {
            {
                let cell = PROGRESS_THROTTLE
                    .get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
                let mut map = cell.lock().unwrap_or_else(|e| e.into_inner());
                let now = std::time::Instant::now();
                if let Some(t0) = map.get(&label).copied() {
                    if now.duration_since(t0).as_millis() < 300 {
                        return;
                    }
                }
                map.insert(label.clone(), now);
            }
            let p = params.clone();
            let app_p = app.clone();
            tauri::async_runtime::spawn(async move {
                let engine = match app_p.try_state::<Db>() {
                    Some(db) => translate::current_engine(db.inner()).await,
                    None => "google".to_string(),
                };
                let num = |k: &str| p.get(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                let payload = serde_json::json!({
                  "state": p.get("state").cloned().unwrap_or_else(|| "update".to_string()),
                  "engine": if engine == "bing" { "Bing" } else { "Google" },
                  "pct": num("pct"),
                  "info": p.get("info").cloned().unwrap_or_default(),
                  "elapsed": num("elapsed"),
                  "eta": num("eta"),
                  "fails": num("fails"),
                });
                let _ = app_p.emit("translate-progress", payload);
            });
        }
        _ => log::warn!("[translate-native] 未知路径: {}", path),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // ===== 画面闪烁修复：禁用 WebKitGTK 的 DMA-BUF 渲染器 =====
    // WebKitGTK 2.42+ 默认走 DMA-BUF 渲染路径，在部分核显（本机 Intel HD 630 / i915）
    // 上与驱动配合不良，表现为**网页/视频画面闪烁**（社区公认问题）。禁用后回退到
    // 共享内存渲染路径。必须在 WebKit 初始化（建 webview）之前设置。
    // 用户可用环境变量覆盖（若某机器上 DMA-BUF 正常，可设 0 保留更优性能）。
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    log::info!(
        "[ShiftBrowser] WEBKIT_DISABLE_DMABUF_RENDERER={:?}（视频闪烁修复）",
        std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER")
    );

    // ===== 视频进程崩溃修复（2026-09-22）：降级硬件视频解码器 =====
    // 症状：播放某些 H.264 视频时 WebKitWebProcess 直接 abort：
    //   ** (WebKitWebProcess:PID): ERROR **: video meta uses 368x480 instead of 360x480
    // 成因：硬件解码器（V4L2 stateless / VA-API）输出帧宽按 16 字节对齐（360→368），
    // 而 WebKit 的 video meta 校验期望未对齐宽度，断言失败 → g_error() → 进程终止。
    // 规避：用 GStreamer 标准的 GST_PLUGIN_FEATURE_RANK 把硬件解码器 rank 压到 0，
    // 使其回退到软件解码（libav），彻底绕开对齐 padding 路径。
    // 用户可覆盖（机器上硬解正常、且想要 4K 硬解性能时，自行设置该变量）。
    if std::env::var_os("GST_PLUGIN_FEATURE_RANK").is_none() {
        std::env::set_var(
            "GST_PLUGIN_FEATURE_RANK",
            // rank 0 = 不参与自动选择；软件解码 avdec_* 保持默认 rank
            "v4l2slh264dec:0,v4l2slh265dec:0,v4l2slvp8dec:0,v4l2slvp9dec:0,v4l2slav1dec:0,\
             v4l2h264dec:0,v4l2h265dec:0,v4l2vp8dec:0,v4l2vp9dec:0,\
             vah264dec:0,vah265dec:0,vavp8dec:0,vavp9dec:0,vaav1dec:0,vajpegdec:0",
        );
    }
    log::info!(
        "[ShiftBrowser] GST_PLUGIN_FEATURE_RANK={:?}（视频进程崩溃修复）",
        std::env::var("GST_PLUGIN_FEATURE_RANK")
    );

    // ===== Cookie 持久化（修复"登录状态不保存"）=====
    // WebKitGTK 的 CookieManager 默认是内存模式，重启即丢——必须显式
    // set_persistent_storage 落盘。且该调用必须在任何 web process 创建之前
    // （tauri 的 setup 回调执行时主 webview 已创建，已太晚），故放在 run() 开头。
    // 存储路径与 app_data_dir 一致：$XDG_DATA_HOME/com.shiftbrowser.app/cookies.sqlite
    #[cfg(target_os = "linux")]
    {
        // WebContext::default() 要求 GTK 已初始化；gtk::init() 幂等（已初始化直接 Ok），
        // 之后 tauri 再初始化不会冲突
        let _ = gtk::init();
        use webkit2gtk::{CookieManagerExt, CookiePersistentStorage, WebContext, WebContextExt};
        if let Some(context) = WebContext::default() {
            if let Some(cm) = context.cookie_manager() {
                let data_home = std::env::var("XDG_DATA_HOME")
                    .map(std::path::PathBuf::from)
                    .unwrap_or_else(|_| {
                        std::env::var("HOME")
                            .map(|h| std::path::PathBuf::from(h).join(".local/share"))
                            .unwrap_or_else(|_| std::path::PathBuf::from("."))
                    });
                let cookie_path = data_home.join("com.shiftbrowser.app").join("cookies.sqlite");
                if let Some(dir) = cookie_path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                cm.set_persistent_storage(
                    cookie_path.to_str().unwrap_or("cookies.sqlite"),
                    CookiePersistentStorage::Sqlite,
                );
                // 安全：Cookie 库按 umask 落盘（常见 664，组/其他可读），立即收紧到 600。
                // 运行时 WebKit 仍可能新建/重建这些文件，故另有周期重扫（见 setup）。
                if let Some(dir) = cookie_path.parent() {
                    crate::harden_sensitive_files(dir);
                }
                // 注意：此处 logger 插件尚未初始化（Builder 之前），log::info! 会被丢弃，
            // 用 println! 保证启动日志可见
            println!("[ShiftBrowser] Cookie 持久化已启用: {}", cookie_path.display());
            }
        }
    }

    // ===== 隐私防护 P3：TLS 指纹规范化（已回退，2026-09-18）=====
    // 回退原因：设置 G_TLS_GNUTLS_PRIORITY 后邮箱正文渲染为空 —— 该环境变量
    // 作用于进程内全部 TLS 连接，若站点部分资源依赖被裁掉的协议/套件组合，
    // 会表现为内容加载不全（空白）。网络底层改动风险远高于收益，故回退。
    // 保留结论：JA4 无法伪装成 Chrome（GnuTLS vs BoringSSL），
    // 与其在底层冒险，不如把预算放在页面内的加噪与归一化。
    // std::env::set_var(
    //     "G_TLS_GNUTLS_PRIORITY",
    //     "NORMAL:-VERS-ALL:+VERS-TLS1.3:+VERS-TLS1.2:-VERS-TLS1.0:-VERS-TLS1.1",
    // );
    tauri::Builder::default()
    // 单实例（阶段 4 D3）：已有实例运行时，新实例退出并把命令行参数转发给已有实例。
    // 必须注册为第一个插件（官方要求）。回调在已有实例进程里执行，
    // argv 是新实例的完整命令行参数——过滤 http/https 后让已有实例开新标签。
    .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
      let urls: Vec<String> = argv
        .iter()
        .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
        .cloned()
        .collect();
      log::info!("[single-instance] 已有实例收到新实例参数: {:?}", argv);
      if !urls.is_empty() {
        log::info!("[single-instance] 转发外部链接 {} 个", urls.len());
        let app_handle = app.clone();
        tauri::async_runtime::spawn(async move {
          // 稍等现有实例就绪（标签管理器已 manage）
          tokio::time::sleep(std::time::Duration::from_millis(300)).await;
          for url in urls {
            if let Some(state) = app_handle.try_state::<TabManager>() {
              match tabs::create_tab(app_handle.clone(), state, url.clone()).await {
                Ok(tab) => log::info!("[single-instance] 外部链接已打开: {}", tab.id),
                Err(e) => log::warn!("[single-instance] 外部链接打开失败: {} ({})", redact_url(&url), e),
              }
            }
          }
          // 窗口前置（可能最小化/缩托盘）
          if let Some(w) = app_handle.get_window("main") {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
          }
        });
      }
    }))
    .plugin(tauri_plugin_opener::init())
    // 注意：不再注册 shell/dialog/fs 插件——前端只用到 opener，
    // 多余插件只增加攻击面与二进制体积（安全最小化原则）
    .plugin(
      tauri_plugin_log::Builder::default()
        .level(log::LevelFilter::Info)
        // 并发写 SQLite 时 sqlx 慢语句 WARN 刷屏，降到 Error
        .level_for("sqlx::query", log::LevelFilter::Error)
        .build(),
    )
    .invoke_handler(tauri::generate_handler![
      // 启动锁屏状态（前端初始化前查询，锁屏期间静默等待解锁）
      is_locked,
      // 前端日志桥（排查前端执行链路）
      front_log,
      // 设置
      settings::get_setting,
      settings::get_all_settings,
      settings::set_setting,
      settings::delete_setting,
      settings::set_settings_bulk,
      // 网络层拦截规则落盘（阶段3.2：写 Web 进程扩展规则文件）
      settings::sync_ext_rules,
      // 默认浏览器（阶段 4：xdg-settings 集成）
      default_browser::default_browser_status,
      default_browser::set_default_browser,
      // 用户脚本（阶段3.2 平台化：Tampermonkey 风格）
      db::userscripts::list_userscripts,
      db::userscripts::refresh_userscripts_cache,
      db::userscripts::add_userscript,
      db::userscripts::update_userscript,
      db::userscripts::delete_userscript,
      // 证书管理 / 监测 / 验证（P1-P3 · 2026-09-22）
      tls::get_tls_info,
      tls::list_certificates,
      tls::get_certificate,
      tls::delete_certificate,
      tls::list_cert_exceptions,
      tls::delete_cert_exception,
      tls::list_cert_events,
      tls::clear_cert_events,
      tls::scan_cert_expiry,
      tls::accept_cert_exception,
      // 权限请求（安全 #2 · 2026-09-22）
      perms::respond_permission,
      perms::list_permission_rules,
      perms::remove_permission_rule,
      // 书签
      bookmarks::get_bookmarks,
      bookmarks::get_bookmark_folders,
      bookmarks::create_bookmark,
      bookmarks::create_bookmark_folder,
      bookmarks::update_bookmark,
      bookmarks::delete_bookmark,
      bookmarks::delete_bookmark_folder,
      bookmarks::search_bookmarks,
      bookmarks::import_bookmarks_html,
      bookmarks::import_bookmarks_from_file,
      bookmarks::export_bookmarks_html,
      // 历史
      history::record_history,
      history::backfill_history_titles,
      history::get_recent_history,
      history::search_history,
      history::get_most_visited,
      history::clear_history,
      history::clear_history_before,
      history::delete_history_entry,
      // 会话
      sessions::save_session,
      sessions::load_last_session,
      sessions::list_sessions,
      sessions::delete_session,
      // 翻译
      translate::translate_text,
      // 下载记录
      downloads::create_download,
      downloads::update_download_progress,
      downloads::get_downloads,
      downloads::delete_download,
      // 标签分组
      tab_groups::create_tab_group,
      tab_groups::get_tab_groups,
      tab_groups::update_tab_group,
      tab_groups::delete_tab_group,
      // 工作区
      workspaces::create_workspace,
      workspaces::get_workspaces,
      workspaces::update_workspace,
      workspaces::delete_workspace,
      // 密码（AES-256-GCM 加密存储）
      passwords::save_password,
      passwords::get_password,
      passwords::get_password_by_id,
      passwords::get_passwords_for_domain,
      passwords::list_passwords,
      passwords::delete_password,
      // 密码库主密码（阶段 2：数据主权底线）
      db::vault::vault_status,
      db::vault::set_vault_password,
      db::vault::unlock_vault,
      db::vault::lock_vault,
      db::vault::remove_vault_password,
      // 上云 E2E 加密同步（第 45 章 · 服务器零知识）
      db::sync::sync_status,
      db::sync::sync_configure,
      db::sync::sync_push,
      db::sync::sync_pull,
      db::sync::sync_clear,
      // 标签页
      tabs::create_tab,
      tabs::close_tab,
      tabs::close_all_tabs,
      tabs::switch_tab,
      // 性能：空闲标签休眠（回收渲染进程内存）
      tabs::hibernate_idle_tabs,
      // 翻译：对当前活动标签整页翻译（复用 whole_page_translate.js）
      tabs::translate_current_page,
      tabs::navigate_tab,
      tabs::go_back,
      tabs::go_forward,
      tabs::get_tabs,
      tabs::set_webview_visible,
      tabs::update_tab,
      tabs::reload_tab,
      tabs::reorder_tabs,
      tabs::toggle_pin,
      tabs::toggle_mute,
      tabs::toggle_maximize,
      tabs::minimize_window,
      tabs::close_window,
      tabs::start_drag_window,
      tabs::set_bookmarks_bar_visible,
      tabs::set_vertical_tabs,
      tabs::set_split_tab,
      tabs::clear_split,
      tabs::get_split_tab,
      tabs::get_nav_state,
      tabs::set_webview_ai_panel,
      tabs::open_devtools,
      // 自检（可观测性）
      selfcheck_cmd,
      // AI 侧边栏
      ai::ai_chat,
      ai::ai_chat_stream,
      ai::ai_cancel_stream,
      ai::ai_list_providers,
      // 页面理解层（阶段 1.1）
      content_cmd::content_snapshot,
      content_cmd::content_extract,
      content_cmd::ai_friendliness,
      // llms.txt 信号（阶段 1.5 标准跟踪 · Lighthouse Agentic 信号③/④落地；
      // 走 LlmsCache 会话缓存 + 每域限次 + 3s 超时 + 失败静默——Agent 导航用的"站点自述"）
      crate::engine::llmstxt::read_llmstxt,
      // MCP 工具层（阶段 1.3）
      mcp_cmd::mcp_list_servers,
      mcp_cmd::mcp_add_server,
      mcp_cmd::mcp_remove_server,
      mcp_cmd::mcp_set_server_enabled,
      mcp_cmd::mcp_list_tools,
      mcp_cmd::mcp_call_tool,
      mcp_cmd::mcp_get_audit_log,
      // Agent 执行层（阶段 1.4）
      agent_cmd::agent_confirm_action,
      agent_cmd::agent_confirm_multi_step,
      agent_cmd::agent_action_record,
      agent_cmd::agent_action_history,
      agent_cmd::agent_action_replay_or_retry,
      agent_cmd::agent_actions_batch_retry,
      agent_cmd::agent_debug_execute,
      // 扩展管理
      extensions::load_extension,
      extensions::list_extensions,
      extensions::set_extension_enabled,
      extensions::delete_extension,
      // 广告拦截
      extensions::set_adblock_enabled,
      extensions::get_adblock_enabled,
      // 隐私防护（GPC + 反指纹 + WebRTC 防泄露）
      extensions::set_privacy_shield_enabled,
      extensions::get_privacy_shield_enabled,
      extensions::set_privacy_shield_level,
      extensions::get_privacy_shield_level,
      extensions::set_privacy_whitelist,
      extensions::get_privacy_whitelist,
      // 反指纹 2.0 双模式（阶段 2：加噪 / 统一化）
      extensions::set_privacy_fp_mode,
      extensions::get_privacy_fp_mode,
      // WebMCP polyfill 试验（阶段 1.5）
      extensions::set_webmcp_polyfill,
      extensions::get_webmcp_polyfill,
      // 隐私防护统计（防护看得见）
      extensions::get_privacy_stats,
      extensions::reset_privacy_stats,
      extensions::privacy_selftest,
      // GPC 站点声明感知（阶段 2：GPC 完整化）
      gpc::gpc_site_check,
      // 零遥测审计（阶段 2：全量网络出口清单）
      audit::egress_audit,
      // 新标签页
      newtab::refresh_newtab,
      newtab::get_newtab_url,
      newtab::save_quick_sites,
      newtab::todo_list,
      newtab::todo_save,
      newtab::open_ai_from_newtab,
      newtab::open_settings_from_newtab,
      // 用户账号（个人中心）
      users::user_register,
      users::user_login,
      users::user_logout,
      users::user_get_current,
      users::user_update_profile,
      users::user_change_password,
      users::user_delete,
      users::user_update_prefs,
      users::user_export,
      users::user_import,
      // 阅读模式
      reader::enter_reader_mode,
      reader::fetch_article,
      extensions::get_adblock_keywords,
      extensions::set_adblock_keywords,
    ])
    .register_uri_scheme_protocol("shift", |ctx, request| {
      // shift://newtab → newtab/index.html; shift://worldnav → world-nav.html
      // shift://main → dist 前端资源（主窗口，从嵌入资源读取并替换 invoke_key）
      // 注意：shift://newtab 的 host 是 newtab，path 是 /
      let host = request.uri().host().unwrap_or("").to_string();
      let path = request.uri().path().to_string();
      let app = ctx.app_handle();

      // 主窗口：shift://main → dist（asset_resolver 读嵌入资源）
      if host == "main" {
        let rel = if path == "/" || path == "/index.html" {
          "index.html".to_string()
        } else {
          path.trim_start_matches('/').to_string()
        };
        if let Some(asset) = app.asset_resolver().get(rel.clone()) {
          let bytes = asset.bytes().to_vec();
          let mime = asset.mime_type();
          if rel == "index.html" {
            // 替换 IPC 桥里的 invoke_key 占位符（Tauri 校验 invoke_key）
            let html = String::from_utf8_lossy(&bytes);
            let html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
            return tauri::http::Response::builder()
              .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
              .header(tauri::http::header::CONTENT_SECURITY_POLICY, SHIFT_CSP)
              .body(html.into_bytes())
              .unwrap();
          }
          return tauri::http::Response::builder()
            .header(tauri::http::header::CONTENT_TYPE, mime)
            .body(bytes)
            .unwrap();
        }
        log::warn!("[shift://] main 资源未找到: {}", rel);
        return tauri::http::Response::builder()
          .status(404)
          .header(tauri::http::header::CONTENT_TYPE, "text/plain")
          .body("Not Found".as_bytes().to_vec())
          .unwrap();
      }

      // 翻译 IPC：child webview 的右键菜单 JS 通过 shift://__translate__/* 调翻译
      // （shift handler 同步返回，翻译异步 spawn，结果通过 webview.eval 写回 window 全局变量，
      //  菜单 JS 轮询拿到结果）
      if host == "__translate__" {
        let webview_label = ctx.webview_label().to_string();
        // 来源校验（2026-09-19 安全修复 + 修正）：翻译功能服务两类页面——
        // ① shift:// 内部页（右键菜单注入在内部页）
        // ② 普通 http/https 网页（右键菜单注入在网页，用户选中文字翻译）
        // 因此按"发起请求的 webview 当前 URL 必须是网页或内部页"放行，
        // 拒绝 file:// 等其他来源（走主线程 proxy 拿 URL，可靠）。
        // 注：不能用 Referer 校验——WebKit 对 shift:// 请求不带 Referer（恒空）。
        let wv_url = app
          .get_webview(&webview_label)
          .and_then(|wv| wv.url().ok())
          .map(|u| u.to_string())
          .unwrap_or_default();
        let wv_url_ok = wv_url.starts_with("shift://")
          || wv_url.starts_with("http://")
          || wv_url.starts_with("https://");
        if !wv_url_ok {
          log::warn!(
            "[translate] 拒绝非网页来源翻译请求: wv_url={:?} label={}",
            wv_url, webview_label
          );
          return tauri::http::Response::builder()
            .status(403)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain")
            .body("Forbidden".as_bytes().to_vec())
            .unwrap();
        }
        let query = request.uri().query().unwrap_or("");
        let params: std::collections::HashMap<String, String> = query
          .split('&')
          .filter_map(|kv| {
            let mut it = kv.splitn(2, '=');
            let k = it.next()?;
            let v = it.next().unwrap_or("");
            Some((percent_decode(k), percent_decode(v)))
          })
          .collect();
        let callback = params.get("callback").cloned().unwrap_or_default();
        // P2：callback 白名单——不合法直接拒绝，不 eval（防 JS 注入）
        // 例外：/progress 只做进度上报（emit 事件给界面），不写回调变量、不 eval，无需 callback
        if path != "/progress" && !valid_callback(&callback) {
          log::warn!("[translate] 非法 callback 拒绝: {:?}", callback);
          return tauri::http::Response::builder()
            .status(400)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain")
            .body("Bad Request".as_bytes().to_vec())
            .unwrap();
        }
        let app_clone = app.clone();

        // 速率限制：翻译类请求（/text /batch /whole-page）每 webview 每分钟最多 30 次；
        // /progress 已有 300ms 节流，不占此额度
        if path != "/progress" && translate_rate_limited(&webview_label) {
          log::warn!("[translate] 翻译请求超频拒绝: label={}", webview_label);
          let payload = serde_json::json!({ "error": "翻译请求过于频繁，请稍后再试" });
          write_translate_result(&app_clone, &webview_label, &callback, &payload);
          return tauri::http::Response::builder()
            .status(200)
            .header(tauri::http::header::CONTENT_TYPE, "image/gif")
            .body(Vec::<u8>::new())
            .unwrap();
        }

        match path.as_str() {
          "/text" => {
            let text = params.get("text").cloned().unwrap_or_default();
            log::info!(
              "[translate] 选中文本翻译 request: label={} callback={} len={}",
              webview_label, callback, text.len()
            );
            // P2：单段文本限长——恶意页面可刷超大文本放大翻译请求（外部网络调用）
            if text.len() > 5000 {
              let payload = serde_json::json!({ "error": "待翻译文本过长（>5000 字符）" });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
              return tauri::http::Response::builder()
                .status(200)
                .header(tauri::http::header::CONTENT_TYPE, "image/gif")
                .body(Vec::<u8>::new())
                .unwrap();
            }
            tauri::async_runtime::spawn(async move {
              // 按设置里的翻译源（google/bing）派发，失败自动回退另一方
              let (engine, timeout) = match app_clone.try_state::<Db>() {
                Some(db) => (
                  translate::current_engine(db.inner()).await,
                  translate::current_timeout(db.inner()).await,
                ),
                None => ("google".to_string(), 15),
              };
              let payload =
                match translate::translate_with(&text, "zh-CN", &engine, timeout).await {
                  Ok(t) => serde_json::json!({ "text": t }),
                  Err(e) => serde_json::json!({ "error": e }),
                };
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
            });
          }
          "/progress" => {
            // 整页翻译进度上报（页面内脚本 → 界面地址栏的进度条）
            // 参照原版 Electron 的 translate-progress 事件：engine/pct/info/elapsed/eta/fails
            // 节流：同一 webview 至少 300ms 一条，防高频上报刷爆 UI 事件通道
            {
              let cell = PROGRESS_THROTTLE.get_or_init(|| {
                std::sync::Mutex::new(std::collections::HashMap::new())
              });
              let mut map = cell.lock().unwrap_or_else(|e| e.into_inner());
              let now = std::time::Instant::now();
              let last = map.get(&webview_label).copied();
              if let Some(t0) = last {
                if now.duration_since(t0).as_millis() < 300 {
                  return tauri::http::Response::builder()
                    .status(200)
                    .header(tauri::http::header::CONTENT_TYPE, "image/gif")
                    .body(Vec::<u8>::new())
                    .unwrap();
                }
              }
              map.insert(webview_label.clone(), now);
            }
            let p = params.clone();
            let app_p = app.clone();
            tauri::async_runtime::spawn(async move {
              let engine = match app_p.try_state::<Db>() {
                Some(db) => translate::current_engine(db.inner()).await,
                None => "google".to_string(),
              };
              let num = |k: &str| p.get(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
              let payload = serde_json::json!({
                "state": p.get("state").cloned().unwrap_or_else(|| "update".to_string()),
                "engine": if engine == "bing" { "Bing" } else { "Google" },
                "pct": num("pct"),
                "info": p.get("info").cloned().unwrap_or_default(),
                "elapsed": num("elapsed"),
                "eta": num("eta"),
                "fails": num("fails"),
              });
              let _ = app_p.emit("translate-progress", payload);
            });
          }
          "/whole-page" => {
            log::info!(
              "[translate] 整页翻译 request: label={} callback={}",
              webview_label, callback
            );
            // 注入整页翻译脚本：脚本在页面内收集文本节点 → 分批调 /batch → 回填 DOM
            // （eval 是单向的，收集结果通过 /batch 的 shift:// 请求与回调变量回传）
            let wv_label = webview_label.clone();
            tauri::async_runtime::spawn(async move {
              if let Some(wv) = app_clone.get_webview(&wv_label) {
                if let Err(e) = wv.eval(WHOLE_PAGE_TRANSLATE_JS) {
                  log::warn!("[translate] 整页翻译脚本注入失败: {}", e);
                  let payload = serde_json::json!({ "error": format!("脚本注入失败: {}", e) });
                  write_translate_result(&app_clone, &wv_label, &callback, &payload);
                } else {
                  log::info!("[translate] 整页翻译脚本已注入: {}", wv_label);
                }
              } else {
                log::warn!("[translate] 找不到 webview: {}", wv_label);
              }
            });
          }
          "/batch" => {
            // 批量翻译（整页翻译脚本调用）：texts 是 JSON 数组，逐段翻（并发 5）
            let texts_json = params.get("texts").cloned().unwrap_or_default();
            log::info!(
              "[translate] 批量翻译 request: label={} bytes={}",
              webview_label, texts_json.len()
            );
            tauri::async_runtime::spawn(async move {
              let texts: Vec<String> = match serde_json::from_str(&texts_json) {
                Ok(v) => v,
                Err(e) => {
                  let payload = serde_json::json!({ "error": format!("参数解析失败: {}", e) });
                  write_translate_result(&app_clone, &webview_label, &callback, &payload);
                  return;
                }
              };
              // P2：批量限长——恶意页面可刷超大数组/超长文本放大翻译请求
              // （每段翻一次外部网络调用，20 段×2000 字符已是整页翻译的合理上限）
              if texts.len() > 20 || texts.iter().any(|t| t.len() > 2000) {
                let payload =
                  serde_json::json!({ "error": "批量翻译参数超限（≤20 段、每段 ≤2000 字符）" });
                write_translate_result(&app_clone, &webview_label, &callback, &payload);
                return;
              }
              let mut out: Vec<String> = vec![String::new(); texts.len()];
              // 整页翻译一次要翻几十段，设置只读一次，按引擎派发（失败自动回退另一方）
              let (engine, target, timeout) = match app_clone.try_state::<Db>() {
                Some(db) => (
                  translate::current_engine(db.inner()).await,
                  translate::current_target(db.inner()).await,
                  translate::current_timeout(db.inner()).await,
                ),
                None => ("google".to_string(), "zh-CN".to_string(), 15),
              };
              // 分批并发（每批 5 段），保持顺序
              for chunk_start in (0..texts.len()).step_by(5) {
                let chunk_end = (chunk_start + 5).min(texts.len());
                let mut handles = Vec::new();
                for (offset, t) in texts[chunk_start..chunk_end].iter().enumerate() {
                  let i = chunk_start + offset;
                  let t = t.clone();
                  let eng = engine.clone();
                  let tgt = target.clone();
                  handles.push((
                    i,
                    tauri::async_runtime::spawn(async move {
                      translate::translate_with(&t, &tgt, &eng, timeout).await
                    }),
                  ));
                }
                for (i, h) in handles {
                  if let Ok(Ok(t)) = h.await {
                    out[i] = t;
                  }
                }
              }
              let payload = serde_json::json!({ "texts": out });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
            });
          }
          "/remove-auto-site" => {
            // 自动汉化功能尚未实现（前端无入口），此分支为预留。
            // 明确记录，不做假成功（2026-09-19 清理）
            log::warn!(
              "[translate] remove-auto-site 收到请求但自动汉化未实现，忽略: label={}",
              webview_label
            );
            tauri::async_runtime::spawn(async move {
              let payload = serde_json::json!({ "ok": true });
              write_translate_result(&app_clone, &webview_label, &callback, &payload);
            });
          }
          _ => {
            log::warn!("[translate] 未知子路径: {}", path);
          }
        }

        // shift handler 同步返回（菜单 JS 用 Image().src 触发，不读响应体）
        return tauri::http::Response::builder()
          .status(200)
          .header(tauri::http::header::CONTENT_TYPE, "image/gif")
          .body(Vec::<u8>::new())
          .unwrap();
      }

      let app_data = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
      let newtab_dir = app_data.join("newtab");
      let file_path = match (host.as_str(), path.as_str()) {
        ("newtab", "/") | ("newtab", "/index.html") => newtab_dir.join("index.html"),
        // newtab 页内"世界导航"按钮用相对路径 world-nav.html 跳转，
        // 落到 shift://newtab/world-nav.html，必须匹配到才能加载世界导航页
        ("newtab", "/world-nav.html") => newtab_dir.join("world-nav.html"),
        ("worldnav", "/") | ("worldnav", "/index.html") => newtab_dir.join("world-nav.html"),
        ("worldnav", "/world-nav.html") => newtab_dir.join("world-nav.html"),
        _ => {
          log::warn!("[shift://] 未知路径: {} {}", host, path);
          return tauri::http::Response::builder()
            .status(404)
            .header(tauri::http::header::CONTENT_TYPE, "text/plain")
            .body("Not Found".as_bytes().to_vec())
            .unwrap();
        }
      };
      log::info!("[shift://] 加载: {:?}", file_path);
      if let Ok(data) = std::fs::read(&file_path) {
        // 把 IPC 桥里的 invoke_key 占位符替换成运行时真实 key（Tauri 校验 invoke_key）
        let html = String::from_utf8_lossy(&data);
        let html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
        tauri::http::Response::builder()
          .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
          // 2026-09-19：WebKit 对自定义协议默认缓存（刷新走缓存不重新请求），
          // 导致改启动页后高哥刷新看不到变化——强制 no-store，每次加载都读最新文件
          .header(tauri::http::header::CACHE_CONTROL, "no-store, no-cache, must-revalidate")
          .header(tauri::http::header::PRAGMA, "no-cache")
          .header(tauri::http::header::CONTENT_SECURITY_POLICY, SHIFT_CSP)
          .body(html.into_bytes())
          .unwrap()
      } else {
        log::error!("[shift://] 文件读取失败: {:?}", file_path);
        tauri::http::Response::builder()
          .status(404)
          .header(tauri::http::header::CONTENT_TYPE, "text/plain")
          .body("File not found".as_bytes().to_vec())
          .unwrap()
      }
    })
    .setup(|app| {
      // 安全（#1 · 2026-09-22）：敏感文件（cookie 库/WAL）启动即加固到 0600，
      // 并每 120s 重扫一次——WebKit 运行时会按 umask 重建这些文件。
      #[cfg(unix)]
      {
        use tauri::Manager;
        if let Ok(dir) = app.path().app_data_dir() {
          crate::harden_sensitive_files(&dir);
          let dir2 = dir.clone();
          tauri::async_runtime::spawn(async move {
            loop {
              tokio::time::sleep(std::time::Duration::from_secs(120)).await;
              crate::harden_sensitive_files(&dir2);
            }
          });
        }
      }
      // ===== 性能：WebKitGTK 内存压力治理 =====
      // 实测痛点：9 个标签时全部 WebKit 进程合计 3.75 GB（每标签一个渲染进程、
      // 单个 240-500MB，WebKitGTK 默认不主动回收）。
      // 方案：设置内存压力阈值，超过后 WebKit 分级回收（保守→严格→杀进程）。
      // 必须在使用任何 WebContext 之前设置（WebsiteDataManager 的静态全局设置）。
      #[cfg(target_os = "linux")]
      {
        use webkit2gtk::{MemoryPressureSettings, WebsiteDataManager};
        // 读系统总内存（MemTotal，kB）
        let total_mb: u32 = std::fs::read_to_string("/proc/meminfo")
          .ok()
          .and_then(|s| {
            s.lines()
              .find(|l| l.starts_with("MemTotal:"))
              .and_then(|l| l.split_whitespace().nth(1))
              .and_then(|v| v.parse::<u64>().ok())
          })
          .map(|kb| (kb / 1024) as u32)
          .unwrap_or(8192);
        // 上限取系统内存 40%：留足余量给系统与其他应用，避免过早回收伤性能
        let limit = ((total_mb as f64) * 0.4) as u32;
        let mut mps = MemoryPressureSettings::new();
        mps.set_memory_limit(limit);
        mps.set_conservative_threshold(0.33); // 温和回收（清缓存）
        mps.set_strict_threshold(0.5); // 严格回收（释放渲染树）
        mps.set_kill_threshold(0.9); // 接近极限才杀进程，避免误杀导致标签崩溃
        mps.set_poll_interval(1.0);
        WebsiteDataManager::set_memory_pressure_settings(&mut mps);
        log::info!(
          "[perf] WebKit 内存压力治理已启用：上限 {} MB（系统总内存 {} MB 的 40%）",
          limit,
          total_mb
        );
      }

      // 锁屏是否启用过（解锁后通知前端重试初始化——锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 命令调用全部失败，必须等 setup 完成后重试）
      let mut unlocked_after_lock = false;

      // 2026-09-19：GNOME 49 对无装饰窗口强制四角圆角，但 Tauri/wry 只把 WebView
      // 背景设为透明，GTK 窗口本身没设透明——圆角外露出黑色窗口背景（四个黑角）。
      // 手动设置 RGBA visual + app_paintable，让窗口真正透明，圆角外透出桌面。
      #[cfg(target_os = "linux")]
      {
        use gtk::prelude::*;
        if let Some(win) = app.get_window("main") {
          if let Ok(gtk_win) = win.gtk_window() {
            if let Some(screen) = gtk::prelude::WidgetExt::screen(&gtk_win) {
              if let Some(visual) = screen.rgba_visual() {
                gtk_win.set_visual(Some(&visual));
              }
              // Wayland 下 set_visual 无效（X11 概念），GTK 默认还会画主题背景色——
              // 用 CSS 强制窗口背景透明，圆角外才能透出桌面
              let css = gtk::CssProvider::new();
              css.load_from_data(b"window { background-color: transparent; }")
                .map_err(|e| format!("CSS 加载失败: {}", e))?;
              gtk::StyleContext::add_provider_for_screen(
                &screen,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
              );
              log::info!(
                "[ShiftBrowser] 窗口透明设置：rgba_visual={}",
                screen.rgba_visual().is_some()
              );
            }
            gtk_win.set_app_paintable(true);
            gtk_win.queue_draw();
            log::info!(
              "[ShiftBrowser] 窗口透明已启用（app_paintable={}）",
              gtk_win.is_app_paintable()
            );
          }
        }
      }

      // 初始化数据库（异步）
      let app_data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));

      log::info!("[ShiftBrowser] app_data_dir = {:?}", app_data_dir);
      log::info!("[ShiftBrowser] 目录存在: {}", app_data_dir.exists());

      let db_handle = tauri::async_runtime::block_on(Db::init(app_data_dir.clone()));
      match db_handle {
        Ok(db) => {
          app.manage(db);
          log::info!("[ShiftBrowser] 数据库初始化成功");
        }
        Err(e) => {
          log::error!("[ShiftBrowser] 数据库初始化失败: {}", e);
        }
      }

      // 锁屏前预生成新标签页 + 后台刷新壁纸——锁屏 dialog 阻塞 setup 期间，
      // 这些异步任务照常跑（tokio 不依赖 GTK 主循环），解锁时 index.html 已就绪，
      // 前端 createTab 的 refresh_newtab 拿 GEN_LOCK 时防抖直接跳过，秒开启动页。
      // generate 关键路径已改为只读缓存不拉网络（newtab.rs），纯本地毫秒级。
      {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
          let db_state = app_handle.state::<Db>();
          // 历史空标题回填（2026-09-19）：页面加载时 document.title 未就绪导致
          // 历史里存了空标题，最近访问只剩网址——用 URL 域名兜底
          if let Ok(fixed) = history::backfill_history_titles(db_state).await {
            if fixed > 0 {
              log::info!("[ShiftBrowser] 历史空标题回填完成: {} 条", fixed);
            }
          }
          let db_state = app_handle.state::<Db>();
          if let Some(path) = newtab::newtab_path(&app_handle) {
            match newtab::generate(&db_state, &path).await {
              Ok(p) => log::info!("[ShiftBrowser] 新标签页已预生成: {}", p),
              Err(e) => log::error!("[ShiftBrowser] 新标签页预生成失败: {}", e),
            }
          }
          // 壁纸后台刷新（独立于 GEN_LOCK，拉取不阻塞任何 generate）
          newtab::refresh_wallpapers_background(&app_handle).await;
        });
      }

      // 启动锁屏：设置启用后，在创建任何 webview 之前弹 GTK 原生密码对话框。
      // 注意：必须在 GTK 层面实现——Vue 锁屏覆盖层只能盖住 Vue 自己的 DOM，
      // 盖不住另一个独立的 GTK webview（标签页）。
      #[cfg(target_os = "linux")]
      {
        use gtk::prelude::*;
        let db_state = app.state::<Db>();
        let lock_enabled: bool = {
          let row = tauri::async_runtime::block_on(
            sqlx::query_as::<_, (String,)>(
              "SELECT value FROM settings WHERE key = 'lock_enabled'",
            )
            .fetch_optional(&db_state.pool),
          )
          .ok()
          .flatten();
          row.map(|r| r.0 == "true").unwrap_or(false)
        };

        if lock_enabled {
          log::info!("[ShiftBrowser] 启动锁屏已启用，弹出 GTK 密码对话框");
          unlocked_after_lock = true;
          // 标记锁屏中：前端 is_locked 查到 true 后静默等待 app-unlocked，
          // 不再反复重试建标签（否则弹"创建标签页失败"toast）
          LOCKED.store(true, std::sync::atomic::Ordering::SeqCst);
          let parent_win = app
            .get_window("main")
            .and_then(|w| w.gtk_window().ok());

          loop {
            let dialog = gtk::Dialog::builder()
              .title("Shift Browser 已锁定")
              .modal(true)
              .resizable(false)
              .default_width(340)
              .default_height(160)
              .build();

            if let Some(ref pw) = parent_win {
              dialog.set_transient_for(Some(pw));
            }

            let content = dialog.content_area();
            content.set_spacing(8);
            content.set_margin(12);

            let label = gtk::Label::new(Some("输入账号密码解锁浏览器"));
            content.add(&label);

            let user_entry = gtk::Entry::new();
            user_entry.set_placeholder_text(Some("用户名"));
            content.add(&user_entry);

            let pass_entry = gtk::Entry::new();
            pass_entry.set_visibility(false);
            pass_entry.set_placeholder_text(Some("密码"));
            pass_entry.set_activates_default(true);
            content.add(&pass_entry);

            dialog.add_button("退出", gtk::ResponseType::Cancel);
            dialog.add_button("解锁", gtk::ResponseType::Ok);

            dialog.show_all();
            pass_entry.grab_focus();

            let response = dialog.run();

            let username = user_entry.text().to_string();
            let password = pass_entry.text().to_string();

            dialog.close();

            if response != gtk::ResponseType::Ok {
              log::info!("[ShiftBrowser] 启动锁屏取消，退出应用");
              std::process::exit(0);
            }

            match tauri::async_runtime::block_on(users::verify_password(
              &db_state, &username, &password,
            )) {
              Ok(true) => {
                log::info!("[ShiftBrowser] 启动锁屏验证通过: {}", username);
                // 验证通过后清除会话：启动锁屏只验证身份，不自动登录个人中心
                if let Err(e) = tauri::async_runtime::block_on(users::clear_session(&db_state)) {
                  log::warn!("[ShiftBrowser] 启动锁屏验证通过但会话清除失败: {}", e);
                }
                // 显式 destroy 锁屏 dialog，避免 close 后残留；forget 阻止 drop 再次释放
                unsafe {
                  dialog.destroy();
                }
                std::mem::forget(dialog);
                // 立即恢复主窗口前台/焦点——否则用户必须点一下窗体才能交互
                // （modal dialog 关闭后焦点不自动回到父窗口）
                if let Some(ref pw) = parent_win {
                  pw.present();
                }
                // 解锁完成：清除锁屏标志（前端收到 app-unlocked 后开始初始化）
                LOCKED.store(false, std::sync::atomic::Ordering::SeqCst);
                break;
              }
              Ok(false) => {
                log::warn!("[ShiftBrowser] 启动锁屏密码错误，重新输入");
              }
              Err(e) => {
                log::error!("[ShiftBrowser] 启动锁屏验证出错: {}", e);
              }
            }
          }
        }
      }

      // 初始化标签管理器
      app.manage(tabs::TabManager::new());
      log::info!("[ShiftBrowser] 标签管理器初始化成功");

      // 用户脚本注入缓存（启动时刷新，确保首个页面注入前就绪——inject 路径只读缓存，不能 await）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::db::userscripts::refresh_cache(&db.pool));
        log::info!("[ShiftBrowser] 用户脚本注入缓存已刷新");
      }

      // 权限白名单载入内存（安全 #2 · 2026-09-22）
      {
        let db = app.state::<crate::db::Db>();
        tauri::async_runtime::block_on(crate::perms::load_allowlist(&db.pool));
      }
      // 开发者工具开关载入（安全 #6 · 2026-09-22，默认关）
      {
        let db = app.state::<crate::db::Db>();
        if let Ok(Some((v,))) = tauri::async_runtime::block_on(async {
          sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'enable_devtools'")
            .fetch_optional(&db.pool)
            .await
        }) {
          crate::engine::webview::DEVTOOLS_ENABLED
            .store(v == "true", std::sync::atomic::Ordering::Relaxed);
        }
      }

      // 定时任务调度器（阶段3 自动化：设置驱动的后台任务）
      auto_tasks::spawn(app.handle().clone());

      // 本地开放 API（阶段4 开放生态：Unix 域套接字 + 行式 JSON）
      api::spawn(app.handle().clone());

      // Linux：手动设置 WebKit web extensions 目录（tauri://localhost 协议依赖 web extension）
      // Tauri 2 不会自动设置 extensions_path，必须手动指定，否则前端资源加载失败
      #[cfg(target_os = "linux")]
      {
        use webkit2gtk::WebContextExt;
        use webkit2gtk::WebContext;
        if let Some(context) = WebContext::default() {
          if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
              // 打包兜底：web extensions 目录 = exe 目录；打包后扩展 .so 进的是资源目录，
              // 若 exe 目录没有就复制过去（开发/便携运行下 .so 本就在 exe 目录，跳过）。
              let target = dir.join("libshift_web_extension.so");
              if !target.exists() {
                if let Ok(res) = app.path().resource_dir() {
                  let src = res.join("libshift_web_extension.so");
                  if src.exists() {
                    match std::fs::copy(&src, &target) {
                      Ok(_) => {
                        log::info!("[ShiftBrowser] 扩展 .so 已从资源目录复制到 exe 目录")
                      }
                      Err(e) => log::warn!("[ShiftBrowser] 扩展 .so 复制失败: {}", e),
                    }
                  }
                }
              }
              context.set_web_extensions_directory(&dir.display().to_string());
              log::info!("[ShiftBrowser] web extensions 目录: {}", dir.display());
            }
          }
          // 注意：不要尝试 set_process_model(SharedSecondaryProcess)——
          // 1) 主 webview 在 setup 前已创建，进程池已固化，设置被 g_return_if_fail 静默忽略；
          // 2) 实测强行切换会触发 WebKit bug（WebLoaderStrategy internallyFailedLoad），整个应用崩。
          // 多标签开销靠"切标签前 hide 旧 + 前端连点合并"缓解，进程模型保持默认。

          // 隐私防护：第三方 Cookie 阻断 —— 仅"严格档"启用（2026-09-18 修订）
          // 背景：全量阻断曾导致邮箱/网盘等内容空白（站点靠第三方域加载正文/附件），
          // 故不再默认开启；改为绑定隐私防护档位：用户主动选"严格"才阻断，
          // 属知情选择。Cookie 策略是 WebContext 级、须在建 webview 前设置，
          // 因此切换档位后需重启浏览器生效。
          #[cfg(target_os = "linux")]
          {
            use webkit2gtk::CookieManagerExt;
            let level: String = tauri::async_runtime::block_on(async {
              let db = app.state::<Db>();
              let row: Option<(String,)> = sqlx::query_as(
                "SELECT value FROM settings WHERE key = 'privacy_shield_level'",
              )
              .fetch_optional(&db.pool)
              .await
              .ok()
              .flatten();
              row.map(|r| r.0).unwrap_or_else(|| "standard".to_string())
            });
            if level == "strict" {
              if let Some(cm) = context.cookie_manager() {
                cm.set_accept_policy(webkit2gtk::CookieAcceptPolicy::NoThirdParty);
                log::info!("[privacy] 严格档：第三方 Cookie 已阻断（重启后按档位生效）");
              }
            } else {
              log::info!("[privacy] 标准档：第三方 Cookie 保持浏览器默认（不阻断）");
            }
          }
        }
      }

      // Linux/GTK：初始化嵌入式 webview 的绝对定位布局（Fixed 容器 + 主 webview reparent）
      #[cfg(target_os = "linux")]
      {
        match tabs::setup_gtk_layout(app) {
          Ok(()) => log::info!("[ShiftBrowser] GTK 布局初始化成功"),
          Err(e) => log::error!("[ShiftBrowser] GTK 布局初始化失败: {}", e),
        }
      }

      // 引擎层：注册内容过滤器（网络层规则拦截）的缓存目录与应用句柄。
      // 必须在下面的注入流程之前注册：规则编译由注入流程触发（在主线程进行），
      // 编译完成后会自动重新注入，把过滤器挂到已打开标签上；
      // 加/删自定义关键词因此无需重启即可生效。
      {
        let cache_dir = app_data_dir.join("content-filters");
        engine::content_filter::register_app(app.handle().clone(), cache_dir);
      }

      // 启动时注入所有启用扩展（UserContentManager 用户脚本/样式）
      {
        let app_handle = app.handle().clone();
        let db_state = app.state::<Db>();
        extensions::inject_on_startup(&app_handle, &db_state);
      }

      // 启动时生成新标签页（左侧栏面板生态）——异步生成，不阻塞主线程首绘
      // 前端 newtab_url 在文件不存在时会自行触发 generate（newtab.rs），有兜底
      {
        let app_handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
          let db_state = app_handle.state::<Db>();
          if let Some(path) = newtab::newtab_path(&app_handle) {
            match newtab::generate(&db_state, &path).await {
              Ok(p) => log::info!("[ShiftBrowser] 新标签页已生成: {}", p),
              Err(e) => log::error!("[ShiftBrowser] 新标签页生成失败: {}", e),
            }
          }
        });
      }

      // 防御：GNOME/Xwayland 偶发把新窗口以 Iconic（最小化）状态挂起，导致窗口不映射上屏。
      // 启动 1 秒后强制 unminimize + show + focus 一次
      {
        let app_win = app.handle().clone();
        tauri::async_runtime::spawn(async move {
          // 用 tokio 异步 sleep，不占用阻塞线程（std::thread::sleep 会占一个 worker）
          tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
          if let Some(w) = app_win.get_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            log::info!("[ShiftBrowser] 窗口已强制恢复/前置");
          }
        });
      }

      // 资讯订阅调度器（62 内置源，启动 6 秒后首轮，之后每 30 分钟）
      feed::start_scheduler(app.handle());

      // 网络状态监测（状态栏：下行/上行/延迟，每 2 秒推送）
      netmon::start_network_monitor(app.handle().clone());

      // 锁屏解锁过 → 通知前端重试初始化（锁屏期间 Db/TabManager 未就绪，
      // 主窗口 Vue 的 onMounted 初始化全部失败，解锁后必须重跑 loadTabs/createTab）
      if unlocked_after_lock {
        let _ = app.emit("app-unlocked", ());
        log::info!("[ShiftBrowser] 已发出 app-unlocked，前端将重试初始化");
      }

      // ===== 系统托盘（关闭 = 缩托盘）=====
      // 见 tray.rs。ksni 菜单闭包跑在 D-Bus 线程，只往 mpsc 投动作，
      // 由下面的 glib 定时器在 GTK 主线程排空执行（绝不跨线程碰 GTK）。
      {
        let (tray_tx, tray_rx) = std::sync::mpsc::channel::<tray::TrayAction>();

        // 起托盘服务（ksni 的 spawn 是 async，交给 tauri runtime）
        tauri::async_runtime::spawn(async move {
          use ksni::TrayMethods;
          match tray::ShiftTray::new(tray_tx).spawn().await {
            Ok(handle) => {
              log::info!("[tray] 系统托盘已启动");
              // Handle 必须存活到进程结束，drop 掉托盘图标就没了
              std::mem::forget(handle);
            }
            Err(e) => log::warn!("[tray] 托盘启动失败（无宿主扩展？）: {}", e),
          }
        });

        // 关窗 = 缩托盘：拦截主窗口 delete-event
        if let Some(w) = app.get_window("main") {
          if let Ok(gtk_win) = w.gtk_window() {
            use gtk::prelude::*;
            let app_de = app.handle().clone();
            gtk_win.connect_delete_event(move |_, _| {
              use std::sync::atomic::Ordering;
              if tray::REAL_QUIT.load(Ordering::SeqCst) {
                // 真退出：放行，窗口关闭 → 应用退出
                gtk::glib::Propagation::Proceed
              } else {
                // 缩托盘：隐藏窗口，阻止关闭
                if let Some(win) = app_de.get_window("main") {
                  let _ = win.hide();
                }
                log::info!("[tray] 窗口已缩到托盘（未退出）");
                gtk::glib::Propagation::Stop
              }
            });
          }
        }

        // GTK 主线程排空托盘动作（120ms 一轮，开销可忽略）
        let app_tray = app.handle().clone();
        gtk::glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
          use std::sync::atomic::Ordering;
          while let Ok(action) = tray_rx.try_recv() {
            match action {
              tray::TrayAction::ToggleWindow => {
                if let Some(w) = app_tray.get_window("main") {
                  if w.is_visible().unwrap_or(false) {
                    let _ = w.hide();
                  } else {
                    let _ = w.show();
                    let _ = w.set_focus();
                  }
                }
              }
              tray::TrayAction::OpenDownloads => {
                let _ = app_tray.emit("tray-open-downloads", ());
              }
              tray::TrayAction::OpenPasswords => {
                let _ = app_tray.emit("tray-open-passwords", ());
              }
              tray::TrayAction::Quit => {
                tray::REAL_QUIT.store(true, Ordering::SeqCst);
                if let Some(w) = app_tray.get_window("main") {
                  let _ = w.close();
                }
              }
            }
          }
          gtk::glib::ControlFlow::Continue
        });
      }

      // 外部 URL 打开（默认浏览器入口）：命令行带 http/https 参数时（desktop 文件
      // Exec=shift-browser %U），启动后延迟打开新标签——等前端初始化完（onMounted
      // 会建初始标签），避免与初始标签竞争
      #[cfg(target_os = "linux")]
      {
        let urls: Vec<String> = std::env::args()
          .skip(1)
          .filter(|a| a.starts_with("http://") || a.starts_with("https://"))
          .collect();
        if !urls.is_empty() {
          log::info!("[default-browser] 外部链接参数 {} 个", urls.len());
          let app_handle = app.handle().clone();
          tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            for url in urls {
              let state = app_handle.state::<TabManager>();
              match tabs::create_tab(app_handle.clone(), state, url.clone()).await {
                Ok(tab) => log::info!("[default-browser] 外部链接已打开: {}", tab.id),
                Err(e) => log::warn!("[default-browser] 外部链接打开失败: {} ({})", redact_url(&url), e),
              }
            }
          });
        }
      }

      Ok(())
    })
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_whitelist_accepts_normal() {
        assert!(valid_callback("cb_1"));
        assert!(valid_callback("a"));
        assert!(valid_callback("_"));
        assert!(valid_callback("x".repeat(64).as_str()));
    }

    #[test]
    fn callback_whitelist_rejects_injection() {
        // 恶意 callback 注入 JS：拼进变量名 eval 到特权 webview
        assert!(!valid_callback(""));
        assert!(!valid_callback("x".repeat(65).as_str()));
        assert!(!valid_callback("cb;alert(1)//"));
        assert!(!valid_callback("cb'));alert(1);//"));
        assert!(!valid_callback("cb\"));alert(1);//"));
        assert!(!valid_callback("cb\nalert(1)"));
        assert!(!valid_callback("cb-1"));
        assert!(!valid_callback("cb.1"));
        assert!(!valid_callback("中文"));
    }
}

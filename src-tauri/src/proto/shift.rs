// shift:// 自定义协议处理 + 翻译 IPC（R2-1b 拆分：从 lib.rs 抽出）
//
// 本文件包含三块紧密耦合的逻辑：
// 1. shift:// 协议响应（main 前端资源 / newtab / worldnav / 未知路径 404）；
// 2. shift://__translate__ 翻译 IPC（text/progress/whole-page/batch，旧通道）；
// 3. 原生桥翻译 native_translate_handle（CSP 免疫新通道，extensions.rs 调用）。
// 三块共享：SHIFT_CSP / nonce 注入 / valid_callback / write_translate_result /
// translate_rate_limited / PROGRESS_THROTTLE / WHOLE_PAGE_TRANSLATE_JS。

use tauri::Emitter;
use tauri::Manager;

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
/// （命令实现见 tabs.rs 的 translate_current_page —— 那里可访问 TabManager 内部字段）
pub(crate) const WHOLE_PAGE_TRANSLATE_JS: &str = include_str!("../whole_page_translate.js");

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
        .or_default();
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
pub(crate) fn valid_callback(cb: &str) -> bool {
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
                let (engine, timeout) = match app_clone.try_state::<crate::db::Db>() {
                    Some(db) => (
                        crate::translate::current_engine(db.inner()).await,
                        crate::translate::current_timeout(db.inner()).await,
                    ),
                    None => ("google".to_string(), 15),
                };
                let payload =
                    match crate::translate::translate_with(&text, "zh-CN", &engine, timeout).await
                    {
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
                let (engine, target, timeout) = match app_clone.try_state::<crate::db::Db>() {
                    Some(db) => (
                        crate::translate::current_engine(db.inner()).await,
                        crate::translate::current_target(db.inner()).await,
                        crate::translate::current_timeout(db.inner()).await,
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
                                crate::translate::translate_with(&t, &tgt, &eng, timeout).await
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
                let engine = match app_p.try_state::<crate::db::Db>() {
                    Some(db) => crate::translate::current_engine(db.inner()).await,
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
                let _ = app_p.emit(crate::contracts::events::TRANSLATE_PROGRESS, payload);
            });
        }
        _ => log::warn!("[translate-native] 未知路径: {}", path),
    }
}

/// shift:// 自定义协议响应（register_uri_scheme_protocol 的闭包体，R2-1b 抽出）。
///
/// - `shift://main` → dist 前端资源（asset_resolver 读嵌入资源，替换 invoke_key）
/// - `shift://__translate__/*` → 翻译 IPC（旧通道，同步返回 image/gif 空响应）
/// - `shift://newtab|worldnav/*` → 新标签页/世界导航页 HTML（读 app_data 落盘文件）
pub(crate) fn handle(
    ctx: &tauri::UriSchemeContext<'_, tauri::Wry>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
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
                let mut html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
                let mut csp = SHIFT_CSP.to_string();
                // release：内联 IPC 桥改 per-request nonce 授权，CSP 去掉 'unsafe-inline'；
                // debug（tauri dev）：Vite HMR/eval 依赖宽松 script-src，保持模板原样
                if !cfg!(debug_assertions) {
                    let nonce = fresh_script_nonce();
                    html = inject_script_nonce(&html, &nonce);
                    csp = csp_with_script_nonce(&nonce);
                }
                return tauri::http::Response::builder()
                    .header(
                        tauri::http::header::CONTENT_TYPE,
                        "text/html; charset=utf-8",
                    )
                    .header(tauri::http::header::CONTENT_SECURITY_POLICY, csp)
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
                    let (engine, timeout) = match app_clone.try_state::<crate::db::Db>() {
                        Some(db) => (
                            crate::translate::current_engine(db.inner()).await,
                            crate::translate::current_timeout(db.inner()).await,
                        ),
                        None => ("google".to_string(), 15),
                    };
                    let payload = match crate::translate::translate_with(
                        &text, "zh-CN", &engine, timeout,
                    )
                    .await
                    {
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
                    let engine = match app_p.try_state::<crate::db::Db>() {
                        Some(db) => crate::translate::current_engine(db.inner()).await,
                        None => "google".to_string(),
                    };
                    let num =
                        |k: &str| p.get(k).and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
                    let payload = serde_json::json!({
                        "state": p.get("state").cloned().unwrap_or_else(|| "update".to_string()),
                        "engine": if engine == "bing" { "Bing" } else { "Google" },
                        "pct": num("pct"),
                        "info": p.get("info").cloned().unwrap_or_default(),
                        "elapsed": num("elapsed"),
                        "eta": num("eta"),
                        "fails": num("fails"),
                    });
                    let _ = app_p.emit(crate::contracts::events::TRANSLATE_PROGRESS, payload);
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
                            let payload =
                                serde_json::json!({ "error": format!("脚本注入失败: {}", e) });
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
                    let (engine, target, timeout) = match app_clone.try_state::<crate::db::Db>() {
                        Some(db) => (
                            crate::translate::current_engine(db.inner()).await,
                            crate::translate::current_target(db.inner()).await,
                            crate::translate::current_timeout(db.inner()).await,
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
                                    crate::translate::translate_with(&t, &tgt, &eng, timeout).await
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
        let mut html = html.replace("__INVOKE_KEY_PLACEHOLDER__", app.invoke_key());
        let mut csp = SHIFT_CSP.to_string();
        // release：newtab/worldnav 内联脚本改 per-request nonce 授权（同上，dev 保持原样）
        if !cfg!(debug_assertions) {
            let nonce = fresh_script_nonce();
            html = inject_script_nonce(&html, &nonce);
            csp = csp_with_script_nonce(&nonce);
        }
        tauri::http::Response::builder()
            .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
            // 2026-09-19：WebKit 对自定义协议默认缓存（刷新走缓存不重新请求），
            // 导致改启动页后高哥刷新看不到变化——强制 no-store，每次加载都读最新文件
            .header(
                tauri::http::header::CACHE_CONTROL,
                "no-store, no-cache, must-revalidate",
            )
            .header(tauri::http::header::PRAGMA, "no-cache")
            .header(tauri::http::header::CONTENT_SECURITY_POLICY, csp)
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
}
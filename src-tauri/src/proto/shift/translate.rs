//! 翻译 IPC 通道（R1 拆分 · 2026-09-24）：
//! 速率限制/节流、callback 白名单、结果回写，以及 CSP 免疫的原生桥处理。
//! 旧 shift:// 通道的协议分支在 [`super::protocol`]。

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use tauri::{Emitter, Manager};

use super::WHOLE_PAGE_TRANSLATE_JS;

/// translate-progress 事件节流器：同一 webview 至少间隔 300ms 才转发一次，
/// 防整页翻译高频上报刷爆 UI 事件通道（2026-09-19 安全修复）
pub(super) static PROGRESS_THROTTLE: std::sync::OnceLock<
    std::sync::Mutex<HashMap<String, Instant>>,
> = std::sync::OnceLock::new();

/// 翻译请求速率限制器：同一 webview 每分钟最多 30 次翻译请求，
/// 防恶意网页批量驱动浏览器外发翻译（2026-09-19）
static TRANSLATE_RATE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, VecDeque<Instant>>>> =
    std::sync::OnceLock::new();

/// 返回 true = 超频应拒绝
pub(super) fn translate_rate_limited(label: &str) -> bool {
    const MAX: usize = 30;
    const WINDOW: std::time::Duration = std::time::Duration::from_secs(60);
    let cell =
        TRANSLATE_RATE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = cell.lock().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    let q = map.entry(label.to_string()).or_default();
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
pub(super) fn percent_decode(s: &str) -> String {
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
pub(super) fn write_translate_result(
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
                let (engine, dual_mode, timeout) = match app_clone.try_state::<crate::db::Db>() {
                    Some(db) => (
                        crate::translate::current_engine(db.inner()).await,
                        crate::translate::current_dual_mode(db.inner()).await,
                        crate::translate::current_timeout(db.inner()).await,
                    ),
                    None => ("google".to_string(), "single".to_string(), 15),
                };
                let payload = match crate::translate::translate_with(
                    &text, "zh-CN", &engine, &dual_mode, timeout,
                )
                .await
                {
                    Ok(t) => serde_json::json!({ "text": t }),
                    // e.to_string()：CommandError 序列化是 {code,message} 对象，
                    // 注入 JS 只认字符串线格式（R4-1 类型化成对改）
                    Err(e) => serde_json::json!({ "error": e.to_string() }),
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
                        let payload =
                            serde_json::json!({ "error": format!("参数解析失败: {}", e) });
                        write_translate_result(&app_clone, &label, &callback, &payload);
                        return;
                    }
                };
                if texts.len() > 20 || texts.iter().any(|t| t.len() > 2000) {
                    let payload = serde_json::json!({ "error": "批量翻译参数超限（≤20 段、每段 ≤2000 字符）" });
                    write_translate_result(&app_clone, &label, &callback, &payload);
                    return;
                }
                let mut out: Vec<String> = vec![String::new(); texts.len()];
                let (engine, target, dual_mode, timeout) = match app_clone.try_state::<crate::db::Db>() {
                    Some(db) => (
                        crate::translate::current_engine(db.inner()).await,
                        crate::translate::current_target(db.inner()).await,
                        crate::translate::current_dual_mode(db.inner()).await,
                        crate::translate::current_timeout(db.inner()).await,
                    ),
                    None => ("google".to_string(), "zh-CN".to_string(), "single".to_string(), 15),
                };
                for chunk_start in (0..texts.len()).step_by(5) {
                    let chunk_end = (chunk_start + 5).min(texts.len());
                    let mut handles = Vec::new();
                    for (offset, t) in texts[chunk_start..chunk_end].iter().enumerate() {
                        let i = chunk_start + offset;
                        let t = t.clone();
                        let eng = engine.clone();
                        let tgt = target.clone();
                        let dm = dual_mode.clone();
                        handles.push((
                            i,
                            tauri::async_runtime::spawn(async move {
                                crate::translate::translate_with(&t, &tgt, &eng, &dm, timeout).await
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

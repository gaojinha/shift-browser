//! 页面内容/状态读取 handler（R1 三次拆分 · 2026-09-25）：
//! read_page（固定采集脚本的多模式读取）与 read_status（只读就绪态元信息）。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

use super::util::{bool_arg, eval_value, target_label};

pub(crate) async fn read_page(app: &AppHandle, req: &Value) -> Value {
    // 页面读取（固定采集脚本，非任意 JS；方案见 docs/开放API-页面读取能力完善方案_2026-09-25.md）
    // 脱敏与截断在 page_read.js 内完成；Rust 侧做参数硬顶、超时、4MB 尺寸兜底
    let mode = req.get("mode").and_then(|v| v.as_str()).unwrap_or("text");
    if !matches!(
        mode,
        "text" | "elements" | "links" | "images" | "tables" | "forms" | "article" | "html"
    ) {
        return AppError::validation(format!(
            "未知模式: {mode}（可选 text/elements/links/images/tables/forms/article/html）"
        ))
        .error_json();
    }
    let selector = req
        .get("selector")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if selector.chars().count() > 1024 {
        return AppError::validation("selector 过长（>1024 字符）").error_json();
    }
    let max_chars = req
        .get("max_chars")
        .and_then(|v| v.as_u64())
        .unwrap_or(200_000)
        .min(2_000_000);
    let max_items = req
        .get("max_items")
        .and_then(|v| v.as_u64())
        .unwrap_or(500)
        .min(5000);
    let label = match target_label(app, req) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    // 固定脚本 + 占位符注入（selector 经 js_escape；mode 已白名单校验，无注入面）
    // P1b 开关（B3 展开 details / B5 穿透 shadow / B4 并入同源 iframe）
    let expand = bool_arg(req, "expand");
    let shadow = bool_arg(req, "shadow");
    let iframe = bool_arg(req, "iframe");
    // B1 懒加载：读前先分步滚到底，触发懒加载（最多 8 步，每步 300ms）
    if bool_arg(req, "lazy") {
        let scroll_js = "(function(){var d=document.documentElement;var h=d?d.scrollHeight:0;\
            var y=window.scrollY+window.innerHeight;window.scrollTo(0,Math.min(h,y));\
            return JSON.stringify({y:Math.round(window.scrollY),h:h});})()";
        for _ in 0..8 {
            match eval_value(&wv, scroll_js, 3).await {
                Some(v) => {
                    let y = v.get("y").and_then(|n| n.as_i64()).unwrap_or(0);
                    let h = v.get("h").and_then(|n| n.as_i64()).unwrap_or(0);
                    if h > 0 && y + 5 >= h {
                        break;
                    }
                }
                None => break,
            }
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        }
    }
    let script = include_str!("../../../page_read.js")
        .replace("__MODE__", mode)
        .replace("__SELECTOR__", &crate::api::js_escape(&selector))
        .replace("__MAX_CHARS__", &max_chars.to_string())
        .replace("__MAX_ITEMS__", &max_items.to_string())
        .replace("__EXPAND__", if expand { "1" } else { "0" })
        .replace("__SHADOW__", if shadow { "1" } else { "0" })
        .replace("__IFRAME__", if iframe { "1" } else { "0" });
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("采集调度失败").error_json();
    }
    let raw = match tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::READ_PAGE_SECS),
        rx,
    )
    .await
    {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => {
            return AppError::internal(format!("采集脚本执行失败: {e}")).error_json()
        }
        Ok(Err(_)) => return AppError::internal("采集结果丢失").error_json(),
        Err(_) => {
            return AppError::timeout(format!(
                "采集超时（{} 秒），页面可能过大或卡死",
                crate::contracts::timeout::READ_PAGE_SECS
            ))
            .error_json()
        }
    };
    let v: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return AppError::internal(format!("采集结果解析失败: {e}")).error_json(),
    };
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        return AppError::validation(err).error_json();
    }
    // article：JS 侧只回传 HTML，readability 正文提取在 Rust 侧 spawn_blocking 执行
    // （CPU 密集，参照 reader.rs::fetch_and_extract；不能跑在 async 线程上）
    if mode == "article" {
        let Some(h) = v.get("html").and_then(|h| h.as_str()) else {
            return AppError::internal("获取页面 HTML 失败").error_json();
        };
        let page_url = v
            .get("url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let url_obj = match url::Url::parse(&page_url) {
            Ok(u) => u,
            Err(_) => return AppError::internal("页面 URL 不合法").error_json(),
        };
        let html_bytes = h.as_bytes().to_vec();
        let extract = tokio::task::spawn_blocking(move || {
            let mut buf = html_bytes.as_slice();
            readability::extractor::extract(&mut buf, &url_obj)
        });
        let product = match tokio::time::timeout(
            std::time::Duration::from_secs(crate::contracts::timeout::READ_PAGE_SECS),
            extract,
        )
        .await
        {
            Ok(Ok(Ok(p))) => p,
            Ok(Ok(Err(e))) => {
                return AppError::internal(format!("正文提取失败: {e}")).error_json()
            }
            Ok(Err(_)) => return AppError::internal("正文提取线程异常").error_json(),
            Err(_) => return AppError::timeout("正文提取超时").error_json(),
        };
        if product.text.is_empty() && product.content.is_empty() {
            return AppError::validation("未提取到正文，页面可能不是文章页").error_json();
        }
        let text = product.text.chars().take(max_chars as usize).collect::<String>();
        let content = product
            .content
            .chars()
            .take(max_chars as usize)
            .collect::<String>();
        let data = json!({
            "mode": "article",
            "url": page_url,
            "title": product.title,
            "text": text,
            "content": content,
            "truncated": product.text.len() > text.len() || product.content.len() > content.len(),
        });
        let resp = serde_json::to_vec(&json!({"ok": true, "data": data})).unwrap_or_default();
        if resp.len() > 4_000_000 {
            return AppError::validation("内容过大（>4MB），请调小 max_chars").error_json();
        }
        return json!({"ok": true, "data": data});
    }
    if let Some(h) = v.get("html").and_then(|h| h.as_str()) {
        if h.len() > 4_000_000 {
            return AppError::validation("内容过大（>4MB），请缩小 selector 范围").error_json();
        }
    }
    let resp = serde_json::to_vec(&json!({"ok": true, "data": v})).unwrap_or_default();
    if resp.len() > 4_000_000 {
        // 行式协议中间截断会产生非法 JSON——放弃响应体，返回干净错误
        return AppError::validation("内容过大（>4MB），请缩小 selector 或调小 max_chars")
            .error_json();
    }
    json!({"ok": true, "data": v})
}

/// 读取标签页状态与脱敏元信息（纯读，不返回页面内容）。
///
/// 方案：docs/开放API-页面读取能力完善方案_2026-09-25.md（P0 · read_status）
/// - 语义：给调用方判断"页面是否就绪、加载到哪一步"，是 open_and_read 等加载的依赖部件；
/// - 固定脚本（page_status.js）只取就绪态/结构提示，不读任何 DOM 值；
/// - `Risk::Read`（registry 已登记）→ api_readonly 下天然放行（P2 只读语义落地）。
pub(crate) async fn read_status(app: &AppHandle, req: &Value) -> Value {
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let all = state.all_tabs();
    // tab_id 为 id 的别名（与 read_page/action/* 参数名统一；id 保留向后兼容，
    // tab_id 优先——对齐 shot.rs 截图的既有口径）
    let target: Option<&crate::tabs::Tab> = req
        .get("tab_id")
        .and_then(|v| v.as_str())
        .or_else(|| req.get("id").and_then(|v| v.as_str()))
        .map(|id| all.iter().find(|t| t.id == id))
        .unwrap_or_else(|| all.iter().find(|t| t.is_active));
    let Some(target) = target else {
        return AppError::validation("指定标签不存在（先 list_tabs 取 id；缺省为活动标签）")
            .error_json();
    };
    let tab_id = target.id.clone();
    let url = target.url.clone();
    let title = target.title.clone();
    let is_active = target.is_active;
    let hibernated = target.hibernated;
    let private = target.private;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // 休眠标签无 webview：直接返回元信息，不做采集
    if hibernated {
        return json!({"ok": true, "data": {
            "tab_id": tab_id, "url": url, "title": title,
            "is_active": is_active, "private": private, "hibernated": true,
            "ts": ts, "ready_state": "hibernated", "status": "hibernated",
            "hints": ["标签已休眠，webview 已销毁（切换标签可重建）"], "counts": {}
        }});
    }

    let label = format!("tab-{}", tab_id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    // 固定状态采集脚本（非任意 JS）：只取就绪态/结构提示，不读任何 DOM 值
    let script = include_str!("../../../page_status.js");
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("状态采集调度失败").error_json();
    }
    let raw = match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => {
            return AppError::internal(format!("状态采集脚本执行失败: {e}")).error_json()
        }
        Ok(Err(_)) => return AppError::internal("状态采集结果丢失").error_json(),
        Err(_) => return AppError::timeout("状态采集超时（2 秒）").error_json(),
    };
    let probe: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    if let Some(err) = probe.get("error").and_then(|e| e.as_str()) {
        return AppError::validation(err).error_json();
    }
    let ready_state = probe
        .get("ready_state")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let status = if ready_state == "complete" {
        "loaded"
    } else {
        "loading"
    };
    json!({"ok": true, "data": {
        "tab_id": tab_id, "url": url, "title": title,
        "is_active": is_active, "private": private, "hibernated": false,
        "ts": ts,
        "ready_state": ready_state,
        "status": status,
        "has_body": probe.get("has_body").cloned().unwrap_or(json!(false)),
        "hints": probe.get("hints").cloned().unwrap_or(json!([])),
        "counts": probe.get("counts").cloned().unwrap_or(json!({})),
    }})
}

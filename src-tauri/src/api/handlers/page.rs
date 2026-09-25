//! 页面交互 handler：截图 / 受控 JS 执行 / 页面内容读取

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

pub(super) async fn screenshot(app: &AppHandle, req: &Value) -> Value {
    // 截图指定标签（缺省 = 当前活动标签；webkit snapshot → PNG），返回落盘路径
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let all = state.all_tabs();
    let target: Option<&crate::tabs::Tab> = req
        .get("id")
        .and_then(|v| v.as_str())
        .map(|id| all.iter().find(|t| t.id == id))
        .unwrap_or_else(|| all.iter().find(|t| t.is_active));
    let Some(target) = target else {
        return AppError::validation("指定标签不存在（先 list_tabs 取 id；缺省为活动标签）")
            .error_json();
    };
    let label = format!("tab-{}", target.id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    let out_path = {
        let db = app.state::<crate::db::Db>();
        let dir = db
            .vault_path
            .parent()
            .map(|p| p.join("screenshots"))
            .unwrap_or_default();
        let _ = std::fs::create_dir_all(&dir);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        dir.join(format!("shot-{}.png", ts))
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let out2 = out_path.clone();
    let dispatched = wv.with_webview(move |pwv| {
        use webkit2gtk::WebViewExt;
        let gtk_wv = pwv.inner().clone();
        // GTK 对象非 Send → 在 GTK 主上下文里跑（spawn_local），结果经 oneshot 回传
        gtk::glib::MainContext::default().spawn_local(async move {
            let res = match gtk_wv
                .snapshot_future(
                    webkit2gtk::SnapshotRegion::Visible,
                    webkit2gtk::SnapshotOptions::NONE,
                )
                .await
            {
                Ok(surface) => std::fs::File::create(&out2)
                    .map_err(|e| format!("创建 PNG 失败: {}", e))
                    .and_then(|mut f| {
                        // write_to_png 收的是 Write 流（不是路径）
                        surface
                            .write_to_png(&mut f)
                            .map_err(|e| format!("写 PNG 失败: {}", e))
                    })
                    .map(|_| out2.display().to_string()),
                Err(e) => Err(format!("截图失败: {}", e)),
            };
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("截图调度失败").error_json();
    }
    match rx.await {
        Ok(Ok(path)) => json!({"ok": true, "data": {"path": path}}),
        Ok(Err(e)) => AppError::internal(e).error_json(),
        Err(_) => AppError::internal("截图结果丢失").error_json(),
    }
}

pub(super) async fn eval(app: &AppHandle, req: &Value) -> Value {
    // 执行 JS（安全闸门：设置 api_allow_eval 必须为 'true'，默认关）
    let db = app.state::<crate::db::Db>();
    let allowed: String = sqlx::query_as::<_, (String,)>(
        "SELECT value FROM settings WHERE key = 'api_allow_eval'",
    )
    .fetch_optional(&db.pool)
    .await
    .ok()
    .flatten()
    .map(|(v,)| v)
    .unwrap_or_default();
    if allowed != "true" {
        return AppError::permission("执行 JS 已禁用：需在设置里显式开启「允许 API 执行 JS」")
            .error_json();
    }
    let code = req
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if code.is_empty() {
        return AppError::validation("缺少 code").error_json();
    }
    if code.len() > 100_000 {
        return AppError::validation("代码过长（>100KB）").error_json();
    }
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let Some(active) = state.all_tabs().into_iter().find(|t| t.is_active) else {
        return AppError::validation("没有活动标签").error_json();
    };
    let label = format!("tab-{}", active.id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到活动标签 webview").error_json();
    };
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &code, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("执行调度失败").error_json();
    }
    match rx.await {
        Ok(Ok(v)) => json!({"ok": true, "data": {"result": v}}),
        Ok(Err(e)) => AppError::internal(e).error_json(),
        Err(_) => AppError::internal("执行结果丢失").error_json(),
    }
}

pub(super) async fn read_page(app: &AppHandle, req: &Value) -> Value {
    // 页面读取（固定采集脚本，非任意 JS；方案见 docs/read_page-命令方案_2026-09-24.md）
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
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let Some(active) = state.all_tabs().into_iter().find(|t| t.is_active) else {
        return AppError::validation("没有活动标签（read_page 一期只支持活动标签）").error_json();
    };
    if let Some(tid) = req.get("tab_id").and_then(|v| v.as_str()) {
        if !tid.is_empty() {
            return AppError::validation(
                "tab_id 暂不支持：read_page 一期只读活动标签（先 switch_tab）",
            )
            .error_json();
        }
    }
    let label = format!("tab-{}", active.id);
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到活动标签 webview").error_json();
    };
    // 固定脚本 + 占位符注入（selector 经 js_escape；mode 已白名单校验，无注入面）
    let script = include_str!("../../page_read.js")
        .replace("__MODE__", mode)
        .replace("__SELECTOR__", &crate::api::js_escape(&selector))
        .replace("__MAX_CHARS__", &max_chars.to_string())
        .replace("__MAX_ITEMS__", &max_items.to_string());
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

//! 页面交互 handler：截图 / 受控 JS 执行 / 页面内容读取

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 移动版 UA（M 类响应式读取）：`open_and_read` 传 `ua:"mobile"` 时启用，
/// 便于读取移动版更简洁的页面（闲鱼/淘宝等）。版本与默认 UA 对齐。
const MOBILE_UA: &str = "Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/142.0.0.0 Mobile Safari/537.36";

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
        Ok(Ok(path)) => {
            // 截图治理（O2）：目录只保留最近 N 张，防止 AI 大量截图堆爆磁盘
            if let Some(dir) = out_path.parent() {
                prune_screenshots(dir, SCREENSHOT_KEEP);
            }
            json!({"ok": true, "data": {"path": path}})
        }
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
    let script = include_str!("../../page_read.js")
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
pub(super) async fn read_status(app: &AppHandle, req: &Value) -> Value {
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
    let script = include_str!("../../page_status.js");
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

/// 取布尔参数（true/false、"true"/"1"、数字 1 均可；缺省 false）
fn bool_arg(req: &Value, key: &str) -> bool {
    match req.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(Value::String(s)) => matches!(s.as_str(), "1" | "true" | "yes"),
        _ => false,
    }
}

/// 解析目标标签的 webview label（`tab_id` 优先，缺省活动标签）。
/// 供 read_page / extract 等按标签读取的命令复用。
pub(super) fn target_label(app: &AppHandle, req: &Value) -> Result<String, Value> {
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return Err(AppError::internal("标签管理器不可用").error_json());
    };
    let all = state.all_tabs();
    let target = match req.get("tab_id").and_then(|v| v.as_str()) {
        Some(tid) if !tid.is_empty() => all.iter().find(|t| t.id == tid).ok_or_else(|| {
            AppError::validation(format!("标签不存在: {tid}（先 list_tabs 取 id）")).error_json()
        })?,
        _ => all
            .iter()
            .find(|t| t.is_active)
            .ok_or_else(|| AppError::validation("没有活动标签（可传 tab_id 指定标签）").error_json())?,
    };
    Ok(format!("tab-{}", target.id))
}

/// open_and_read 的内网 SSRF 守卫（入口 + 最终 URL 复核共用）。
///
/// 返回 `Ok(())` 放行；`Err(响应)` 为拒绝（非 http/https、或内网地址且未进白名单）。
async fn guard_url(app: &AppHandle, url: &str) -> Result<(), Value> {
    let Ok(parsed) = tauri::Url::parse(url) else {
        return Err(AppError::validation("URL 解析失败").error_json());
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(AppError::permission(format!(
            "open_and_read 只接受 http/https（收到 {}）",
            parsed.scheme()
        ))
        .error_json());
    }
    let Some(host) = parsed.host_str().map(|h| h.to_string()) else {
        return Err(AppError::validation("URL 缺少主机名").error_json());
    };
    // 内网 / 私有地址：查库取白名单（同时刷新进程内缓存，供 GTK 导航守卫同步读取）
    let allowlist: String = {
        let db = app.state::<crate::db::Db>();
        sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'api_allowed_ips'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_default()
    };
    crate::net_guard::set_allowlist(&allowlist);
    if crate::net_guard::host_permitted(&host) {
        Ok(())
    } else {
        Err(AppError::permission(format!(
            "内网 / 私有地址默认拒绝：{host}（如确需读取，请在 api_allowed_ips 白名单中显式放行）"
        ))
        .error_json())
    }
}

/// 在给定 webview 上执行脚本并取回解析后的 JSON（lazy 滚动扫掠用）。
async fn eval_value(wv: &tauri::Webview, script: &str, timeout_secs: u64) -> Option<Value> {
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let s = script.to_string();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), &s, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return None;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), rx).await {
        Ok(Ok(Ok(raw))) => serde_json::from_str::<Value>(&raw).ok(),
        _ => None,
    }
}

/// 单次探测标签状态（open_and_read 等加载 / 最终 URL 复核用）。返回 page_status.js 的完整 JSON。
async fn probe_page(app: &AppHandle, tab_id: &str) -> Option<Value> {
    let wv = app.get_webview(&format!("tab-{}", tab_id))?;
    let script = include_str!("../../page_status.js");
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return None;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
        Ok(Ok(Ok(raw))) => serde_json::from_str::<Value>(&raw).ok(),
        _ => None,
    }
}

/// 打开 URL 并读取页面（P0 杀招命令 · 方案 §4.1）。
///
/// 一次调用完成「开标签 → 等加载 → 读页面」，返回统一结构（tab_id/final_url/meta/read）。
/// 登记 `Risk::Read`（语义读）→ api_readonly 下放行（P2 核心拍板：能看不能动）。
/// 一期实现：SSRF 内网守卫 + 等就绪 + 读取 + `close` + `reuse`；`ua` / 重定向逐跳校验留待后续切片。
pub(super) async fn open_and_read(app: &AppHandle, req: &Value) -> Value {
    let url = req
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if url.is_empty() {
        return AppError::validation("缺少 url").error_json();
    }
    if let Err(resp) = guard_url(app, &url).await {
        return resp;
    }
    let close_after = req.get("close").and_then(|v| v.as_bool()).unwrap_or(false);
    let reuse = req.get("reuse").and_then(|v| v.as_bool()).unwrap_or(false);

    // 1) 决定目标标签：reuse 命中已有同 URL 的 AI 标签 → 直接复用；否则开新标签
    let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
        return AppError::internal("标签管理器不可用").error_json();
    };
    let reused_id = if reuse {
        state
            .all_tabs()
            .into_iter()
            .find(|t| t.origin == "ai" && t.url == url)
            .map(|t| t.id)
    } else {
        None
    };
    let (tab_id, reused) = match reused_id {
        Some(id) => (id, true),
        None => {
            // AI 标签上限（J 类）：默认 10，超限拒绝（提示先 tabs/close_ai 回收），防标签堆爆
            const AI_TAB_LIMIT: usize = 10;
            if state.ai_tab_count() >= AI_TAB_LIMIT {
                return AppError::permission(format!(
                    "AI 标签已达上限 {AI_TAB_LIMIT}（先调用 tabs/close_ai 回收再试）"
                ))
                .error_json();
            }
            let tab = match crate::tabs::commands::create_tab(
                app.clone(),
                state,
                url.clone(),
                None,
                None,
                Some(true), // J6：AI 标签后台打开，不抢用户焦点
            )
            .await
            {
                Ok(t) => t,
                Err(e) => return AppError::from(e).error_json(),
            };
            let id = tab.id.clone();
            // 标记 AI 来源（J 类）：供 tabs/close_ai 回收、AI 上限计数、看板区分
            app.state::<crate::tabs::TabManager>().set_origin(&id, "ai");
            (id, false)
        }
    };

    // ua 覆盖（M 类响应式）：mobile 时切移动版 UA 并重载，读移动版（闲鱼/淘宝类更简洁）
    let ua_mode = req.get("ua").and_then(|v| v.as_str()).unwrap_or("desktop");
    let ua_mobile = ua_mode == "mobile";
    if ua_mobile {
        if let Some(wv) = app.get_webview(&format!("tab-{}", tab_id)) {
            let ua = MOBILE_UA.to_string();
            let _ = wv.with_webview(move |pwv| {
                use webkit2gtk::{SettingsExt, WebViewExt};
                if let Some(settings) = pwv.inner().settings() {
                    settings.set_user_agent(Some(&ua));
                }
            });
            if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
                let _ = crate::tabs::commands::reload_tab(app.clone(), st, tab_id.clone()).await;
            }
        }
    }

    // 2) 等加载就绪：复用标签视为已就绪（探测一次）；新开标签轮询 readyState（最多 ~8s）
    let mut ready_state = "unknown".to_string();
    let mut final_url = url.clone();
    if reused {
        if let Some(p) = probe_page(app, &tab_id).await {
            ready_state = p
                .get("ready_state")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown")
                .to_string();
            if let Some(u) = p.get("url").and_then(|s| s.as_str()) {
                if !u.is_empty() {
                    final_url = u.to_string();
                }
            }
        }
    } else {
        for _ in 0..40 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            if let Some(p) = probe_page(app, &tab_id).await {
                let rs = p
                    .get("ready_state")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                if let Some(u) = p.get("url").and_then(|s| s.as_str()) {
                    if !u.is_empty() {
                        final_url = u.to_string();
                    }
                }
                ready_state = rs.clone();
                if rs == "complete" {
                    break;
                }
            }
        }
    }

    // 2b) O1 重定向逐跳/最终 URL 复核：加载后的真实 URL 再跑一遍 SSRF 守卫。
    //     公开页跳转到内网（重定向绕过入口校验）→ 读取前拦截并回收，绝不让内网内容外流。
    //     注意：仅当 final_url 是 http/https 且与入口不同才复核——新标签未加载完时为
    //     `about:blank`（非 http），不应触发拦截（修复 2026-09-25：曾误伤所有读取）。
    let final_is_http = final_url.starts_with("http://") || final_url.starts_with("https://");
    if final_is_http && final_url != url {
        if let Err(resp) = guard_url(app, &final_url).await {
            // 复核失败：回收刚开的标签（复用标签不关，避免误关用户标签）
            if !reused {
                if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
                    let _ = crate::tabs::commands::close_tab(app.clone(), st, tab_id.clone()).await;
                }
            }
            let msg = resp
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("重定向到内网地址被拦截");
            return AppError::permission(format!(
                "重定向复核拦截（{final_url}）：{msg}"
            ))
            .error_json();
        }
    }

    // 3) 读取（复用 read_page，显式指定刚开的 tab_id，避免活动标签漂移读错）
    let mut read_req = json!({
        "mode": req.get("mode").cloned().unwrap_or(json!("text")),
        "selector": req.get("selector").cloned().unwrap_or(json!("")),
        "tab_id": tab_id.clone(),
        "max_chars": req.get("max_chars").cloned().unwrap_or(json!(200000)),
        "max_items": req.get("max_items").cloned().unwrap_or(json!(500)),
    });
    if let Some(obj) = read_req.as_object_mut() {
        obj.insert("cmd".to_string(), json!("read_page"));
    }
    let read_resp = read_page(app, &read_req).await;
    let read_ok = read_resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);

    // 4) 可选关闭
    if close_after {
        if let Some(st) = app.try_state::<crate::tabs::TabManager>() {
            let _ = crate::tabs::commands::close_tab(app.clone(), st, tab_id.clone()).await;
        }
    }

    // final_url：优先用 read_page 采集到的真实 URL（探测时可能还是 about:blank）
    let final_url_resp = {
        let from_read = read_resp
            .get("data")
            .and_then(|d| d.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("");
        if from_read.starts_with("http") {
            from_read.to_string()
        } else {
            final_url.clone()
        }
    };
    let data = json!({
        "tab_id": tab_id,
        "url": url,
        "final_url": final_url_resp,
        "title": read_resp.get("data").and_then(|d| d.get("title")).cloned().unwrap_or(json!("")),
        "ready_state": ready_state,
        "closed": close_after,
        "reused": reused,
        "ua": if ua_mobile { "mobile" } else { "desktop" },
        "read_ok": read_ok,
        "content_type": "page",
        "read": if read_ok { read_resp.get("data").cloned().unwrap_or(Value::Null) } else { Value::Null },
        "read_error": if read_ok { Value::Null } else { read_resp.get("error").cloned().unwrap_or(Value::Null) },
    });
    json!({"ok": true, "data": data})
}

/// 截图保留上限（O2）：超过则按时间淘汰最旧。
const SCREENSHOT_KEEP: usize = 100;

/// 截图文件筛选：只认 `shot-<定长毫秒时间戳>.png`（字典序 = 时间序）。
fn is_screenshot_name(name: &str) -> bool {
    name.starts_with("shot-") && name.ends_with(".png")
}

/// 截图治理（O2）：目录内只保留最近 `keep` 张 `shot-*.png`，多余按文件名（= 时间戳）淘汰最旧。
/// 失败静默（治理是尽力而为，不影响截图主流程）。
fn prune_screenshots(dir: &std::path::Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut shots: Vec<std::path::PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(is_screenshot_name)
                .unwrap_or(false)
        })
        .collect();
    if shots.len() <= keep {
        return;
    }
    shots.sort();
    let remove_n = shots.len() - keep;
    for p in shots.into_iter().take(remove_n) {
        let _ = std::fs::remove_file(p);
    }
}

/// `action/dismiss`：关闭常见弹窗（B8/H3）。保守识别（浮层内 close 语义/白名单文案），
/// 识别不了返回提示，**绝不盲点**。属交互（改变页面状态）→ Risk::Write，受 api_readonly 约束。
pub(super) async fn dismiss(app: &AppHandle, req: &Value) -> Value {
    let label = match target_label(app, req) {
        Ok(l) => l,
        Err(resp) => return resp,
    };
    let Some(wv) = app.get_webview(&label) else {
        return AppError::internal("找不到标签 webview").error_json();
    };
    let script = include_str!("../../page_dismiss.js");
    let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
    let dispatched = wv.with_webview(move |pwv| {
        crate::engine::webview::eval_js_with(&pwv.inner(), script, move |res| {
            let _ = tx.send(res);
        });
    });
    if dispatched.is_err() {
        return AppError::internal("弹窗识别调度失败").error_json();
    }
    let raw = match tokio::time::timeout(std::time::Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(v))) => v,
        Ok(Ok(Err(e))) => return AppError::internal(format!("弹窗识别执行失败: {e}")).error_json(),
        Ok(Err(_)) => return AppError::internal("弹窗识别结果丢失").error_json(),
        Err(_) => return AppError::timeout("弹窗识别超时").error_json(),
    };
    match serde_json::from_str::<Value>(&raw) {
        Ok(v) => json!({"ok": true, "data": v}),
        Err(e) => AppError::internal(format!("弹窗识别结果解析失败: {e}")).error_json(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 截图治理（O2）：超出 keep 张时按时间淘汰最旧，非 shot-*.png 不受影响。
    #[test]
    fn prune_screenshots_keeps_newest() {
        let dir = std::env::temp_dir().join(format!("shift-shot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 造 5 张截图（时间戳递增）+ 1 个无关文件
        for ts in [1000u128, 2000, 3000, 4000, 5000] {
            std::fs::write(dir.join(format!("shot-{ts}.png")), b"x").unwrap();
        }
        std::fs::write(dir.join("keep.txt"), b"x").unwrap();
        // 只留最近 3 张
        prune_screenshots(&dir, 3);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec!["keep.txt", "shot-3000.png", "shot-4000.png", "shot-5000.png"],
            "应淘汰最旧两张、保留无关文件"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_screenshot_name_filters_non_shots() {
        assert!(is_screenshot_name("shot-1727000000000.png"));
        assert!(!is_screenshot_name("keep.txt"));
        assert!(!is_screenshot_name("shot-1.jpg"));
        assert!(!is_screenshot_name("readme.md"));
    }
}

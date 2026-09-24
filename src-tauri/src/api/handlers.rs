// 本地 API 命令分发（R2-1c 拆分：从 api.rs 的 handle() match 拆出）
//
// 每个命令分支保持原逻辑不动；依赖 api.rs 的 url_encode/js_escape 辅助函数
// （super:: 访问），其余 crate 内部引用一律全路径。
// 后续可进一步把本文件按领域拆成多个 handler 文件。

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// 命令分发：解析后的请求 JSON + cmd 字符串 → 响应 JSON。
/// （api.rs 的 handle() 负责 JSON 解析与只读模式闸门，这里只做命令分发）
pub(crate) async fn dispatch(app: &AppHandle, req: &Value, cmd: &str) -> Value {
    match cmd {
        // 能力清单（单一来源：capabilities.rs）——MCP Server 启动时拉取，
        // 避免其内部再手写一份 schema（历史问题：25 工具曾三处各写一遍）
        "capabilities/list" => json!({ "ok": true, "data": crate::capabilities::list_json() }),

        // 技能清单（单一来源：contracts::skills，与前端生成物同源；P4）
        "skills/list" => json!({ "ok": true, "data": crate::capabilities::skills_list_json() }),

        // 插件清单（四形态统一视图：用户脚本 / 内容过滤 / Web 扩展 / MCP 服务器）
        "plugins/list" => {
            use crate::capabilities::plugins::{entry, PluginKind};
            let db = app.state::<crate::db::Db>();
            let mut list: Vec<serde_json::Value> = Vec::new();

            // 用户脚本
            if let Ok(rows) = sqlx::query_as::<_, (i64, String, i64)>(
                "SELECT id, name, enabled FROM userscripts ORDER BY id",
            )
            .fetch_all(&db.pool)
            .await
            {
                for (id, name, enabled) in rows {
                    list.push(entry(
                        PluginKind::UserScript,
                        &id.to_string(),
                        &name,
                        enabled != 0,
                        "注入页面",
                    ));
                }
            }

            // MCP 服务器
            if let Ok(rows) = sqlx::query_as::<_, (i64, String, i64)>(
                "SELECT id, name, enabled FROM mcp_servers ORDER BY id",
            )
            .fetch_all(&db.pool)
            .await
            {
                for (id, name, enabled) in rows {
                    list.push(entry(
                        PluginKind::McpServer,
                        &id.to_string(),
                        &name,
                        enabled != 0,
                        "外部工具服务器",
                    ));
                }
            }

            // 内容过滤（广告拦截）
            let adblock_on = sqlx::query_as::<_, (String,)>(
                "SELECT value FROM settings WHERE key = 'adblock_enabled'",
            )
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .map(|(v,)| v != "false")
            .unwrap_or(true);
            list.push(entry(
                PluginKind::ContentFilter,
                "adblock",
                "广告与追踪拦截",
                adblock_on,
                "内置规则 + 自定义关键词",
            ));

            // Web 扩展（编译期加载）
            list.push(entry(
                PluginKind::WebExtension,
                "shift-web-extension",
                "网络层拦截扩展",
                true,
                "编译期 .so，随应用加载",
            ));

            let count = list.len();
            json!({ "ok": true, "data": { "plugins": list, "count": count } })
        }

        // 插件启停（四形态统一入口；Web 扩展为编译期加载，不可运行时启停）
        "plugins/set_enabled" => {
            let kind = req.get("kind").and_then(|v| v.as_str()).unwrap_or("");
            let id = req.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let enabled = req.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
            let db = app.state::<crate::db::Db>();
            let flag: i64 = if enabled { 1 } else { 0 };
            let num_id: i64 = id.parse().unwrap_or(-1);
            let res = match kind {
                "userscript" => {
                    sqlx::query("UPDATE userscripts SET enabled = ? WHERE id = ?")
                        .bind(flag)
                        .bind(num_id)
                        .execute(&db.pool)
                        .await
                }
                "mcp_server" => {
                    sqlx::query("UPDATE mcp_servers SET enabled = ? WHERE id = ?")
                        .bind(flag)
                        .bind(num_id)
                        .execute(&db.pool)
                        .await
                }
                "content_filter" => {
                    sqlx::query(
                        "INSERT INTO settings (key, value) VALUES ('adblock_enabled', ?)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    )
                    .bind(if enabled { "true" } else { "false" })
                    .execute(&db.pool)
                    .await
                }
                "web_extension" => {
                    return json!({ "ok": false, "error": "Web 扩展为编译期加载，不支持运行时启停" })
                }
                other => return json!({ "ok": false, "error": format!("未知插件形态: {}", other) }),
            };
            match res {
                Ok(r) => {
                    // 统一审计：插件启停记入 tool_audit_log（与 MCP 工具调用同一张表，
                    // server_name='plugin' 区分来源；后续审计面板一屏看全部能力调用）
                    let _ = sqlx::query(
                        "INSERT INTO tool_audit_log
                         (server_id, server_name, tool_name, arguments, result_summary, success)
                         VALUES (NULL, 'plugin', 'set_enabled', ?, ?, 1)",
                    )
                    .bind(
                        serde_json::json!({ "kind": kind, "id": id, "enabled": enabled })
                            .to_string(),
                    )
                    .bind(format!("affected={}", r.rows_affected()))
                    .execute(&db.pool)
                    .await;
                    json!({ "ok": true, "data": {
                        "kind": kind, "id": id, "enabled": enabled, "affected": r.rows_affected()
                    }})
                }
                Err(e) => json!({ "ok": false, "error": format!("启停失败: {}", e) }),
            }
        }
        "status" => {
            let tabs = app
                .try_state::<crate::tabs::TabManager>()
                .map(|s| s.all_tabs().len())
                .unwrap_or(0);
            json!({"ok": true, "data": {"version": env!("CARGO_PKG_VERSION"), "tabs": tabs}})
        }
        "list_tabs" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            json!({"ok": true, "data": state.all_tabs()})
        }
        "open_url" => {
            let url = req
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if url.is_empty() {
                return json!({"ok": false, "error": "缺少 url"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::commands::create_tab(app.clone(), state, url, None, None).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "close_tab" | "switch_tab" | "reload_tab" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() {
                return json!({"ok": false, "error": "缺少 id"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let r = match cmd {
                "close_tab" => crate::tabs::commands::close_tab(app.clone(), state, id).await,
                "switch_tab" => crate::tabs::commands::switch_tab(app.clone(), state, id).await,
                _ => crate::tabs::commands::reload_tab(app.clone(), state, id).await,
            };
            match r {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "navigate" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let url = req
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if id.is_empty() || url.is_empty() {
                return json!({"ok": false, "error": "缺少 id 或 url"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::commands::navigate_tab(app.clone(), state, id, url).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "search" => {
            let q = req
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if q.is_empty() {
                return json!({"ok": false, "error": "缺少 query"});
            }
            // 读 search_engine 设置，拼搜索 URL（与 newtab/AddressBar 同映射）
            let engine: String = {
                let db = app.state::<crate::db::Db>();
                sqlx::query_as::<_, (String,)>(
                    "SELECT value FROM settings WHERE key = 'search_engine'",
                )
                .fetch_optional(&db.pool)
                .await
                .ok()
                .flatten()
                .map(|(v,)| v)
                .unwrap_or_else(|| crate::constants::search::DEFAULT_ENGINE.to_string())
            };
            // 搜索引擎 URL 模板来自 constants.rs（唯一来源，未知 id 自动回退默认引擎）
            let template = crate::constants::search::url_template(&engine);
            let url = format!("{}{}", template, super::url_encode(&q));
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::commands::create_tab(app.clone(), state, url, None, None).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "new_tab" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            // 注意：Rust 侧 create_tab 需要绝对 URL，"newtab" 会解析失败 → 用 shift://newtab
            match crate::tabs::commands::create_tab(
                app.clone(),
                state,
                crate::contracts::urls::NEWTAB.to_string(),
                None,
                None,
            )
            .await
            {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "history" => {
            let limit = req
                .get("limit")
                .and_then(|v| v.as_i64())
                .unwrap_or(50)
                .clamp(1, 500);
            let db = app.state::<crate::db::Db>();
            let rows = sqlx::query_as::<_, (String, Option<String>, i64, String)>(
                "SELECT url, title, visit_count, last_visit_at FROM history
                 ORDER BY last_visit_at DESC LIMIT ?",
            )
            .bind(limit)
            .fetch_all(&db.pool)
            .await
            .unwrap_or_default();
            let items: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(url, title, vc, lv)| {
                    json!({"url": url, "title": title, "visit_count": vc, "last_visit_at": lv})
                })
                .collect();
            json!({"ok": true, "data": items})
        }
        "bookmarks" => {
            let db = app.state::<crate::db::Db>();
            let rows = sqlx::query_as::<_, (String, String, Option<String>, i64)>(
                "SELECT b.title, b.url, f.name, b.sort_order FROM bookmarks b
                 LEFT JOIN bookmark_folders f ON f.id = b.folder_id ORDER BY b.id",
            )
            .fetch_all(&db.pool)
            .await
            .unwrap_or_default();
            let items: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(title, url, folder, so)| {
                    json!({"title": title, "url": url, "folder": folder, "sort_order": so})
                })
                .collect();
            json!({"ok": true, "data": items})
        }
        "screenshot" => {
            // 截图指定标签（缺省 = 当前活动标签；webkit snapshot → PNG），返回落盘路径
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let all = state.all_tabs();
            let target: Option<&crate::tabs::Tab> = req
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| all.iter().find(|t| t.id == id))
                .unwrap_or_else(|| all.iter().find(|t| t.is_active));
            let Some(target) = target else {
                return json!({"ok": false, "error": "指定标签不存在（先 list_tabs 取 id；缺省为活动标签）"});
            };
            let label = format!("tab-{}", target.id);
            let Some(wv) = app.get_webview(&label) else {
                return json!({"ok": false, "error": "找不到标签 webview"});
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
                return json!({"ok": false, "error": "截图调度失败"});
            }
            match rx.await {
                Ok(Ok(path)) => json!({"ok": true, "data": {"path": path}}),
                Ok(Err(e)) => json!({"ok": false, "error": e}),
                Err(_) => json!({"ok": false, "error": "截图结果丢失"}),
            }
        }
        "eval" => {
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
                return json!({"ok": false, "error": "执行 JS 已禁用：需在设置里显式开启「允许 API 执行 JS」"});
            }
            let code = req
                .get("code")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if code.is_empty() {
                return json!({"ok": false, "error": "缺少 code"});
            }
            if code.len() > 100_000 {
                return json!({"ok": false, "error": "代码过长（>100KB）"});
            }
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let Some(active) = state.all_tabs().into_iter().find(|t| t.is_active) else {
                return json!({"ok": false, "error": "没有活动标签"});
            };
            let label = format!("tab-{}", active.id);
            let Some(wv) = app.get_webview(&label) else {
                return json!({"ok": false, "error": "找不到活动标签 webview"});
            };
            let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
            let dispatched = wv.with_webview(move |pwv| {
                crate::engine::webview::eval_js_with(&pwv.inner(), &code, move |res| {
                    let _ = tx.send(res);
                });
            });
            if dispatched.is_err() {
                return json!({"ok": false, "error": "执行调度失败"});
            }
            match rx.await {
                Ok(Ok(v)) => json!({"ok": true, "data": {"result": v}}),
                Ok(Err(e)) => json!({"ok": false, "error": e}),
                Err(_) => json!({"ok": false, "error": "执行结果丢失"}),
            }
        }
        "read_page" => {
            // 页面读取（固定采集脚本，非任意 JS；方案见 docs/read_page-命令方案_2026-09-24.md）
            // 脱敏与截断在 page_read.js 内完成；Rust 侧做参数硬顶、超时、4MB 尺寸兜底
            let mode = req
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("text");
            if !matches!(mode, "text" | "elements" | "links" | "images" | "tables" | "forms" | "article" | "html") {
                return json!({"ok": false, "error": format!("未知模式: {mode}（可选 text/elements/links/images/tables/forms/article/html）")});
            }
            let selector = req
                .get("selector")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if selector.chars().count() > 1024 {
                return json!({"ok": false, "error": "selector 过长（>1024 字符）"});
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
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let Some(active) = state.all_tabs().into_iter().find(|t| t.is_active) else {
                return json!({"ok": false, "error": "没有活动标签（read_page 一期只支持活动标签）"});
            };
            if let Some(tid) = req.get("tab_id").and_then(|v| v.as_str()) {
                if !tid.is_empty() {
                    return json!({"ok": false, "error": "tab_id 暂不支持：read_page 一期只读活动标签（先 switch_tab）"});
                }
            }
            let label = format!("tab-{}", active.id);
            let Some(wv) = app.get_webview(&label) else {
                return json!({"ok": false, "error": "找不到活动标签 webview"});
            };
            // 固定脚本 + 占位符注入（selector 经 js_escape；mode 已白名单校验，无注入面）
            let script = include_str!("../page_read.js")
                .replace("__MODE__", mode)
                .replace("__SELECTOR__", &super::js_escape(&selector))
                .replace("__MAX_CHARS__", &max_chars.to_string())
                .replace("__MAX_ITEMS__", &max_items.to_string());
            let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
            let dispatched = wv.with_webview(move |pwv| {
                crate::engine::webview::eval_js_with(&pwv.inner(), &script, move |res| {
                    let _ = tx.send(res);
                });
            });
            if dispatched.is_err() {
                return json!({"ok": false, "error": "采集调度失败"});
            }
            let raw = match tokio::time::timeout(
                std::time::Duration::from_secs(crate::contracts::timeout::READ_PAGE_SECS),
                rx,
            )
            .await
            {
                Ok(Ok(Ok(v))) => v,
                Ok(Ok(Err(e))) => return json!({"ok": false, "error": format!("采集脚本执行失败: {e}")}),
                Ok(Err(_)) => return json!({"ok": false, "error": "采集结果丢失"}),
                Err(_) => return json!({"ok": false, "error": format!("采集超时（{} 秒），页面可能过大或卡死", crate::contracts::timeout::READ_PAGE_SECS)}),
            };
            let v: Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => return json!({"ok": false, "error": format!("采集结果解析失败: {e}")}),
            };
            if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                return json!({"ok": false, "error": err});
            }
            // article：JS 侧只回传 HTML，readability 正文提取在 Rust 侧 spawn_blocking 执行
            // （CPU 密集，参照 reader.rs::fetch_and_extract；不能跑在 async 线程上）
            if mode == "article" {
                let Some(h) = v.get("html").and_then(|h| h.as_str()) else {
                    return json!({"ok": false, "error": "获取页面 HTML 失败"});
                };
                let page_url = v
                    .get("url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                let url_obj = match url::Url::parse(&page_url) {
                    Ok(u) => u,
                    Err(_) => return json!({"ok": false, "error": "页面 URL 不合法"}),
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
                    Ok(Ok(Err(e))) => return json!({"ok": false, "error": format!("正文提取失败: {e}")}),
                    Ok(Err(_)) => return json!({"ok": false, "error": "正文提取线程异常"}),
                    Err(_) => return json!({"ok": false, "error": "正文提取超时"}),
                };
                if product.text.is_empty() && product.content.is_empty() {
                    return json!({"ok": false, "error": "未提取到正文，页面可能不是文章页"});
                }
                let text = product.text.chars().take(max_chars as usize).collect::<String>();
                let content = product.content.chars().take(max_chars as usize).collect::<String>();
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
                    return json!({"ok": false, "error": "内容过大（>4MB），请调小 max_chars"});
                }
                return json!({"ok": true, "data": data});
            }
            if let Some(h) = v.get("html").and_then(|h| h.as_str()) {
                if h.len() > 4_000_000 {
                    return json!({"ok": false, "error": "内容过大（>4MB），请缩小 selector 范围"});
                }
            }
            let resp = serde_json::to_vec(&json!({"ok": true, "data": v})).unwrap_or_default();
            if resp.len() > 4_000_000 {
                // 行式协议中间截断会产生非法 JSON——放弃响应体，返回干净错误
                return json!({"ok": false, "error": "内容过大（>4MB），请缩小 selector 或调小 max_chars"});
            }
            json!({"ok": true, "data": v})
        }
        "tools" => {
            // 工具网关（阶段4 自托管套件）：列出可用 MCP 工具
            let db = app.state::<crate::db::Db>();
            match crate::engine::mcp::list_tools(&db.pool).await {
                Ok(tools) => {
                    let items: Vec<serde_json::Value> = tools
                        .into_iter()
                        .map(|t| {
                            json!({
                                "name": t.name,
                                "server_name": t.server_name,
                                "description": t.description,
                                "input_schema": t.input_schema,
                            })
                        })
                        .collect();
                    json!({"ok": true, "data": items})
                }
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "tool_call" => {
            // 工具网关：调用一个 MCP 工具（受 MCP server 的 allowed_tools 白名单约束）
            let name = req
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if name.is_empty() {
                return json!({"ok": false, "error": "缺少 name"});
            }
            let args = req.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let db = app.state::<crate::db::Db>();
            match crate::engine::mcp::call_tool(&db.pool, &name, args).await {
                Ok(r) => json!({"ok": true, "data": {"result": r}}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        // ===== 只读扩展命令（阶段4 C2：更多只读命令）=====
        "list_downloads" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::downloads::get_downloads(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list_sessions" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::sessions::list_sessions(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list_groups" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::tab_groups::get_tab_groups(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list_spaces" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::workspaces::get_workspaces(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list_userscripts" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::userscripts::list_userscripts(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "search_history" => {
            let query = req
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if query.is_empty() {
                return json!({"ok": false, "error": "缺少 query"});
            }
            let limit = req.get("limit").and_then(|v| v.as_i64());
            let db = app.state::<crate::db::Db>();
            match crate::db::history::search_history(db, query, limit).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "get_setting" => {
            let key = req
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if key.is_empty() {
                return json!({"ok": false, "error": "缺少 key"});
            }
            let db = app.state::<crate::db::Db>();
            match crate::db::settings::get_setting(db, key).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        "list_settings" => {
            let db = app.state::<crate::db::Db>();
            match crate::db::settings::get_all_settings(db).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e.to_string()}),
            }
        }
        // ===== 操作命令（写；只读模式下被拒）=====
        "pin_tab" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::commands::toggle_pin(state, id).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "mute_tab" => {
            let id = req
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::commands::toggle_mute(app.clone(), state, id).await {
                Ok(v) => json!({"ok": true, "data": v}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "minimize_window" => match crate::tabs::commands::minimize_window(app.clone()).await {
            Ok(_) => json!({"ok": true, "data": null}),
            Err(e) => json!({"ok": false, "error": e}),
        },
        "maximize_window" => match crate::tabs::commands::toggle_maximize(app.clone()).await {
            Ok(v) => json!({"ok": true, "data": v}),
            Err(e) => json!({"ok": false, "error": e}),
        },
        "active_tab" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            let active = state.all_tabs().into_iter().find(|t| t.is_active);
            json!({"ok": true, "data": active})
        }
        _ => json!({"ok": false, "error": format!("未知命令: {}", cmd)}),
    }
}


//! 本地开放 API（阶段 4 · 开放生态）
//!
//! 让外部程序（脚本/工具）驱动浏览器。选 **Unix 域套接字 + 行式 JSON**：
//! - 套接字文件权限 0600（仅本用户可连），不做网络监听，天然本地隔离；
//! - 协议：每行一个 JSON 请求 → 每行一个 JSON 响应。
//!
//! 命令：
//! - `{"cmd":"status"}`                     → `{"ok":true,"data":{"version":…,"tabs":N}}`
//! - `{"cmd":"list_tabs"}`                  → `{"ok":true,"data":[Tab…]}`
//! - `{"cmd":"open_url","url":"https://…"}` → `{"ok":true,"data":Tab}`
//!
//! 用法示例：
//! ```bash
//! printf '{"cmd":"list_tabs"}\n' | socat - UNIX-CONNECT:~/.local/share/com.shiftbrowser.app/api.sock
//! ```

use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

/// 套接字路径（应用数据目录，与 vault.key/sync.key 同级）
pub fn socket_path(app: &AppHandle) -> Option<PathBuf> {
    let db = app.try_state::<crate::db::Db>()?;
    Some(db.vault_path.with_file_name("api.sock"))
}

/// 最小 percent-encode（查询串用；不引额外依赖）
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 起 API 服务（lib.rs setup 调用）
pub fn spawn(app: AppHandle) {
    let Some(path) = socket_path(&app) else {
        log::warn!("[api] 无法确定套接字路径，开放 API 未启动");
        return;
    };
    // 清理上次残留
    let _ = std::fs::remove_file(&path);
    // 同时起本地 HTTP API（阶段4 自托管服务端套件最小版）——必须在下面 move app 之前克隆
    spawn_http(app.clone());
    tauri::async_runtime::spawn(async move {
        let listener = match UnixListener::bind(&path) {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[api] 绑定 {} 失败: {}", path.display(), e);
                return;
            }
        };
        // 权限 0600：仅本用户可连
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        log::info!("[api] 开放 API 已启动: {}", path.display());
        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let (r, mut w) = stream.into_split();
                        let mut lines = BufReader::new(r).lines();
                        while let Ok(Some(line)) = lines.next_line().await {
                            if line.trim().is_empty() {
                                continue;
                            }
                            let resp = handle(&app, &line).await;
                            let mut s = serde_json::to_string(&resp)
                                .unwrap_or_else(|_| "{\"ok\":false}".to_string());
                            s.push('\n');
                            if w.write_all(s.as_bytes()).await.is_err() {
                                break;
                            }
                        }
                    });
                }
                Err(e) => log::warn!("[api] accept 失败: {}", e),
            }
        }
    });

}

/// HTTP API 端口（**仅 127.0.0.1**；局域网暴露需自行改绑并保管好 token）
const HTTP_PORT: u16 = 19701;

/// 确保 API token 存在（0600 落盘，与 vault.key/sync.key 同级），返回 token
fn ensure_token(app: &AppHandle) -> Option<String> {
    let db = app.try_state::<crate::db::Db>()?;
    let path = db.vault_path.with_file_name("api-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return Some(t);
        }
    }
    let t = format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let _ = std::fs::write(&path, &t);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Some(t)
}

/// 起 HTTP API：`POST http://127.0.0.1:19701/api`，body 为同款 JSON 命令；
/// 鉴权：`Authorization: Bearer <token>`（token 见 api-token 文件）。复用 `handle()`。
/// 同步读一个设置（setup 阶段调用；setup 不在 tokio worker 上，block_on 安全）
fn read_setting_blocking(app: &AppHandle, key: &str) -> Option<String> {
    let db = app.try_state::<crate::db::Db>()?;
    tauri::async_runtime::block_on(async {
        sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten()
            .map(|(v,)| v)
    })
}

fn spawn_http(app: AppHandle) {
    let Some(token) = ensure_token(&app) else {
        return;
    };
    // 端口/绑定可配（默认 127.0.0.1:19701）；改完需重启生效
    let port: u16 = read_setting_blocking(&app, "api_http_port")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(HTTP_PORT);
    let lan = read_setting_blocking(&app, "api_bind_lan")
        .map(|v| v == "true")
        .unwrap_or(false);
    let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
    if lan {
        log::warn!("[api] HTTP 已绑 0.0.0.0（局域网可达）——请确保 token 不外泄");
    }
    tauri::async_runtime::spawn(async move {
        let addr = format!("{}:{}", host, port);
        let listener = match tokio::net::TcpListener::bind(&addr).await {
            Ok(l) => l,
            Err(e) => {
                log::warn!("[api] HTTP 绑定 {} 失败: {}", addr, e);
                return;
            }
        };
        log::info!(
            "[api] HTTP API 已启动: http://{}/api（Bearer token 见 api-token 文件）",
            addr
        );
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                continue;
            };
            let app = app.clone();
            let token = token.clone();
            tauri::async_runtime::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = vec![0u8; 1 << 16];
                let n = match stream.read(&mut buf).await {
                    Ok(n) => n,
                    Err(_) => return,
                };
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let (head, body) = match req.find("\r\n\r\n") {
                    Some(i) => (&req[..i], &req[i + 4..]),
                    None => (req.as_str(), ""),
                };
                let path = head
                    .lines()
                    .next()
                    .and_then(|l| l.split_whitespace().nth(1))
                    .unwrap_or("/")
                    .to_string();
                let auth_ok = head.lines().any(|l| {
                    l.to_ascii_lowercase().starts_with("authorization:")
                        && l.splitn(2, ':')
                            .nth(1)
                            .map(|v| {
                                v.trim().trim_start_matches("Bearer ").trim() == token.as_str()
                            })
                            .unwrap_or(false)
                });
                let resp = if path != "/api" && path != "/" {
                    json!({"ok": false, "error": "请用 POST /api 发 JSON 命令"})
                } else if !auth_ok {
                    json!({"ok": false, "error": "未授权：需要 Authorization: Bearer <token>"})
                } else {
                    handle(&app, body.trim()).await
                };
                let body_out = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".into());
                let out = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body_out.len(),
                    body_out
                );
                let _ = stream.write_all(out.as_bytes()).await;
            });
        }
    });
}

/// 处理一条请求
async fn handle(app: &AppHandle, line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("请求 JSON 非法: {}", e)}),
    };
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    match cmd {
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
            match crate::tabs::create_tab(app.clone(), state, url).await {
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
                "close_tab" => crate::tabs::close_tab(app.clone(), state, id).await,
                "switch_tab" => crate::tabs::switch_tab(app.clone(), state, id).await,
                _ => crate::tabs::reload_tab(app.clone(), state, id).await,
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
            match crate::tabs::navigate_tab(app.clone(), state, id, url).await {
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
                .unwrap_or_else(|| "bing".to_string())
            };
            let (base, param) = match engine.as_str() {
                "baidu" => ("https://www.baidu.com/s", "wd"),
                "google" => ("https://www.google.com/search", "q"),
                _ => ("https://www.bing.com/search", "q"),
            };
            let url = format!("{}?{}={}", base, param, url_encode(&q));
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            match crate::tabs::create_tab(app.clone(), state, url).await {
                Ok(t) => json!({"ok": true, "data": t}),
                Err(e) => json!({"ok": false, "error": e}),
            }
        }
        "new_tab" => {
            let Some(state) = app.try_state::<crate::tabs::TabManager>() else {
                return json!({"ok": false, "error": "标签管理器不可用"});
            };
            // 注意：Rust 侧 create_tab 需要绝对 URL，"newtab" 会解析失败 → 用 shift://newtab
            match crate::tabs::create_tab(app.clone(), state, "shift://newtab".to_string()).await {
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
            // 截图当前活动标签（webkit snapshot → PNG），返回落盘路径
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
            let allowed: String =
                sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = 'api_allow_eval'")
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

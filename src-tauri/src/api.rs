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

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

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
/// 常量时间字符串比较（安全 · 2026-09-22）：防 token 鉴权被时序侧信道逐字节推断。
/// 长度不等直接返回（token 长度固定，泄露长度无意义）。
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for i in 0..a.len() {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

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

/// IP 白名单校验：名单为空 = 不限制；否则逗号分隔，按前缀匹配
/// （如 `192.168.1.` 匹配整个 192.168.1.x 网段；`127.0.0.1` 精确匹配）
fn ip_allowed(ip: &str, allowlist: &str) -> bool {
    let allow = allowlist.trim();
    if allow.is_empty() {
        return true;
    }
    allow
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .any(|pat| ip == pat || ip.starts_with(pat))
}

fn spawn_http(app: AppHandle) {
    let Some(token) = ensure_token(&app) else {
        return;
    };
    // 端口/绑定可配（默认 127.0.0.1:19701）；改完需重启生效
    let port: u16 = read_setting_blocking(&app, "api_http_port")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(HTTP_PORT);
    let lan_want = read_setting_blocking(&app, "api_bind_lan")
        .map(|v| v == "true")
        .unwrap_or(false);
    // IP 白名单（阶段4 C2；#4 加固 2026-09-22）：LAN 暴露必须配非空白名单
    let allowed_ips = read_setting_blocking(&app, "api_allowed_ips").unwrap_or_default();
    // 安全（#4）：绑定 0.0.0.0（局域网可达）时，必须配非空 IP 白名单，否则整个局域网
    // 都能打到 API（仅靠 token 保护）。空名单则拒绝 LAN 绑定，退回仅本机。
    let lan = if lan_want && allowed_ips.trim().is_empty() {
        log::error!(
            "[api] 拒绝绑定 LAN：api_allowed_ips 为空（安全策略要求非空白名单），已退回仅本机 127.0.0.1"
        );
        false
    } else {
        lan_want
    };
    let host = if lan { "0.0.0.0" } else { "127.0.0.1" };
    if lan {
        log::info!("[api] HTTP 已绑 0.0.0.0，IP 白名单: {}", allowed_ips);
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
            let Ok((mut stream, peer)) = listener.accept().await else {
                continue;
            };
            // IP 白名单校验（空名单放行）
            let peer_ip = peer.ip().to_string();
            if !ip_allowed(&peer_ip, &allowed_ips) {
                log::warn!(
                    "[api] 拒绝非白名单 IP: {}（api_allowed_ips={}）",
                    peer_ip,
                    allowed_ips
                );
                continue;
            }
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
                                ct_eq(v.trim().trim_start_matches("Bearer ").trim(), token.as_str())
                            })
                            .unwrap_or(false)
                });
                // 模型网关（阶段4）：POST /v1/chat/completions → 转发到配置的 provider
                if path.starts_with("/v1/chat/completions") {
                    let out_json = if !auth_ok {
                        json!({"error": {"message": "未授权：需要 Authorization: Bearer <token>"}})
                    } else {
                        model_gateway(&app, body).await
                    };
                    let s = serde_json::to_string(&out_json).unwrap_or_else(|_| "{}".into());
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        s.len(),
                        s
                    );
                    let _ = stream.write_all(out.as_bytes()).await;
                    return;
                }
                // 状态面板（浏览器用）：GET /?token=<t>（浏览器不便发 Bearer 头，故支持查询串）
                if !path.starts_with("/api") {
                    let qtok = path
                        .splitn(2, '?')
                        .nth(1)
                        .unwrap_or("")
                        .split('&')
                        .find_map(|kv| {
                            let mut it = kv.splitn(2, '=');
                            let k = it.next()?;
                            let v = it.next().unwrap_or("");
                            (k == "token").then(|| v.to_string())
                        });
                    let ok = qtok.as_deref().map(|t| ct_eq(t, token.as_str())).unwrap_or(false) || auth_ok;
                    let html = if ok {
                        dashboard_html(&app).await
                    } else {
                        "<!doctype html><meta charset=utf-8><h2>ShiftBrowser API</h2><p>需要 token：在地址后加 <code>?token=&lt;api-token 文件里的值&gt;</code></p>".to_string()
                    };
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        html.len(),
                        html
                    );
                    let _ = stream.write_all(out.as_bytes()).await;
                    return;
                }
                let resp = if path != "/api" {
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

/// 状态面板 HTML（自托管套件的"管理页"，浏览器打开 `/?token=<t>`）
async fn dashboard_html(app: &AppHandle) -> String {
    let (n, active) = match app.try_state::<crate::tabs::TabManager>() {
        Some(s) => {
            let all = s.all_tabs();
            let a = all
                .iter()
                .find(|t| t.is_active)
                .map(|t| (t.title.clone(), t.url.clone()));
            (all.len(), a)
        }
        None => (0, None),
    };
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let active_html = match active {
        Some((t, u)) => format!(
            "<div><b>{}</b><br><span class=u>{}</span></div>",
            esc(&t),
            esc(&u)
        ),
        None => "<div>（无活动标签）</div>".to_string(),
    };
    let cmds = "status · list_tabs · active_tab · history · bookmarks · search_history · \
                list_downloads · list_sessions · list_groups · list_spaces · list_userscripts · \
                get_setting · list_settings · open_url · close_tab · switch_tab · reload_tab · \
                navigate · search · new_tab · screenshot · pin_tab · mute_tab · \
                minimize_window · maximize_window · eval（默认关）· tools · tool_call";
    format!(
        r#"<!doctype html><meta charset=utf-8><title>ShiftBrowser 状态</title>
<style>body{{font:14px/1.6 system-ui,sans-serif;max-width:760px;margin:32px auto;padding:0 16px;color:#222}}
h1{{font-size:20px}} .u{{color:#888;font-size:12px}} code{{background:#f2f2f2;padding:1px 5px;border-radius:4px}}
.card{{border:1px solid #ddd;border-radius:10px;padding:14px 16px;margin:12px 0}}</style>
<h1>ShiftBrowser 状态面板</h1>
<div class=card><b>版本</b>：{ver}　<b>标签数</b>：{n}</div>
<div class=card><b>当前活动标签</b>{active}</div>
<div class=card><b>开放 API 命令</b><br><code>{cmds}</code></div>
<div class=card><b>用法</b>：<code>POST /api</code> + <code>Authorization: Bearer &lt;token&gt;</code>；详见 <code>docs/local-api.md</code></div>
"#,
        ver = env!("CARGO_PKG_VERSION"),
        n = n,
        active = active_html,
        cmds = cmds,
    )
}

/// 模型网关：把 OpenAI 兼容的 chat/completions 请求转发到设置里的 provider。
/// 默认关（设置 `api_model_gateway` 需为 'true'）；复用设置里的 base_url / key / model。
async fn model_gateway(app: &AppHandle, body: &str) -> serde_json::Value {
    let db = app.state::<crate::db::Db>();
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT key, value FROM settings WHERE key IN \
         ('api_model_gateway','ai_base_url','ai_api_key','ai_model')",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap_or_default();
    let m: std::collections::HashMap<String, String> = rows.into_iter().collect();
    if m.get("api_model_gateway").map(|v| v.as_str()) != Some("true") {
        return json!({"error": {"message": "模型网关已禁用：需在设置里显式开启「允许模型网关」"}});
    }
    let base = m
        .get("ai_base_url")
        .cloned()
        .unwrap_or_else(|| "https://apihub.agnes-ai.com/v1".to_string());
    let key = m
        .get("ai_api_key")
        .cloned()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("AGNES_API_KEY").ok())
        .unwrap_or_default();
    if key.is_empty() {
        return json!({"error": {"message": "未配置 AI API Key（设置里填或设 AGNES_API_KEY）"}});
    }
    let model = m
        .get("ai_model")
        .cloned()
        .unwrap_or_else(|| "agnes-2.0-flash".to_string());

    let mut payload: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json!({"error": {"message": format!("请求 JSON 非法: {}", e)}}),
    };
    if payload.get("model").is_none() {
        if let Some(o) = payload.as_object_mut() {
            o.insert("model".to_string(), json!(model));
        }
    }
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
    {
        Ok(c) => c,
        Err(e) => return json!({"error": {"message": format!("创建 HTTP 客户端失败: {}", e)}}),
    };
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    match client.post(&url).bearer_auth(&key).json(&payload).send().await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(e) => json!({"error": {"message": format!("解析上游响应失败: {}", e)}}),
        },
        Err(e) => json!({"error": {"message": format!("上游请求失败: {}", e)}}),
    }
}

/// 写命令（改变浏览器状态）；只读模式（api_readonly=true）下统一拒绝
fn is_write_cmd(cmd: &str) -> bool {
    matches!(
        cmd,
        "open_url"
            | "close_tab"
            | "switch_tab"
            | "reload_tab"
            | "navigate"
            | "search"
            | "new_tab"
            | "eval"
            | "tool_call"
            | "pin_tab"
            | "mute_tab"
            | "minimize_window"
            | "maximize_window"
    )
}

/// 处理一条请求
async fn handle(app: &AppHandle, line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("请求 JSON 非法: {}", e)}),
    };
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    // 只读模式（api_readonly=true）：拒绝一切写命令
    if is_write_cmd(cmd) {
        let db = app.state::<crate::db::Db>();
        let ro = sqlx::query_scalar::<_, String>(
            "SELECT value FROM settings WHERE key = 'api_readonly'",
        )
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
        if ro {
            return json!({"ok": false, "error": format!("只读模式已开启，写命令 {} 被拒绝", cmd)});
        }
    }
    match cmd {
        // 能力清单（单一来源：capabilities.rs）——MCP Server 启动时拉取，
        // 避免其内部再手写一份 schema（历史问题：25 工具曾三处各写一遍）
        "capabilities/list" => json!({ "ok": true, "data": crate::capabilities::list_json() }),
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
            let url = format!("{}{}", template, url_encode(&q));
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
            match crate::tabs::commands::create_tab(app.clone(), state, crate::contracts::urls::NEWTAB.to_string(), None, None).await {
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

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

// R2-1c 拆分（2026-09-24）：命令分发（原 handle() 的 match）独立成子模块
mod handlers;
// R2-1c 拆分（2026-09-24）：状态面板 + 模型网关独立成子模块
pub(crate) mod dashboard;
// R2-1c 拆分（2026-09-24）：token/限速/IP 白名单等鉴权辅助独立成子模块
pub(crate) mod auth;

use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::RwLock;

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

/// 最小 JS 字符串转义（嵌入单引号 JS 字面量用；与 extensions/adblock.rs 先例一致）
fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// 起 API 服务（lib.rs setup 调用）
pub fn spawn(app: AppHandle) {
    let Some(path) = socket_path(&app) else {
        log::warn!("[api] 无法确定套接字路径，开放 API 未启动");
        return;
    };
    // 清理上次残留
    let _ = std::fs::remove_file(&path);
    // 同时起本地 HTTP API（阶段4 自托管服务端套件最小版）——必须在下面 move app 之前克隆。
    // token/tickets 以 Tauri state 共享：rotate_api_token / create_dashboard_ticket
    // 两个命令与 HTTP 服务操作同一份运行期状态
    if let Some(token) = auth::ensure_token(&app) {
        let token_shared = auth::ApiTokenShared(Arc::new(RwLock::new(token)));
        app.manage(token_shared.clone());
        app.manage(auth::DashboardTickets::new());
        spawn_http(app.clone(), token_shared);
    } else {
        log::warn!("[api] api-token 初始化失败，HTTP API 未启动");
    }
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

/// HTTP 请求头上限（64KB，远超正常请求头；超出按畸形请求断开）
const HTTP_MAX_HEADER: usize = 64 * 1024;
/// HTTP 请求体上限（8MB；模型网关转发长对话 chat/completions 必需）
const HTTP_MAX_BODY: usize = 8 * 1024 * 1024;

/// HTTP 请求读取结果
#[derive(Debug)]
enum HttpReq {
    /// (请求头文本[不含结尾 \r\n\r\n], body 文本)
    Complete(String, String),
    /// Content-Length 超过 [`HTTP_MAX_BODY`]
    TooLarge,
    /// 连接提前关闭 / 请求畸形 / 头部超限 / chunked 不支持
    Bad,
}

/// 从任意 AsyncRead 完整读取一个 HTTP/1.x 请求：
/// - 头部循环读到 `\r\n\r\n`（头部可能跨 TCP 包到达，旧实现只读一次必然漏读）；
/// - 按 Content-Length 把 body 读满（body 也可能跨包，或与头部在同一次 read 中到达）；
/// - 无 Content-Length 时 body 取已随头部读到的剩余字节（兼容简单客户端）。
async fn read_http_request<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> HttpReq {
    use tokio::io::AsyncReadExt;
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = match r.read(&mut chunk).await {
            Ok(0) => return HttpReq::Bad,
            Ok(n) => n,
            Err(_) => return HttpReq::Bad,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find_header_end(&buf) {
            break i;
        }
        if buf.len() > HTTP_MAX_HEADER {
            return HttpReq::Bad;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    // Transfer-Encoding: chunked 不支持（本地 API 面向程序客户端，均带 Content-Length）
    if is_chunked(&head) {
        return HttpReq::Bad;
    }
    let target_len = match content_length_of(&head) {
        Some(len) => {
            if len > HTTP_MAX_BODY {
                return HttpReq::TooLarge;
            }
            header_end.saturating_add(len)
        }
        None => buf.len(),
    };
    while buf.len() < target_len {
        let n = match r.read(&mut chunk).await {
            Ok(0) => return HttpReq::Bad,
            Ok(n) => n,
            Err(_) => return HttpReq::Bad,
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..target_len]).to_string();
    HttpReq::Complete(head, body)
}

/// 在已读字节中定位 HTTP 头部结束位置（`\r\n\r\n` 之后的偏移）
fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// 请求头是否声明 `Transfer-Encoding: chunked`
fn is_chunked(head: &str) -> bool {
    head.lines().skip(1).any(|l| {
        l.split_once(':')
            .map(|(k, v)| {
                k.trim().eq_ignore_ascii_case("transfer-encoding")
                    && v.split(',')
                        .any(|te| te.trim().eq_ignore_ascii_case("chunked"))
            })
            .unwrap_or(false)
    })
}

/// 从请求头解析 Content-Length
fn content_length_of(head: &str) -> Option<usize> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("content-length") {
            return None;
        }
        v.trim().parse().ok()
    })
}

/// dashboard 页面安全响应头：禁被嵌入任意框架（点击劫持），CSP 收紧到无脚本/无外联
const DASHBOARD_HEADERS: &str = "X-Frame-Options: DENY\r\n\
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'\r\n\
Referrer-Policy: no-referrer\r\n";

fn spawn_http(app: AppHandle, tokens: auth::ApiTokenShared) {
    // 端口/绑定可配（默认 127.0.0.1:19701）；改完需重启生效
    let port: u16 = auth::read_setting_blocking(&app, "api_http_port")
        .and_then(|v| v.trim().parse::<u16>().ok())
        .unwrap_or(HTTP_PORT);
    let lan_want = auth::read_setting_blocking(&app, "api_bind_lan")
        .map(|v| v == "true")
        .unwrap_or(false);
    // IP 白名单（阶段4 C2；#4 加固 2026-09-22）：LAN 暴露必须配非空白名单
    let allowed_ips = auth::read_setting_blocking(&app, "api_allowed_ips").unwrap_or_default();
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
    // LAN 认证失败限速状态（仅绑定 0.0.0.0 时有意义，本机绑定下 peer 全是 loopback 不计数）
    let guard = Arc::new(auth::LoginGuard::default());
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
            let peer_ip = peer.ip();
            if !auth::ip_allowed(&peer_ip.to_string(), &allowed_ips) {
                log::warn!(
                    "[api] 拒绝非白名单 IP: {}（api_allowed_ips={}）",
                    peer_ip,
                    allowed_ips
                );
                continue;
            }
            let app = app.clone();
            let tokens = tokens.clone();
            let guard = guard.clone();
            tauri::async_runtime::spawn(async move {
                // 完整读取请求（头部可能跨 TCP 包；body 按 Content-Length 读满——
                // 旧实现只读一次 64KB，长对话经模型网关转发必然 JSON 截断）
                let (head, body) = match read_http_request(&mut stream).await {
                    HttpReq::Complete(h, b) => (h, b),
                    HttpReq::TooLarge => {
                        write_http(
                            &mut stream,
                            413,
                            "Payload Too Large",
                            "application/json",
                            "",
                            json!({"error": {"message": "请求体过大（上限 8MB）"}})
                                .to_string()
                                .as_bytes(),
                        )
                        .await;
                        return;
                    }
                    HttpReq::Bad => return,
                };
                // 请求行：method 与 target（target 可能带 query；path 取问号前）
                let request_line = head.lines().next().unwrap_or("");
                let mut rl = request_line.split_whitespace();
                let method = rl.next().unwrap_or("GET");
                let target = rl.next().unwrap_or("/");
                let path = target.split('?').next().unwrap_or("/").to_string();
                // 限速闸门（HANDOFF P2-3）：封禁期先于一切凭据校验直接 429
                if let Some(retry) = guard.check(&peer_ip, Instant::now()) {
                    log::warn!("[api] peer {peer_ip} 封禁中，429（{retry}s）");
                    write_http(
                        &mut stream,
                        429,
                        "Too Many Requests",
                        "application/json",
                        &format!("Retry-After: {retry}\r\n"),
                        json!({"error": {"message": format!("认证失败过多，请 {retry} 秒后重试")}})
                            .to_string()
                            .as_bytes(),
                    )
                    .await;
                    return;
                }
                // 每请求读取当前 token（轮换后立即生效）
                let token = tokens.0.read().await.clone();
                let has_auth_header = head
                    .lines()
                    .any(|l| l.to_ascii_lowercase().starts_with("authorization:"));
                let bearer_ok = bearer_token_ok(&head, &token);
                // 模型网关（阶段4）：POST /v1/chat/completions → 转发到配置的 provider
                if path.starts_with("/v1/chat/completions") {
                    let (status, reason, extra, payload) = if bearer_ok {
                        guard.record_success(&peer_ip);
                        let s = serde_json::to_string(&dashboard::model_gateway(&app, &body).await)
                            .unwrap_or_else(|_| "{}".into());
                        (200u16, "OK", String::new(), s)
                    } else {
                        let retry = guard.record_failure(&peer_ip, Instant::now());
                        log::warn!("[api] 模型网关未授权 peer={peer_ip}（封禁 {retry}s）");
                        (
                            401,
                            "Unauthorized",
                            format!("Retry-After: {retry}\r\n"),
                            json!({"error": {"message": "未授权：需要 Authorization: Bearer <token>"}})
                                .to_string(),
                        )
                    };
                    write_http(
                        &mut stream,
                        status,
                        reason,
                        "application/json",
                        &extra,
                        payload.as_bytes(),
                    )
                    .await;
                    return;
                }
                // 状态面板（浏览器用）。凭据三种（HANDOFF P2-3 起长期 token 禁止走 URL query）：
                // 1) Bearer 头；2) GET ?ticket=<一次性门票>（应用内按钮签发，5 分钟/用后即焚）；
                // 3) POST 表单字段 token（可贴长期 token 或门票——不进历史/Referer/代理日志）
                if !path.starts_with("/api") {
                    let query_ticket = if method == "POST" {
                        None
                    } else {
                        kv_value(target.split('?').nth(1).unwrap_or(""), "ticket")
                    };
                    let posted = if method == "POST" {
                        kv_value(body.trim(), "token")
                    } else {
                        None
                    };
                    let tickets_ok = app.try_state::<auth::DashboardTickets>();
                    let ticket_ok = query_ticket.as_deref().is_some_and(|t| {
                        tickets_ok.as_ref().is_some_and(|s| s.consume(t))
                    }) || posted.as_deref().is_some_and(|t| {
                        tickets_ok.as_ref().is_some_and(|s| s.consume(t))
                    });
                    let ok = bearer_ok
                        || ticket_ok
                        || posted.as_deref().is_some_and(|t| auth::ct_eq(t, &token));
                    if ok {
                        guard.record_success(&peer_ip);
                        write_http(
                            &mut stream,
                            200,
                            "OK",
                            "text/html; charset=utf-8",
                            DASHBOARD_HEADERS,
                            dashboard::dashboard_html(&app).await.as_bytes(),
                        )
                        .await;
                        return;
                    }
                    // 只有真的提交了凭据（表单/Authorization 头/带 ticket 链接）才计失败；
                    // 裸开登录页与浏览器预取不触发限速
                    let submitted = has_auth_header
                        || query_ticket.is_some()
                        || posted.is_some();
                    let retry = if submitted {
                        guard.record_failure(&peer_ip, Instant::now())
                    } else {
                        0
                    };
                    if submitted {
                        log::warn!("[api] 状态面板凭据无效 peer={peer_ip}（封禁 {retry}s）");
                    }
                    let extra = if retry > 0 {
                        format!("Retry-After: {retry}\r\n")
                    } else {
                        String::new()
                    };
                    let err = if submitted {
                        Some("凭据无效或门票已过期")
                    } else {
                        None
                    };
                    write_http(
                        &mut stream,
                        200,
                        "OK",
                        "text/html; charset=utf-8",
                        &format!("{DASHBOARD_HEADERS}{extra}"),
                        dashboard::dashboard_login_html(err, retry).as_bytes(),
                    )
                    .await;
                    return;
                }
                let (status, reason, extra, payload) = if path != "/api" {
                    (
                        200u16,
                        "OK",
                        String::new(),
                        json!({"ok": false, "error": "请用 POST /api 发 JSON 命令"}).to_string(),
                    )
                } else if !bearer_ok {
                    let retry = guard.record_failure(&peer_ip, Instant::now());
                    log::warn!("[api] /api 未授权 peer={peer_ip}（封禁 {retry}s）");
                    (
                        401,
                        "Unauthorized",
                        format!("Retry-After: {retry}\r\n"),
                        json!({"ok": false, "error": "未授权：需要 Authorization: Bearer <token>"})
                            .to_string(),
                    )
                } else {
                    guard.record_success(&peer_ip);
                    (
                        200,
                        "OK",
                        String::new(),
                        serde_json::to_string(&handle(&app, body.trim()).await)
                            .unwrap_or_else(|_| "{}".into()),
                    )
                };
                write_http(
                    &mut stream,
                    status,
                    reason,
                    "application/json",
                    &extra,
                    payload.as_bytes(),
                )
                .await;
            });
        }
    });
}

/// 写一个完整 HTTP/1.1 响应（连接固定 close；`extra_headers` 每行需自带 `\r\n`）
async fn write_http(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    extra_headers: &str,
    body: &[u8],
) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

/// 校验 Authorization 头是否为匹配的 Bearer token（常量时间比较）
fn bearer_token_ok(head: &str, token: &str) -> bool {
    head.lines().any(|l| {
        if !l.to_ascii_lowercase().starts_with("authorization:") {
            return false;
        }
        l.splitn(2, ':')
            .nth(1)
            .map(|v| {
                let v = v.trim();
                let v = v
                    .strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
                    .unwrap_or(v)
                    .trim();
                auth::ct_eq(v, token)
            })
            .unwrap_or(false)
    })
}

/// 从 `a=b&c=d` 形式串（query 或 form body）取值并 percent-decode
fn kv_value(encoded: &str, key: &str) -> Option<String> {
    encoded.split('&').find_map(|kv| {
        let mut it = kv.splitn(2, '=');
        let k = it.next()?;
        (k == key).then(|| url_decode(it.next().unwrap_or("")))
    })
}

/// 最小 percent-decode（`%XX` 与 `+`；非法转义原样保留）
fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                } else {
                    out.push(b'%');
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


/// 处理一条请求
async fn handle(app: &AppHandle, line: &str) -> Value {
    let req: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"ok": false, "error": format!("请求 JSON 非法: {}", e)}),
    };
    let cmd = req.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
    // 只读模式（api_readonly=true）：拒绝一切写命令。
    // 写命令身份唯一来源：capabilities 注册表的 Risk 分类（P0-2，2026-09-24），
    // 不再在本文件维护手写第二份名单。
    if crate::capabilities::is_write(cmd) {
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
    // R2-1c（2026-09-24）：命令分发已拆到 api/handlers.rs
    handlers::dispatch(app, &req, cmd).await
}

/// 模糊测试支撑（HANDOFF P2-5）
///
/// `fuzz/` 是独立 crate，无法访问本模块私有解析器；此 shim 仅在 `fuzz` feature
/// （或 lib 单测）下编译，对外签名只用基本类型，把任意字节/字符串喂入手写解析面。
/// 约定：**任何输入都不得 panic / 死循环**，只能返回结果码。
#[cfg(any(test, feature = "fuzz"))]
pub mod fuzz_helpers {
    use super::*;
    use std::net::IpAddr;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, ReadBuf};

    /// 按给定绝对切点把 data 拆成任意大小的 read 返回，切点用尽后 EOF。
    /// 模拟 TCP 任意拆包/粘包；构造时自动排序去重并补齐末尾，永不产生 0 字节"假 EOF"
    pub(crate) struct ChaosReader {
        data: Vec<u8>,
        cuts: Vec<usize>,
        cut_idx: usize,
        pos: usize,
    }

    impl ChaosReader {
        pub(crate) fn new(data: Vec<u8>, mut cuts: Vec<usize>) -> Self {
            cuts.retain(|&c| c > 0 && c < data.len());
            cuts.sort_unstable();
            cuts.dedup();
            cuts.push(data.len());
            Self {
                data,
                cuts,
                cut_idx: 0,
                pos: 0,
            }
        }
    }

    impl AsyncRead for ChaosReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            loop {
                if this.cut_idx >= this.cuts.len() {
                    return Poll::Ready(Ok(())); // 真 EOF
                }
                let end = this.cuts[this.cut_idx].min(this.data.len());
                let n = end.saturating_sub(this.pos).min(buf.remaining());
                this.cut_idx += 1;
                if n == 0 {
                    continue; // 跳过落在当前位置之前的切点，不发 0 字节读
                }
                buf.put_slice(&this.data[this.pos..this.pos + n]);
                this.pos += n;
                return Poll::Ready(Ok(()));
            }
        }
    }

    /// 驱动 HTTP/1.1 解析器。返回：0=Complete，1=TooLarge，2=Bad
    pub async fn parse_http(data: Vec<u8>, cuts: Vec<usize>) -> u8 {
        let mut r = ChaosReader::new(data, cuts);
        match read_http_request(&mut r).await {
            HttpReq::Complete(..) => 0,
            HttpReq::TooLarge => 1,
            HttpReq::Bad => 2,
        }
    }

    /// percent 解码（任意输入不得 panic）
    pub fn url_decode(s: &str) -> String {
        super::url_decode(s)
    }

    /// form-urlencoded 取值
    pub fn kv_value(encoded: &str, key: &str) -> Option<String> {
        super::kv_value(encoded, key)
    }

    /// Content-Length 解析
    pub fn content_length(head: &str) -> Option<usize> {
        super::content_length_of(head)
    }

    /// Transfer-Encoding: chunked 识别
    pub fn is_chunked(head: &str) -> bool {
        super::is_chunked(head)
    }

    /// CIDR 匹配。返回：1=true，0=false，-1=非法 CIDR，-2=非法 IP
    pub fn cidr_contains(ip: &str, cidr: &str) -> i8 {
        match ip.parse::<IpAddr>() {
            Ok(ip) => match super::auth::cidr_contains(ip, cidr) {
                Some(true) => 1,
                Some(false) => 0,
                None => -1,
            },
            Err(_) => -2,
        }
    }
}

#[cfg(test)]
mod http_req_tests {
    use super::*;
    use std::net::IpAddr;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, ReadBuf};

    /// 每次 poll_read 最多吐出 max_chunk 字节，模拟 TCP 跨包/拆包
    struct SlowReader {
        data: Vec<u8>,
        pos: usize,
        max_chunk: usize,
    }

    impl AsyncRead for SlowReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            let remain = this.data.len() - this.pos;
            if remain == 0 {
                return Poll::Ready(Ok(()));
            }
            let n = remain.min(this.max_chunk).min(buf.remaining());
            buf.put_slice(&this.data[this.pos..this.pos + n]);
            this.pos += n;
            Poll::Ready(Ok(()))
        }
    }

    fn post_request(body: &str) -> Vec<u8> {
        format!(
            "POST /api HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn small_body_single_read() {
        let mut r = SlowReader {
            data: post_request("hello"),
            pos: 0,
            max_chunk: 4096,
        };
        match read_http_request(&mut r).await {
            HttpReq::Complete(head, body) => {
                assert!(head.starts_with("POST /api HTTP/1.1"));
                assert_eq!(body, "hello");
            }
            other => panic!("期望 Complete，实际 {other:?}"),
        }
    }

    /// 回归：旧实现只读一次 64KB，200KB body 的长对话必然被截断
    #[tokio::test]
    async fn large_body_read_full_across_packets() {
        let body = "a".repeat(200 * 1024);
        let mut r = SlowReader {
            data: post_request(&body),
            pos: 0,
            max_chunk: 4096,
        };
        match read_http_request(&mut r).await {
            HttpReq::Complete(_, got) => assert_eq!(got.len(), 200 * 1024),
            other => panic!("期望 Complete，实际 {other:?}"),
        }
    }

    /// 头部本身跨多个 TCP 包（\r\n\r\n 被拆开）也能正确定位
    #[tokio::test]
    async fn headers_split_across_packets() {
        let mut r = SlowReader {
            data: post_request("xy"),
            pos: 0,
            max_chunk: 13,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Complete(_, b) if b == "xy"));
    }

    #[tokio::test]
    async fn oversized_body_rejected() {
        let body = "a".repeat(HTTP_MAX_BODY + 1);
        let mut r = SlowReader {
            data: post_request(&body),
            pos: 0,
            max_chunk: 16 * 1024,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::TooLarge));
    }

    #[tokio::test]
    async fn body_without_content_length_uses_tail_bytes() {
        let raw = b"POST /api HTTP/1.1\r\nHost: x\r\n\r\nabc".to_vec();
        let mut r = SlowReader {
            data: raw,
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Complete(_, b) if b == "abc"));
    }

    #[tokio::test]
    async fn closed_connection_is_bad() {
        let mut r = SlowReader {
            data: vec![],
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Bad));
    }

    #[tokio::test]
    async fn chunked_unsupported() {
        let raw =
            b"POST /api HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
                .to_vec();
        let mut r = SlowReader {
            data: raw,
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Bad));
    }

    #[test]
    fn content_length_parsing() {
        let head = "POST /api HTTP/1.1\r\ncontent-length: 42\r\n";
        assert_eq!(content_length_of(head), Some(42));
        assert_eq!(content_length_of("POST / HTTP/1.1\r\nHost: x\r\n"), None);
        assert_eq!(find_header_end(b"abc\r\n\r\nbody"), Some(7),);
        assert_eq!(find_header_end(b"abc\r\n\r"), None);
    }

    #[test]
    fn ip_allowlist_empty_allows_all() {
        assert!(super::auth::ip_allowed("8.8.8.8", ""));
        assert!(super::auth::ip_allowed("8.8.8.8", "   "));
    }

    #[test]
    fn ip_allowlist_bare_prefix_no_longer_matches_neighbor_segment() {
        // 旧 bug：裸 starts_with("192.168.1") 误放行 192.168.10.x
        assert!(!super::auth::ip_allowed("192.168.10.5", "192.168.1"));
        // 裸前缀现在是非法条目（不是精确 IP/CIDR），同样不放行
        assert!(!super::auth::ip_allowed("192.168.1.5", "192.168.1"));
    }

    #[test]
    fn ip_allowlist_dotted_segment_prefix_backcompat() {
        assert!(super::auth::ip_allowed("192.168.1.5", "192.168.1."));
        assert!(super::auth::ip_allowed("192.168.1.200", "192.168.1."));
        assert!(!super::auth::ip_allowed("192.168.10.5", "192.168.1."));
        assert!(!super::auth::ip_allowed("192.169.1.5", "192.168.1."));
    }

    #[test]
    fn ip_allowlist_exact_and_cidr() {
        assert!(super::auth::ip_allowed("192.168.1.5", "192.168.1.5"));
        assert!(!super::auth::ip_allowed("192.168.1.6", "192.168.1.5"));
        assert!(super::auth::ip_allowed("192.168.1.1", "192.168.1.0/24"));
        assert!(super::auth::ip_allowed("192.168.1.254", "192.168.1.0/24"));
        assert!(!super::auth::ip_allowed("192.168.2.1", "192.168.1.0/24"));
        assert!(super::auth::ip_allowed("10.1.2.3", "10.0.0.0/8"));
        assert!(!super::auth::ip_allowed("11.1.2.3", "10.0.0.0/8"));
        // 多值 + 空白容忍
        assert!(super::auth::ip_allowed("1.2.3.4", "  10.0.0.0/8 , 1.2.3.4 "));
        assert!(!super::auth::ip_allowed("5.6.7.8", "10.0.0.0/8,1.2.3.4"));
        // 非法 CIDR（前缀越界）与垃圾条目不影响合法条目
        assert!(super::auth::ip_allowed("10.0.0.1", "garbage,10.0.0.0/8,99.0.0.0/33"));
    }

    #[test]
    fn ip_allowlist_ipv6_and_mapped_v4() {
        assert!(super::auth::ip_allowed("::1", "::1"));
        assert!(super::auth::ip_allowed("fe80::1234", "fe80::/10"));
        assert!(!super::auth::ip_allowed("2001::1", "fe80::/10"));
        // IPv4-mapped IPv6 归一化后按 v4 规则判定（防 v6 形态绕过）
        assert!(super::auth::ip_allowed("::ffff:192.168.1.5", "192.168.1.0/24"));
        assert!(super::auth::ip_allowed("::ffff:192.168.1.5", "192.168.1."));
        assert!(!super::auth::ip_allowed("::ffff:192.168.2.5", "192.168.1.0/24"));
        // 跨族不匹配
        assert!(!super::auth::ip_allowed("::1", "127.0.0.0/8"));
    }

    #[test]
    fn backoff_exponential_caps_at_300() {
        assert_eq!(super::auth::backoff_secs(1), 1);
        assert_eq!(super::auth::backoff_secs(2), 2);
        assert_eq!(super::auth::backoff_secs(3), 4);
        assert_eq!(super::auth::backoff_secs(9), 256);
        assert_eq!(super::auth::backoff_secs(10), 300);
        assert_eq!(super::auth::backoff_secs(100), 300);
    }

    #[test]
    fn login_guard_loopback_never_locked() {
        let g = super::auth::LoginGuard::default();
        let t0 = Instant::now();
        let v4: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(g.record_failure(&v4, t0), 0);
        assert!(g.check(&v4, t0).is_none());
        g.record_success(&v6); // 不 panic、不产生状态
    }

    #[test]
    fn login_guard_locks_then_recovers_on_success() {
        let g = super::auth::LoginGuard::default();
        let ip: IpAddr = "192.168.5.5".parse().unwrap();
        let t0 = Instant::now();
        assert_eq!(g.record_failure(&ip, t0), 1);
        // 封禁窗口内 429
        assert!(g.check(&ip, t0).is_some());
        assert!(g.check(&ip, t0 + Duration::from_millis(500)).is_some());
        // 1s 后窗口过了，放行（fails 保留）
        assert!(g.check(&ip, t0 + Duration::from_secs(2)).is_none());
        // 再失败：fails=2 → 封 2s
        assert_eq!(g.record_failure(&ip, t0 + Duration::from_secs(2)), 2);
        assert!(g.check(&ip, t0 + Duration::from_secs(3)).is_some());
        // 成功认证清零：立刻放行，且下一次失败从 1s 重新计
        g.record_success(&ip);
        assert!(g.check(&ip, t0 + Duration::from_secs(3)).is_none());
        assert_eq!(g.record_failure(&ip, t0 + Duration::from_secs(3)), 1);
    }

    #[test]
    fn login_guard_peers_independent() {
        let g = super::auth::LoginGuard::default();
        let t0 = Instant::now();
        g.record_failure(&"192.168.0.1".parse().unwrap(), t0);
        assert!(g.check(&"192.168.0.2".parse().unwrap(), t0).is_none());
    }

    #[test]
    fn dashboard_ticket_single_use() {
        let tk = super::auth::DashboardTickets::new();
        let t = tk.issue();
        assert_eq!(t.len(), 32, "门票为 32hex");
        assert!(tk.consume(&t));
        assert!(!tk.consume(&t), "用后即焚");
        assert!(!tk.consume("deadbeefdeadbeefdeadbeefdeadbeef"));
        // 错误票不影响另一张合法票
        let t2 = tk.issue();
        assert!(tk.consume(&t2));
    }

    #[test]
    fn kv_value_and_url_decode() {
        assert_eq!(
            kv_value("a=1&token=ab%20c&d", "token").as_deref(),
            Some("ab c")
        );
        assert_eq!(kv_value("token=a+b", "token").as_deref(), Some("a b"));
        assert_eq!(kv_value("x=1", "token"), None);
        assert_eq!(kv_value("", "token"), None);
        assert_eq!(url_decode("100%25"), "100%");
        assert_eq!(url_decode("%zz"), "%zz");
    }

    #[test]
    fn bearer_header_check() {
        assert!(bearer_token_ok(
            "POST /api HTTP/1.1\r\nAuthorization: Bearer secret123\r\n",
            "secret123"
        ));
        assert!(!bearer_token_ok(
            "POST /api HTTP/1.1\r\nAuthorization: Bearer wrong\r\n",
            "secret123"
        ));
        assert!(bearer_token_ok(
            "POST /api HTTP/1.1\r\nauthorization: bearer secret123\r\n",
            "secret123"
        ));
        assert!(!bearer_token_ok("POST /api HTTP/1.1\r\nX-A: 1\r\n", "secret123"));
    }

    // ── 确定性模糊测试（HANDOFF P2-5）──────────────────────────────────────
    // 固定种子 xorshift，不引第三方依赖，CI 每次跑同一序列；
    // 覆盖引导深挖由 fuzz/ 下 cargo-fuzz 目标承担（每周 CI），二者互补。
    // 核心安全性质：任意输入 + 任意 TCP 拆包，解析器**不得 panic / 死循环**。
    struct Rng {
        state: u64,
    }

    impl Rng {
        fn new(seed: u64) -> Self {
            Self { state: seed | 1 }
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.state;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.state = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next_u64() % n as u64) as usize
        }
        fn byte(&mut self) -> u8 {
            (self.next_u64() & 0xff) as u8
        }
        fn percent(&mut self, p: usize) -> bool {
            self.below(100) < p
        }
    }

    /// 偏向解析器分支的字节字母表（CR/LF/冒号/数字/chunked 关键字等）
    const FUZZ_ALPHABET: &[u8] =
        b"\r\n\r\n :0123456789Content-LengthTransfer-EncodingchunkedPOST/%.\t-";

    fn fuzz_byte(rng: &mut Rng) -> u8 {
        if rng.percent(55) {
            FUZZ_ALPHABET[rng.below(FUZZ_ALPHABET.len())]
        } else {
            rng.byte()
        }
    }

    /// 对种子做随机替换/插入/删除
    fn mutate(rng: &mut Rng, bytes: &mut Vec<u8>) {
        for _ in 0..rng.below(10) {
            if bytes.is_empty() {
                if rng.percent(50) {
                    bytes.push(fuzz_byte(rng));
                }
                continue;
            }
            match rng.below(3) {
                0 => {
                    let i = rng.below(bytes.len());
                    bytes[i] = fuzz_byte(rng);
                }
                1 => {
                    let i = rng.below(bytes.len() + 1);
                    bytes.insert(i, fuzz_byte(rng));
                }
                _ => {
                    let i = rng.below(bytes.len());
                    bytes.remove(i);
                }
            }
        }
    }

    /// 生成 0..4 个随机绝对切点（ChaosReader 自行排序/去重/补末尾）
    fn random_cuts(rng: &mut Rng, len: usize) -> Vec<usize> {
        let mut cuts = Vec::new();
        for _ in 0..rng.below(5) {
            cuts.push(rng.below(len + 1));
        }
        cuts
    }

    #[tokio::test]
    async fn fuzz_http_parser_arbitrary_streams() {
        let mut rng = Rng::new(0x5f3a_2026_0924);
        for iter in 0..20_000 {
            let mut data = if rng.percent(65) {
                // 结构化种子：合法 POST 模板 + 各类边界 Content-Length
                let body_len = rng.below(40);
                let body: Vec<u8> = (0..body_len).map(|_| rng.byte()).collect();
                let cl = match rng.below(7) {
                    0 => "0".to_string(),
                    1 => body_len.to_string(),
                    2 => body_len
                        .saturating_add(rng.below(5))
                        .saturating_add(1)
                        .to_string(), // 声明比实际长 → 提前 EOF
                    3 => (8 * 1024 * 1024 + 1).to_string(), // 超 8MB 上限
                    4 => format!("{}abc", rng.below(100)),  // 含非数字
                    5 => "-1".to_string(),                  // 负数
                    _ => format!(" {}", body_len),          // 前导空格
                };
                let mut seed = String::from("POST /api HTTP/1.1\r\nHost: x\r\n");
                if rng.percent(30) {
                    seed.push_str("Transfer-Encoding: chunked\r\n");
                }
                seed.push_str(&format!("Content-Length: {cl}\r\n\r\n"));
                let mut bytes = seed.into_bytes();
                bytes.extend_from_slice(&body);
                mutate(&mut rng, &mut bytes);
                // 随机截断
                bytes.truncate(rng.below(bytes.len() + 1));
                // 20% 概率追加垃圾尾巴
                if rng.percent(20) {
                    let n = rng.below(30);
                    bytes.extend((0..n).map(|_| fuzz_byte(&mut rng)));
                }
                bytes
            } else {
                // 纯随机字节流
                (0..rng.below(300)).map(|_| fuzz_byte(&mut rng)).collect()
            };
            // 每 500 次造一个 >64KB 无终止头，钉死头部上限分支
            if iter % 500 == 0 {
                data.extend(std::iter::repeat(b'a').take(70_000));
            }

            let cuts = random_cuts(&mut rng, data.len());
            // 与 cargo-fuzz 目标走同一个 shim 入口，只校验结果码
            let code = super::fuzz_helpers::parse_http(data.clone(), cuts.clone()).await;
            assert!(code <= 2, "shim 返回非法结果码: {code}");
            // 再直接驱动私有解析器拿 head/body 做弱不变量校验
            let mut reader = super::fuzz_helpers::ChaosReader::new(data, cuts);
            match read_http_request(&mut reader).await {
                HttpReq::Complete(head, body) => {
                    // 弱不变量：声明了合法 CL 时，body 不会膨胀超 CL 的 3 倍（lossy 替换上限）
                    if let Some(cl) = content_length_of(&head) {
                        assert!(
                            body.is_empty() || body.len() <= cl.saturating_mul(3),
                            "body 超长 cl={cl} body_len={}",
                            body.len()
                        );
                    }
                }
                HttpReq::TooLarge | HttpReq::Bad => {}
            }
        }
    }

    #[test]
    fn fuzz_codec_arbitrary_inputs() {
        let mut rng = Rng::new(0xc0de_2026_0924);
        for _ in 0..30_000 {
            // 任意字符串（含残缺 %XX、+、非 ASCII）不得 panic
            let raw: String = (0..rng.below(40))
                .map(|_| {
                    let b = fuzz_byte(&mut rng);
                    // 映射成合法 char：保留可打印 ASCII 与少量多字节范围
                    char::from_u32(if rng.percent(80) {
                        (b % 95 + 32) as u32
                    } else {
                        0x4e00 + (b as u32)
                    })
                    .unwrap_or('?')
                })
                .collect();
            // 与 cargo-fuzz codec 目标走同一 shim
            let _ = super::fuzz_helpers::url_decode(&raw);
            let _ = super::fuzz_helpers::kv_value(&raw, "token");
            let _ = super::fuzz_helpers::content_length(&raw);
            let _ = super::fuzz_helpers::is_chunked(&raw);
        }

        // 性质 1：unreserved 字符经 encode→decode 往返不变
        let unreserved = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.~";
        for _ in 0..5_000 {
            let s: String = (0..rng.below(30) + 1)
                .map(|_| unreserved[rng.below(unreserved.len())] as char)
                .collect();
            assert_eq!(
                super::fuzz_helpers::url_decode(&url_encode(&s)),
                s,
                "unreserved 往返失败: {s}"
            );
        }

        // 性质 2：任意可打印 ASCII（除 & 分隔符）编码后都能从 form 中精确取回
        for _ in 0..5_000 {
            let v: String = (0..rng.below(20) + 1)
                .map(|_| {
                    let mut b = (rng.below(0x5f) + 0x20) as u8; // 空格(0x20)~~(0x7e)
                    if b == b'&' {
                        b = b'x';
                    }
                    b as char
                })
                .collect();
            let form = format!("token={}", url_encode(&v));
            assert_eq!(
                super::fuzz_helpers::kv_value(&form, "token"),
                Some(v),
                "form 往返失败"
            );
        }
    }

    #[test]
    fn fuzz_cidr_arbitrary_inputs() {
        let mut rng = Rng::new(0xc1d4_2026_0924);
        // 任意字符串不得 panic
        for _ in 0..10_000 {
            let ip: String = (0..rng.below(40))
                .map(|_| (rng.below(95) + 32) as u8 as char)
                .collect();
            let cidr: String = (0..rng.below(40))
                .map(|_| (rng.below(95) + 32) as u8 as char)
                .collect();
            let code = super::fuzz_helpers::cidr_contains(&ip, &cidr);
            assert!((-2..=1).contains(&code));
        }
        // 确定性性质
        let cidr = |ip: &str, c: &str| super::fuzz_helpers::cidr_contains(ip, c);
        assert_eq!(cidr("8.8.8.8", "0.0.0.0/0"), 1);
        assert_eq!(cidr("2001:db8::1", "::/0"), 1);
        assert_eq!(cidr("192.168.1.5", "192.168.1.5/32"), 1);
        assert_eq!(cidr("192.168.1.6", "192.168.1.5/32"), 0);
        assert_eq!(cidr("192.168.1.5", "192.168.0.0/16"), 1);
        assert_eq!(cidr("10.0.0.1", "192.168.0.0/16"), 0);
        assert_eq!(cidr("::1", "127.0.0.0/8"), 0, "v6/v4 族不匹配");
        assert_eq!(cidr("1.2.3.4", "1.2.3.4/99"), -1, "/99 非法前缀");
        assert_eq!(cidr("1.2.3.4", "not-a-cidr"), -1);
        assert_eq!(cidr("999.1.1.1", "0.0.0.0/0"), -2, "非法 IP");
    }
}

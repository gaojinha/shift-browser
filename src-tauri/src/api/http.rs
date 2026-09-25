//! HTTP API 服务器（R2-1c 拆分：从 api.rs 抽出）
//!
//! 本机/局域网 HTTP 服务（默认 127.0.0.1:19701）：状态面板（登录页 + 主页）、
//! POST /api JSON 命令、/v1/chat/completions 模型网关。鉴权与限速见 super::auth，
//! 面板渲染与网关转发见 super::dashboard，命令分发见 super::handle。

use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Manager};

use super::auth;
use super::dashboard;
use crate::error::AppError;

// 兄弟文件子模块（R1 拆分 2026-09-25）：请求读取/解析 → req，响应写入/小工具 → util
#[path = "http/req.rs"]
mod req;
#[path = "http/util.rs"]
mod util;

// re-export 保持 `super::http::<item>` 路径稳定（api.rs fuzz shim 与 http_tests 依赖）
pub(crate) use req::{read_http_request, HttpReq};
pub(crate) use util::{bearer_token_ok, kv_value, write_http};
// 仅 fuzz shim（api.rs，cfg test/fuzz 门控）消费
#[cfg(any(test, feature = "fuzz"))]
pub(crate) use req::{content_length_of, is_chunked};
#[cfg(any(test, feature = "fuzz"))]
pub(crate) use util::url_decode;
// 仅 http_tests 消费
#[cfg(test)]
pub(crate) use req::{find_header_end, HTTP_MAX_BODY};

const HTTP_PORT: u16 = 19701;

/// dashboard 页面安全响应头：禁被嵌入任意框架（点击劫持），CSP 收紧到无脚本/无外联
const DASHBOARD_HEADERS: &str = "X-Frame-Options: DENY\r\n\
Content-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'\r\n\
Referrer-Policy: no-referrer\r\n";

pub(crate) fn spawn_http(app: AppHandle, tokens: auth::ApiTokenShared) {
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
                            AppError::validation("请求体过大（上限 8MB）")
                                .gateway_error_json()
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
                        AppError::auth(format!("认证失败过多，请 {retry} 秒后重试"))
                            .gateway_error_json()
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
                            AppError::auth("未授权：需要 Authorization: Bearer <token>")
                                .gateway_error_json()
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
                        AppError::validation("请用 POST /api 发 JSON 命令")
                            .error_json()
                            .to_string(),
                    )
                } else if !bearer_ok {
                    let retry = guard.record_failure(&peer_ip, Instant::now());
                    log::warn!("[api] /api 未授权 peer={peer_ip}（封禁 {retry}s）");
                    (
                        401,
                        "Unauthorized",
                        format!("Retry-After: {retry}\r\n"),
                        AppError::auth("未授权：需要 Authorization: Bearer <token>")
                            .error_json()
                            .to_string(),
                    )
                } else {
                    guard.record_success(&peer_ip);
                    (
                        200,
                        "OK",
                        String::new(),
                        serde_json::to_string(&super::handle(&app, body.trim()).await)
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

#[cfg(test)]
#[path = "http_tests.rs"]
mod http_tests;

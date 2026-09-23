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
use std::collections::HashMap;
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
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
    if let Some(token) = ensure_token(&app) {
        let token_shared = ApiTokenShared(Arc::new(RwLock::new(token)));
        app.manage(token_shared.clone());
        app.manage(DashboardTickets::new());
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
    let t = generate_token();
    let _ = atomic_write_token(&path, &t);
    Some(t)
}

/// 生成 API token（两个 uuid v4 去横线拼接：256bit 随机、64 hex 字符）
fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 原子写 token：同目录临时文件 0600 + rename，防半写/替换窗口读到空文件
fn atomic_write_token(path: &std::path::Path, token: &str) -> std::io::Result<()> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("api-token");
    let tmp = path.with_file_name(format!("{name}.rotate-tmp"));
    std::fs::write(&tmp, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// 运行期共享的 API token：`rotate_api_token` 替换后，HTTP 服务下一请求即认新 token，
/// 无需重启（连接任务每请求读取一次）
#[derive(Clone)]
pub(crate) struct ApiTokenShared(Arc<RwLock<String>>);

/// 状态面板一次性门票：TTL 内兑换**一次即焚**，取代 URL query 里的长期 token
///（长期 token 进 query 会落浏览器历史、Referer、代理日志）
#[derive(Clone)]
pub(crate) struct DashboardTickets(Arc<Mutex<HashMap<String, Instant>>>);

/// 门票有效期（秒）
const DASHBOARD_TICKET_TTL: Duration = Duration::from_secs(300);

impl DashboardTickets {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    /// 签发一张新门票（32 hex / 128bit 随机）；顺带清理过期票防无界增长
    fn issue(&self) -> String {
        let ticket = uuid::Uuid::new_v4().simple().to_string();
        if let Ok(mut map) = self.0.lock() {
            let now = Instant::now();
            map.retain(|_, at| now.duration_since(*at) < DASHBOARD_TICKET_TTL);
            map.insert(ticket.clone(), now);
        }
        ticket
    }

    /// 兑换门票：存在且未过期 → 删除并返回 true；错误/过期票不影响其它门票
    fn consume(&self, ticket: &str) -> bool {
        let Ok(mut map) = self.0.lock() else {
            return false;
        };
        let now = Instant::now();
        let valid = map
            .get(ticket)
            .is_some_and(|at| now.duration_since(*at) < DASHBOARD_TICKET_TTL);
        if valid {
            map.remove(ticket);
        }
        map.retain(|_, at| now.duration_since(*at) < DASHBOARD_TICKET_TTL);
        valid
    }
}

/// LAN 认证失败限速状态：同一 peer 连续 401 后指数退避。
/// 只对非 loopback 生效——本机调用不计数不锁定（本地工具配错 token 不应被自锁）。
#[derive(Default)]
struct LoginGuard {
    map: Mutex<HashMap<IpAddr, FailState>>,
}

#[derive(Clone, Copy)]
struct FailState {
    /// 连续失败次数（成功认证清零）
    fails: u32,
    /// 封禁截止时刻；到期后允许重试，但 fails 保留（持续探测维持高水位退避）
    locked_until: Option<Instant>,
}

/// 第 fails 次失败后的封禁秒数：1, 2, 4, 8 … 封顶 300
fn backoff_secs(fails: u32) -> u64 {
    2u64.saturating_pow(fails.saturating_sub(1)).min(300)
}

impl LoginGuard {
    /// 封禁中返回剩余秒数；loopback 与未锁定返回 None（先于 token 校验调用）
    fn check(&self, ip: &IpAddr, now: Instant) -> Option<u64> {
        if ip.is_loopback() {
            return None;
        }
        let map = self.map.lock().ok()?;
        map.get(ip)
            .and_then(|s| s.locked_until)
            .filter(|until| now < *until)
            .map(|until| until.saturating_duration_since(now).as_secs().max(1))
    }

    /// 记录一次凭据错误，返回本次封禁秒数（loopback 返回 0 表示不锁）
    fn record_failure(&self, ip: &IpAddr, now: Instant) -> u64 {
        if ip.is_loopback() {
            return 0;
        }
        let Ok(mut map) = self.map.lock() else {
            return 0;
        };
        // 简单防无界增长：条目过多时先淘汰已解封的
        if map.len() > 256 {
            map.retain(|_, s| s.locked_until.is_some_and(|t| t > now));
        }
        let state = map.entry(*ip).or_insert(FailState {
            fails: 0,
            locked_until: None,
        });
        state.fails = state.fails.saturating_add(1);
        let secs = backoff_secs(state.fails);
        state.locked_until = Some(now + Duration::from_secs(secs));
        secs
    }

    /// 认证成功：清零该 peer 的失败计数
    fn record_success(&self, ip: &IpAddr) {
        if ip.is_loopback() {
            return;
        }
        if let Ok(mut map) = self.map.lock() {
            map.remove(ip);
        }
    }
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

/// IP 白名单校验：名单为空 = 不限制；非空时逗号分隔，每个条目须为以下之一：
///
/// - 精确 IP：`192.168.1.5`、`::1`
/// - CIDR：`192.168.1.0/24`、`10.0.0.0/8`、`fe80::/10`
/// - 旧式段前缀（向后兼容，**必须以点结尾**）：`192.168.1.` 匹配该 C 段
///
/// 安全（HANDOFF P2-3）：旧实现裸 `starts_with`，配置 `192.168.1` 会误放行
/// `192.168.10.x`；现在不带尾点的条目必须是合法 IP/CIDR，非法条目忽略并告警。
/// IPv4-mapped IPv6（`::ffff:1.2.3.4`）先归一化，防用 v6 形态绕过 v4 规则。
fn ip_allowed(ip: &str, allowlist: &str) -> bool {
    let allow = allowlist.trim();
    if allow.is_empty() {
        return true;
    }
    let Ok(peer): Result<IpAddr, _> = ip.parse() else {
        return false;
    };
    // IPv4-mapped IPv6 归一化为 IPv4
    let peer = match peer {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
        v4 => v4,
    };
    let peer_str = peer.to_string();
    for raw in allow
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if raw.contains('/') {
            match cidr_contains(peer, raw) {
                Some(true) => return true,
                Some(false) => continue,
                None => log::warn!("[api] 白名单 CIDR 条目非法，已忽略: {raw}"),
            }
        } else if raw.ends_with('.') {
            // 旧式段前缀：尾点保证不跨段（"192.168.1." 不会碰到 "192.168.10.x"）
            if peer.is_ipv4() && peer_str.starts_with(raw) {
                return true;
            }
        } else if let Ok(entry) = raw.parse::<IpAddr>() {
            if entry == peer {
                return true;
            }
        } else {
            log::warn!(
                "[api] 白名单条目非法（需为 IP / CIDR / 尾点段前缀如 192.168.1.），已忽略: {raw}"
            );
        }
    }
    false
}

/// CIDR 包含判定；条目语法非法返回 None（同族才比较，v4 前缀 0..=32，v6 0..=128）
fn cidr_contains(peer: IpAddr, cidr: &str) -> Option<bool> {
    let (net_s, prefix_s) = cidr.split_once('/')?;
    let prefix: u32 = prefix_s.trim().parse().ok()?;
    let net: IpAddr = net_s.trim().parse().ok()?;
    // 用 octets()+from_be_bytes 而非 to_bits()（后者 1.80 才稳定，本仓 MSRV 1.77.2）
    let (a, n, width) = match (peer, net) {
        (IpAddr::V4(a), IpAddr::V4(n)) => (
            u32::from_be_bytes(a.octets()) as u128,
            u32::from_be_bytes(n.octets()) as u128,
            32,
        ),
        (IpAddr::V6(a), IpAddr::V6(n)) => (
            u128::from_be_bytes(a.octets()),
            u128::from_be_bytes(n.octets()),
            128,
        ),
        _ => return Some(false),
    };
    if prefix > width {
        return None;
    }
    let mask = if prefix == 0 {
        0
    } else {
        u128::MAX << (width - prefix)
    };
    Some((a & mask) == (n & mask))
}

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

fn spawn_http(app: AppHandle, tokens: ApiTokenShared) {
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
    // LAN 认证失败限速状态（仅绑定 0.0.0.0 时有意义，本机绑定下 peer 全是 loopback 不计数）
    let guard = Arc::new(LoginGuard::default());
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
            if !ip_allowed(&peer_ip.to_string(), &allowed_ips) {
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
                        let s = serde_json::to_string(&model_gateway(&app, &body).await)
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
                    let tickets_ok = app.try_state::<DashboardTickets>();
                    let ticket_ok = query_ticket.as_deref().is_some_and(|t| {
                        tickets_ok.as_ref().is_some_and(|s| s.consume(t))
                    }) || posted.as_deref().is_some_and(|t| {
                        tickets_ok.as_ref().is_some_and(|s| s.consume(t))
                    });
                    let ok = bearer_ok
                        || ticket_ok
                        || posted.as_deref().is_some_and(|t| ct_eq(t, &token));
                    if ok {
                        guard.record_success(&peer_ip);
                        write_http(
                            &mut stream,
                            200,
                            "OK",
                            "text/html; charset=utf-8",
                            DASHBOARD_HEADERS,
                            dashboard_html(&app).await.as_bytes(),
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
                        dashboard_login_html(err, retry).as_bytes(),
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
                ct_eq(v, token)
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

/// 状态面板登录页（无脚本、严格 CSP；长期 token 只接受 POST 表单）
fn dashboard_login_html(error: Option<&str>, retry_after: u64) -> String {
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    };
    let banner = if retry_after > 0 {
        format!("<p class=err>认证失败过多，请 {retry_after} 秒后再试</p>")
    } else {
        String::new()
    };
    let error_html = error
        .map(|e| format!("<p class=err>{}</p>", esc(e)))
        .unwrap_or_default();
    r##"<!doctype html><meta charset=utf-8><title>ShiftBrowser API 登录</title>
<style>body{font:14px/1.6 system-ui,sans-serif;max-width:560px;margin:64px auto;padding:0 16px;color:#222}
h1{font-size:20px} .card{border:1px solid #ddd;border-radius:10px;padding:18px 20px;margin:12px 0}
input{width:100%;box-sizing:border-box;padding:8px 10px;margin:10px 0;font:13px monospace;border:1px solid #ccc;border-radius:6px}
button{padding:8px 18px;border:0;border-radius:6px;background:#2f6fed;color:#fff;font-size:14px;cursor:pointer}
.hint{color:#888;font-size:12px;line-height:1.7} .err{color:#c0392b} code{background:#f2f2f2;padding:1px 5px;border-radius:4px}</style>
<h1>ShiftBrowser API</h1>
<div class=card>
<form method="post" action="/">
  <label>粘贴 API token（或一次性门票）</label>
  <input name="token" autocomplete="off" autofocus required placeholder="api-token 文件内容 / 应用内签发的门票">
  <button type="submit">打开状态面板</button>
</form>
{BANNER}{ERROR}
<p class=hint>长期 token 不再支持放在 URL 查询串（<code>?token=</code> 会落入浏览器历史、Referer 与代理日志）。
两种安全方式：① 在此表单粘贴（POST body，不进 URL）；② 在应用「设置 → 开放 API」点「打开状态面板」，
自动签发 5 分钟有效、用后即焚的一次性门票。</p>
</div>"##
        .replace("{BANNER}", &banner)
        .replace("{ERROR}", &error_html)
}

/// 状态面板 HTML（自托管套件的"管理页"；凭据见 spawn_http 中三种安全方式）
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

/// 轮换 API token（HANDOFF P2-3）：生成新 token 原子落盘并热替换运行期状态，
/// 旧 token 在下一请求立即失效（HTTP 服务无需重启）。返回新 token，仅本次展示。
#[tauri::command]
pub(crate) async fn rotate_api_token(
    app: AppHandle,
    tokens: tauri::State<'_, ApiTokenShared>,
) -> Result<String, String> {
    let db = app
        .try_state::<crate::db::Db>()
        .ok_or_else(|| "数据库未就绪".to_string())?;
    let path = db.vault_path.with_file_name("api-token");
    let new = generate_token();
    atomic_write_token(&path, &new).map_err(|e| format!("写入 api-token 失败: {e}"))?;
    *tokens.0.write().await = new.clone();
    log::info!("[api] API token 已轮换（旧 token 即时失效）");
    Ok(new)
}

/// 签发状态面板一次性门票：32hex 随机，5 分钟有效、兑换一次即焚。
/// 前端拿到后拼 `http://127.0.0.1:<port>/?ticket=<t>` 用外部浏览器打开。
#[tauri::command]
pub(crate) fn create_dashboard_ticket(
    tickets: tauri::State<'_, DashboardTickets>,
) -> Result<String, String> {
    Ok(tickets.issue())
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
    // SSRF 防护（P0-5，2026-09-24）：网关转发携带 API Key，上游地址必须与
    // AI 聊天同一校验口径——https-only、拒绝 userinfo、拒绝本机/内网字面地址
    // （含 IPv4 混淆与 IPv4-mapped IPv6 绕过）。否则持 token 的调用方可把
    // 网关变成携带密钥打内网的跳板。
    let base = base.trim();
    if let Err(e) = crate::ai::validate_base_url(base) {
        log::warn!("[api] 模型网关上游地址被安全策略拒绝: {} ({})", e, base);
        return json!({"error": {"message": format!("上游地址未通过安全校验: {e}")}});
    }
    // P0-6：域名解析后 pin 死公网地址，消除"校验时解析一次、发送时再解析一次"
    // 的 DNS-rebinding TOCTOU（双答 DNS 在内网混答时直接拒绝）。
    let (resolve_key, pinned) = match crate::ai::resolve_pin_for_https(base).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[api] 模型网关注解/pin 上游地址失败: {} ({})", e, base);
            return json!({"error": {"message": format!("上游地址未通过安全校验: {e}")}});
        }
    };
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120));
    if let Some(addrs) = &pinned {
        builder = builder.resolve_to_addrs(&resolve_key, addrs);
    }
    let client = match builder.build() {
        Ok(c) => c,
        Err(e) => return json!({"error": {"message": format!("创建 HTTP 客户端失败: {}", e)}}),
    };
    let url = format!("{}/chat/completions", base.trim_end_matches('/'));
    match client
        .post(&url)
        .bearer_auth(&key)
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => v,
            Err(e) => json!({"error": {"message": format!("解析上游响应失败: {}", e)}}),
        },
        Err(e) => json!({"error": {"message": format!("上游请求失败: {}", e)}}),
    }
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

#[cfg(test)]
mod http_req_tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
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
        assert!(ip_allowed("8.8.8.8", ""));
        assert!(ip_allowed("8.8.8.8", "   "));
    }

    #[test]
    fn ip_allowlist_bare_prefix_no_longer_matches_neighbor_segment() {
        // 旧 bug：裸 starts_with("192.168.1") 误放行 192.168.10.x
        assert!(!ip_allowed("192.168.10.5", "192.168.1"));
        // 裸前缀现在是非法条目（不是精确 IP/CIDR），同样不放行
        assert!(!ip_allowed("192.168.1.5", "192.168.1"));
    }

    #[test]
    fn ip_allowlist_dotted_segment_prefix_backcompat() {
        assert!(ip_allowed("192.168.1.5", "192.168.1."));
        assert!(ip_allowed("192.168.1.200", "192.168.1."));
        assert!(!ip_allowed("192.168.10.5", "192.168.1."));
        assert!(!ip_allowed("192.169.1.5", "192.168.1."));
    }

    #[test]
    fn ip_allowlist_exact_and_cidr() {
        assert!(ip_allowed("192.168.1.5", "192.168.1.5"));
        assert!(!ip_allowed("192.168.1.6", "192.168.1.5"));
        assert!(ip_allowed("192.168.1.1", "192.168.1.0/24"));
        assert!(ip_allowed("192.168.1.254", "192.168.1.0/24"));
        assert!(!ip_allowed("192.168.2.1", "192.168.1.0/24"));
        assert!(ip_allowed("10.1.2.3", "10.0.0.0/8"));
        assert!(!ip_allowed("11.1.2.3", "10.0.0.0/8"));
        // 多值 + 空白容忍
        assert!(ip_allowed("1.2.3.4", "  10.0.0.0/8 , 1.2.3.4 "));
        assert!(!ip_allowed("5.6.7.8", "10.0.0.0/8,1.2.3.4"));
        // 非法 CIDR（前缀越界）与垃圾条目不影响合法条目
        assert!(ip_allowed("10.0.0.1", "garbage,10.0.0.0/8,99.0.0.0/33"));
    }

    #[test]
    fn ip_allowlist_ipv6_and_mapped_v4() {
        assert!(ip_allowed("::1", "::1"));
        assert!(ip_allowed("fe80::1234", "fe80::/10"));
        assert!(!ip_allowed("2001::1", "fe80::/10"));
        // IPv4-mapped IPv6 归一化后按 v4 规则判定（防 v6 形态绕过）
        assert!(ip_allowed("::ffff:192.168.1.5", "192.168.1.0/24"));
        assert!(ip_allowed("::ffff:192.168.1.5", "192.168.1."));
        assert!(!ip_allowed("::ffff:192.168.2.5", "192.168.1.0/24"));
        // 跨族不匹配
        assert!(!ip_allowed("::1", "127.0.0.0/8"));
    }

    #[test]
    fn backoff_exponential_caps_at_300() {
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(9), 256);
        assert_eq!(backoff_secs(10), 300);
        assert_eq!(backoff_secs(100), 300);
    }

    #[test]
    fn login_guard_loopback_never_locked() {
        let g = LoginGuard::default();
        let t0 = Instant::now();
        let v4: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(g.record_failure(&v4, t0), 0);
        assert!(g.check(&v4, t0).is_none());
        g.record_success(&v6); // 不 panic、不产生状态
    }

    #[test]
    fn login_guard_locks_then_recovers_on_success() {
        let g = LoginGuard::default();
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
        let g = LoginGuard::default();
        let t0 = Instant::now();
        g.record_failure(&"192.168.0.1".parse().unwrap(), t0);
        assert!(g.check(&"192.168.0.2".parse().unwrap(), t0).is_none());
    }

    #[test]
    fn dashboard_ticket_single_use() {
        let tk = DashboardTickets::new();
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
}

//! API 鉴权辅助（R2-1c 拆分：从 api.rs 抽出）
//!
//! API token 生成/原子落盘、运行期共享 token 与状态面板一次性门票、
//! LAN 登录失败限速（LoginGuard）、常数时间比较、IP 白名单/CIDR 校验。
//! 供 api.rs 的 spawn、api/http.rs 的 HTTP 服务、api/dashboard.rs 的命令使用。

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tokio::sync::RwLock;

pub(crate) fn ensure_token(app: &AppHandle) -> Option<String> {
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
pub(crate) fn generate_token() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

/// 原子写 token：同目录临时文件 0600 + rename，防半写/替换窗口读到空文件
pub(crate) fn atomic_write_token(path: &std::path::Path, token: &str) -> std::io::Result<()> {
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
pub(crate) struct ApiTokenShared(pub(crate) Arc<RwLock<String>>);

/// 状态面板一次性门票：TTL 内兑换**一次即焚**，取代 URL query 里的长期 token
///（长期 token 进 query 会落浏览器历史、Referer、代理日志）
#[derive(Clone)]
pub(crate) struct DashboardTickets(pub(crate) Arc<Mutex<HashMap<String, Instant>>>);

/// 门票有效期（秒）
const DASHBOARD_TICKET_TTL: Duration = Duration::from_secs(300);

impl DashboardTickets {
    pub(crate) fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    /// 签发一张新门票（32 hex / 128bit 随机）；顺带清理过期票防无界增长
    pub(crate) fn issue(&self) -> String {
        let ticket = uuid::Uuid::new_v4().simple().to_string();
        if let Ok(mut map) = self.0.lock() {
            let now = Instant::now();
            map.retain(|_, at| now.duration_since(*at) < DASHBOARD_TICKET_TTL);
            map.insert(ticket.clone(), now);
        }
        ticket
    }

    /// 兑换门票：存在且未过期 → 删除并返回 true；错误/过期票不影响其它门票
    pub(crate) fn consume(&self, ticket: &str) -> bool {
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
pub(crate) struct LoginGuard {
    map: Mutex<HashMap<IpAddr, FailState>>,
}

#[derive(Clone, Copy)]
pub(crate) struct FailState {
    /// 连续失败次数（成功认证清零）
    fails: u32,
    /// 封禁截止时刻；到期后允许重试，但 fails 保留（持续探测维持高水位退避）
    locked_until: Option<Instant>,
}

/// 第 fails 次失败后的封禁秒数：1, 2, 4, 8 … 封顶 300
pub(crate) fn backoff_secs(fails: u32) -> u64 {
    2u64.saturating_pow(fails.saturating_sub(1)).min(300)
}

impl LoginGuard {
    /// 封禁中返回剩余秒数；loopback 与未锁定返回 None（先于 token 校验调用）
    pub(crate) fn check(&self, ip: &IpAddr, now: Instant) -> Option<u64> {
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
    pub(crate) fn record_failure(&self, ip: &IpAddr, now: Instant) -> u64 {
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
    pub(crate) fn record_success(&self, ip: &IpAddr) {
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
pub(crate) fn ct_eq(a: &str, b: &str) -> bool {
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
pub(crate) fn read_setting_blocking(app: &AppHandle, key: &str) -> Option<String> {
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

/// 开放接口侧只读判断唯一来源（与 `capabilities::is_write` 配合组成只读闸门）：
/// `settings.api_readonly == "true"` 即只读。查询失败/键缺失按未开启处理——
/// 只读是用户显式开启的约束，不是默认状态。调用方：`api.rs handle` 主闸门、
/// `api/http.rs` HTTP 闸门、`api/handlers/data.rs` 凭据键闸门，三处同口径。
pub(crate) async fn is_readonly(app: &AppHandle) -> bool {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return false;
    };
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = 'api_readonly'")
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .is_some_and(|v| v == "true")
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
pub(crate) fn ip_allowed(ip: &str, allowlist: &str) -> bool {
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
    for raw in allow.split(',').map(str::trim).filter(|s| !s.is_empty()) {
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
pub(crate) fn cidr_contains(peer: IpAddr, cidr: &str) -> Option<bool> {
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

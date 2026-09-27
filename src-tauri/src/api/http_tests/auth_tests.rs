//! 鉴权面测试：IP 允许名单 / 退避 / LoginGuard / DashboardTickets / 编解码 / bearer
//! （R1 2026-09-25 从 http_tests.rs 拆分）

use super::super::*;
use std::net::IpAddr;
use std::time::Duration;

#[test]
fn ip_allowlist_empty_allows_all() {
    assert!(crate::api::auth::ip_allowed("8.8.8.8", ""));
    assert!(crate::api::auth::ip_allowed("8.8.8.8", "   "));
}

#[test]
fn ip_allowlist_bare_prefix_no_longer_matches_neighbor_segment() {
    // 旧 bug：裸 starts_with("192.168.1") 误放行 192.168.10.x
    assert!(!crate::api::auth::ip_allowed("192.168.10.5", "192.168.1"));
    // 裸前缀现在是非法条目（不是精确 IP/CIDR），同样不放行
    assert!(!crate::api::auth::ip_allowed("192.168.1.5", "192.168.1"));
}

#[test]
fn ip_allowlist_dotted_segment_prefix_backcompat() {
    assert!(crate::api::auth::ip_allowed("192.168.1.5", "192.168.1."));
    assert!(crate::api::auth::ip_allowed("192.168.1.200", "192.168.1."));
    assert!(!crate::api::auth::ip_allowed("192.168.10.5", "192.168.1."));
    assert!(!crate::api::auth::ip_allowed("192.169.1.5", "192.168.1."));
}

#[test]
fn ip_allowlist_exact_and_cidr() {
    assert!(crate::api::auth::ip_allowed("192.168.1.5", "192.168.1.5"));
    assert!(!crate::api::auth::ip_allowed("192.168.1.6", "192.168.1.5"));
    assert!(crate::api::auth::ip_allowed(
        "192.168.1.1",
        "192.168.1.0/24"
    ));
    assert!(crate::api::auth::ip_allowed(
        "192.168.1.254",
        "192.168.1.0/24"
    ));
    assert!(!crate::api::auth::ip_allowed(
        "192.168.2.1",
        "192.168.1.0/24"
    ));
    assert!(crate::api::auth::ip_allowed("10.1.2.3", "10.0.0.0/8"));
    assert!(!crate::api::auth::ip_allowed("11.1.2.3", "10.0.0.0/8"));
    // 多值 + 空白容忍
    assert!(crate::api::auth::ip_allowed(
        "1.2.3.4",
        "  10.0.0.0/8 , 1.2.3.4 "
    ));
    assert!(!crate::api::auth::ip_allowed(
        "5.6.7.8",
        "10.0.0.0/8,1.2.3.4"
    ));
    // 非法 CIDR（前缀越界）与垃圾条目不影响合法条目
    assert!(crate::api::auth::ip_allowed(
        "10.0.0.1",
        "garbage,10.0.0.0/8,99.0.0.0/33"
    ));
}

#[test]
fn ip_allowlist_ipv6_and_mapped_v4() {
    assert!(crate::api::auth::ip_allowed("::1", "::1"));
    assert!(crate::api::auth::ip_allowed("fe80::1234", "fe80::/10"));
    assert!(!crate::api::auth::ip_allowed("2001::1", "fe80::/10"));
    // IPv4-mapped IPv6 归一化后按 v4 规则判定（防 v6 形态绕过）
    assert!(crate::api::auth::ip_allowed(
        "::ffff:192.168.1.5",
        "192.168.1.0/24"
    ));
    assert!(crate::api::auth::ip_allowed(
        "::ffff:192.168.1.5",
        "192.168.1."
    ));
    assert!(!crate::api::auth::ip_allowed(
        "::ffff:192.168.2.5",
        "192.168.1.0/24"
    ));
    // 跨族不匹配
    assert!(!crate::api::auth::ip_allowed("::1", "127.0.0.0/8"));
}

#[test]
fn backoff_exponential_caps_at_300() {
    assert_eq!(crate::api::auth::backoff_secs(1), 1);
    assert_eq!(crate::api::auth::backoff_secs(2), 2);
    assert_eq!(crate::api::auth::backoff_secs(3), 4);
    assert_eq!(crate::api::auth::backoff_secs(9), 256);
    assert_eq!(crate::api::auth::backoff_secs(10), 300);
    assert_eq!(crate::api::auth::backoff_secs(100), 300);
}

#[test]
fn login_guard_loopback_never_locked() {
    let g = crate::api::auth::LoginGuard::default();
    let t0 = Instant::now();
    let v4: IpAddr = "127.0.0.1".parse().unwrap();
    let v6: IpAddr = "::1".parse().unwrap();
    assert_eq!(g.record_failure(&v4, t0), 0);
    assert!(g.check(&v4, t0).is_none());
    g.record_success(&v6); // 不 panic、不产生状态
}

#[test]
fn login_guard_locks_then_recovers_on_success() {
    let g = crate::api::auth::LoginGuard::default();
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
    let g = crate::api::auth::LoginGuard::default();
    let t0 = Instant::now();
    g.record_failure(&"192.168.0.1".parse().unwrap(), t0);
    assert!(g.check(&"192.168.0.2".parse().unwrap(), t0).is_none());
}

#[test]
fn dashboard_ticket_single_use() {
    let tk = crate::api::auth::DashboardTickets::new();
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
    assert!(!bearer_token_ok(
        "POST /api HTTP/1.1\r\nX-A: 1\r\n",
        "secret123"
    ));
}

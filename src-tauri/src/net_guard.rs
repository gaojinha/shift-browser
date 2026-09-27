//! 内网 / 私有地址守卫（SSRF 防护 · M 类内网 URL 策略 / O1 / O6 · 2026-09-25）
//!
//! 两处共用同一判定：
//! - `api/handlers/page.rs`：`open_and_read` 入口 + 最终 URL 复核（异步，查库取白名单）
//! - `engine/webview.rs`：GTK `decide-policy` 逐跳导航拦截（同步回调，读进程内白名单缓存）
//!
//! 域名解析到内网（DNS rebinding）：`open_and_read` 的 guard 已接 `ai::check_resolved_ips`
//! 做解析后复核（fail-closed）；WebKit 导航自身会二次解析，残余窗口靠入口 + 最终 URL
//! 双次复核压窄，彻底 pin 死需浏览器层支持，暂接受此口径。

use std::net::IpAddr;
use std::sync::{OnceLock, RwLock};

/// 内网 / 私有主机判定（IP 字面量 + localhost）。域名返回 false（不做解析级判定）。
pub fn host_is_private(host: &str) -> bool {
    let h = host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase();
    if h == "localhost" || h.ends_with(".localhost") {
        return true;
    }
    match h.parse::<IpAddr>() {
        Ok(ip) => ip_is_private(ip),
        Err(_) => false,
    }
}

/// IP 是否属于私有 / 内网 / 特殊用途网段
pub fn ip_is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xc0) == 64 // 100.64.0.0/10 CGNAT
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

/// 取 URL 主机（小写）
pub fn url_host(url: &str) -> Option<String> {
    tauri::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

static ALLOWLIST: OnceLock<RwLock<String>> = OnceLock::new();

fn allowlist_cell() -> &'static RwLock<String> {
    ALLOWLIST.get_or_init(|| RwLock::new(String::new()))
}

/// 刷新进程内 `api_allowed_ips` 缓存（`guard_url` 每次查库后调用，供同步导航守卫读取）。
pub fn set_allowlist(value: &str) {
    if let Ok(mut w) = allowlist_cell().write() {
        *w = value.to_string();
    }
}

/// 读进程内白名单缓存
pub fn allowlist() -> String {
    allowlist_cell()
        .read()
        .map(|r| r.clone())
        .unwrap_or_default()
}

/// 同步判定：主机是否被允许（非私有恒允许；私有需非空白名单覆盖）
pub fn host_permitted(host: &str) -> bool {
    if !host_is_private(host) {
        return true;
    }
    let allow = allowlist();
    !allow.trim().is_empty() && crate::api::auth::ip_allowed(host, &allow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_internal_targets() {
        for h in [
            "localhost",
            "app.localhost",
            "127.0.0.1",
            "10.0.0.5",
            "172.16.0.1",
            "192.168.1.100",
            "169.254.1.1",
            "100.64.0.1",
            "[::1]",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(host_is_private(h), "{h} 应判定为内网/私有");
        }
    }

    #[test]
    fn allows_public_targets() {
        for h in [
            "example.com",
            "8.8.8.8",
            "172.32.0.1",
            "192.169.1.1",
            "2001:4860:4860::8888",
        ] {
            assert!(!host_is_private(h), "{h} 不应判定为内网");
        }
    }

    #[test]
    fn url_host_helpers() {
        assert!(host_is_private(&url_host("http://192.168.1.1/admin").unwrap()));
        assert!(!host_is_private(&url_host("https://example.com/x").unwrap()));
        assert_eq!(url_host("https://Example.COM/a"), Some("example.com".to_string()));
    }

    #[test]
    fn permitted_requires_explicit_allowlist_for_private() {
        // 空缓存：私有一律不放行；公网恒放行
        set_allowlist("");
        assert!(!host_permitted("192.168.1.5"));
        assert!(host_permitted("example.com"));
        // 显式白名单覆盖后放行
        set_allowlist("192.168.1.0/24");
        assert!(host_permitted("192.168.1.5"));
        assert!(!host_permitted("10.0.0.1"));
        // 清理，避免影响其他测试
        set_allowlist("");
    }
}

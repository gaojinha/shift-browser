//! 内网 / 私有地址守卫（SSRF 防护 · M 类内网 URL 策略 / O1 / O6 · 2026-09-25）
//!
//! 两处共用同一判定：
//! - `api/handlers/page.rs`：`open_and_read` 入口 + 最终 URL 复核（异步，查库取白名单）
//! - `engine/webview.rs`：GTK `decide-policy` 逐跳导航拦截（同步回调，读进程内白名单缓存）
//!
//! 判定口径唯一来源：`ai::ssrf`（与模型网关/阅读器同源）。
//! 本模块只做白名单缓存与放行组装，不再手写第二套私有判定。
//! 域名解析后复核（DNS rebinding）由 `open_and_read` 的 guard 做，导航守卫只做字符串级判定。

use std::sync::{OnceLock, RwLock};

/// 内网 / 私有主机判定（统一口径：委托 ai::ssrf，与模型网关/阅读器同源）。
/// 覆盖：localhost 及内网域名后缀、标准 IP 字面量、IPv4 混淆写法
/// （0x7f000001/2130706433/127.1 等）、IPv4-mapped IPv6。
/// 域名返回 false（不做解析级判定，解析复核由调用方做）。
pub fn host_is_private(host: &str) -> bool {
    crate::ai::is_private_host(host)
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
    fn blocks_obfuscated_and_mapped_bypass() {
        // IPv4 混淆写法（浏览器会解析，旧字符串判定直接放行）
        for h in [
            "0x7f000001",
            "0X7F000001",
            "2130706433",
            "017700000001",
            "127.1",
            "127.0.1",
            "0xC0.0xA8.0x1.0x1",
        ] {
            assert!(host_is_private(h), "{h} 应判定为内网/私有");
        }
        // IPv4-mapped IPv6 按 v4 判定
        for h in [
            "::ffff:127.0.0.1",
            "::ffff:192.168.1.1",
            "[::ffff:10.0.0.1]",
        ] {
            assert!(host_is_private(h), "{h} 应判定为内网/私有");
        }
        // 组播 / 废弃站点本地 / 运营商内网 / 文档地址
        for h in ["ff02::1", "224.0.0.1", "fec0::1", "100.64.0.1", "192.0.2.1"] {
            assert!(host_is_private(h), "{h} 应判定为内网/私有");
        }
        // 公网混淆写法不误伤
        assert!(!host_is_private("0x08080808"));
    }

    #[test]
    fn url_host_helpers() {
        assert!(host_is_private(
            &url_host("http://192.168.1.1/admin").unwrap()
        ));
        assert!(!host_is_private(
            &url_host("https://example.com/x").unwrap()
        ));
        assert_eq!(
            url_host("https://Example.COM/a"),
            Some("example.com".to_string())
        );
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

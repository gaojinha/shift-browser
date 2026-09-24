//! AI 上游地址 SSRF 防护（R2-1 拆分：从 ai.rs 抽出）
//!
//! https-only + 拒绝本机/内网字面地址（含 IPv4 混淆、IPv4-mapped IPv6 绕过）、
//! 域名解析后 pin 死公网地址闭合 DNS-rebinding TOCTOU。供 ai.rs 的聊天实现、
//! reader.rs 的 fetch 与 api/dashboard.rs 的模型网关共用（统一口径）。

/// 校验 AI API 地址（SSRF 防护）：
/// - 只允许 https://（拒绝 http 明文，避免 key 走明文）
/// - 拒绝 URL 内嵌用户名/密码（userinfo@）
/// - 拒绝本机/内网字面地址（localhost、127.x、10.x、172.16-31.x、192.168.x、169.254.x、0.0.0.0、::1 等）
///
/// 说明：域名解析成内网 IP 的 DNS rebinding 属于高级攻击面，
/// 这里做静态校验挡住绝大多数场景（恶意页面无法让 key 发往任意内网/本机）。
pub(crate) fn validate_base_url(base_url: &str) -> Result<String, String> {
    let parsed = url::Url::parse(base_url).map_err(|_| "API 地址格式不合法".to_string())?;
    if parsed.scheme() != "https" {
        return Err("API 地址只允许 https://（防止 API Key 明文传输）".to_string());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("API 地址不能包含用户名/密码".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "API 地址缺少主机名".to_string())?;
    if is_private_host(host) {
        return Err(format!("API 地址不允许指向本机/内网: {}", host));
    }
    Ok(base_url.to_string())
}

fn is_private_host(host: &str) -> bool {
    // url crate 的 host_str() 对 IPv6 返回带方括号的 "[::1]"，先去掉再判断
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');
    // 1. 标准 IP 字面量（含 IPv4-mapped IPv6 的映射回判）
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }
    // 2. IPv4 混淆格式（十六进制/八进制/十进制整数、省略段）——解析后按 v4 判断
    if let Some(v4) = parse_obfuscated_ipv4(host) {
        return is_private_v4(v4);
    }
    // 3. 域名：localhost 及常见内网域名后缀
    let h = host.to_ascii_lowercase();
    h == "localhost"
        || h.ends_with(".localhost")
        || h.ends_with(".local")
        || h.ends_with(".internal")
        || h.ends_with(".lan")
        || h.ends_with(".home")
}

/// 判断 IP 是否为本机/内网（v4/v6 统一入口；reader/api 等所有 SSRF 判定共用，
/// 避免各处私有判定口径漂移——reader 旧实现就漏判了 IPv4-mapped IPv6）
pub(crate) fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_v4(v4),
        std::net::IpAddr::V6(v6) => {
            // IPv4-mapped IPv6（::ffff:a.b.c.d）：必须按 v4 判断，
            // 否则 ::ffff:192.168.1.1 会被当成"看似公网的 v6"放行（SSRF 绕过）
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private_v4(v4);
            }
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // 以下方法在 MSRV(1.77.2) 不可用，用手动位判断：
                // fc00::/7 唯一本地（内网）、fe80::/10 链路本地、fec0::/10 站点本地（已废弃但路由器仍可能实现）
                || v6.segments()[0] & 0xfe00 == 0xfc00
                || v6.segments()[0] & 0xffc0 == 0xfe80
                || v6.segments()[0] & 0xffc0 == 0xfec0
        }
    }
}

fn is_private_v4(v4: std::net::Ipv4Addr) -> bool {
    v4.is_loopback()
        || v4.is_private()
        || v4.is_link_local()
        || v4.is_unspecified()
        || v4.is_multicast()
        || v4.is_broadcast()
}

/// 解析 IPv4 混淆写法（浏览器/系统解析器支持，标准 IpAddr::parse 不认）：
/// - 单整数：十六进制 0x7f000001、八进制 017700000001、十进制 2130706433 → 127.0.0.1
/// - 省略段：127.1 → 127.0.0.1、127.0.1 → 127.0.0.1
/// - 段内进制：127.0x0.0x0.1、127.0.1（0 前缀八进制）
pub(crate) fn parse_obfuscated_ipv4(host: &str) -> Option<std::net::Ipv4Addr> {
    let h = host.trim();
    if h.is_empty() || h.contains(':') {
        return None;
    }
    if !h.contains('.') {
        // 单整数形式
        let n = parse_int_radix(h)?;
        if n <= u32::MAX as u64 {
            return Some(std::net::Ipv4Addr::from(n as u32));
        }
        return None;
    }
    // 多段形式：1~4 段，每段 0-255；省略段时最后一段可承载多个字节（127.1 → 127.0.0.1）
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() > 4 {
        return None;
    }
    let mut bytes = [0u8; 4];
    for (i, p) in parts.iter().enumerate() {
        let v = parse_int_radix(p)?;
        if i == parts.len() - 1 && parts.len() < 4 {
            let remaining = 4 - parts.len();
            let mut v = v;
            for j in 0..=remaining {
                bytes[3 - j] = (v & 0xff) as u8;
                v >>= 8;
            }
            if v != 0 {
                return None;
            }
        } else {
            if v > 255 {
                return None;
            }
            bytes[i] = v as u8;
        }
    }
    Some(std::net::Ipv4Addr::from(bytes))
}

/// 解析带进制前缀的整数段：0x 十六进制、0 八进制、否则十进制
fn parse_int_radix(s: &str) -> Option<u64> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).ok()
    } else if s.len() > 1 && s.starts_with('0') {
        u64::from_str_radix(&s[1..], 8).ok()
    } else {
        s.parse::<u64>().ok()
    }
}

/// 对一次 DNS 解析的结果做 fail-closed 校验并产出 pin 地址（P0-6 核心纯函数）。
/// - 任一结果落内网/保留段即整体拒绝——双答 DNS（公网+内网混答）无法靠"碰运气
///   连到公网"或"校验时只看到公网"绕过
/// - 空结果同样拒绝（不能把"解析不到地址"当成放行）
///
/// 通过的地址统一封装为带 `port` 的 SocketAddr，供 reqwest resolve_to_addrs /
/// curl --resolve 直接使用，使建连阶段不再二次解析，闭合 TOCTOU。
pub(crate) fn check_resolved_ips(
  host: &str,
  ips: impl IntoIterator<Item = std::net::IpAddr>,
  port: u16,
) -> Result<Vec<std::net::SocketAddr>, String> {
  let mut out = Vec::new();
  let mut count = 0u32;
  for ip in ips {
    count += 1;
    if is_private_ip(ip) {
      return Err(format!("域名 {host} 解析到本机/内网地址 {ip}，已拒绝（SSRF 防护）"));
    }
    out.push(std::net::SocketAddr::new(ip, port));
  }
  if count == 0 {
    return Err(format!("域名 {host} 没有可用的 DNS 解析结果，已拒绝（SSRF 防护）"));
  }
  Ok(out)
}

/// 解析已通过 `validate_base_url` 静态校验的 https URL，返回 pin 所需信息（P0-6）。
/// 返回 `(resolve_key, pinned)`：
/// - 标准 IP 字面量（静态校验已保证公网）：`pinned = None`，无 DNS 面，客户端直连即可
/// - 域名 / 数字混淆主机：`pinned = 本次解析出的全部公网地址`，调用方必须用
///   `reqwest::ClientBuilder::resolve_to_addrs(resolve_key, &pinned)` 注册，
///   reqwest 建连时只在这些地址间选择、不再重新解析（DNS-rebinding TOCTOU 闭合）
pub(crate) async fn resolve_pin_for_https(
  url: &str,
) -> Result<(String, Option<Vec<std::net::SocketAddr>>), String> {
  let parsed = url::Url::parse(url).map_err(|_| "API 地址格式不合法".to_string())?;
  let port = parsed.port_or_known_default().unwrap_or(443);
  let raw = parsed
    .host_str()
    .ok_or_else(|| "API 地址缺少主机名".to_string())?;
  // url crate 对 IPv6 返回带方括号的 "[::1]"，解析/resolve key 都用去括号形式
  let host = raw.trim_start_matches('[').trim_end_matches(']').to_string();
  // 标准 IP 字面量：validate_base_url 已拒绝内网/混淆写法，公网字面量无需 pin
  if host.parse::<std::net::IpAddr>().is_ok() {
    return Ok((host, None));
  }
  // IPv4 数字混淆主机（如 0x08080808）：归一化后按规范 IP pin；
  // 内网写法已在 validate_base_url 静态拒绝，这里 fail-closed 再判一次
  if let Some(v4) = parse_obfuscated_ipv4(&host) {
    let addrs = check_resolved_ips(
      &host,
      std::iter::once(std::net::IpAddr::V4(v4)),
      port,
    )?;
    return Ok((host, Some(addrs)));
  }
  // 域名：只解析这一次，校验全部结果，随后 pin 死交给调用方
  let sock: Vec<_> = tokio::net::lookup_host((host.as_str(), port))
    .await
    .map_err(|e| format!("域名解析失败: {host} ({e})"))?
    .collect();
  let addrs = check_resolved_ips(&host, sock.iter().map(|s| s.ip()), port)?;
  Ok((host, Some(addrs)))
}

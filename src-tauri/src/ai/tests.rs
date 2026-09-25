//! ai 安全测试（R1 外置 · 2026-09-25）：base URL SSRF 校验、混淆 IP、DNS pin fail-closed。

use super::*;
use super::ssrf::parse_obfuscated_ipv4;

#[test]
fn base_url_accepts_https_public() {
  assert!(validate_base_url("https://apihub.agnes-ai.com/v1").is_ok());
  assert!(validate_base_url("https://api.openai.com/v1").is_ok());
}

#[test]
fn base_url_rejects_plain_http() {
  assert!(validate_base_url("http://apihub.agnes-ai.com/v1").is_err());
}

#[test]
fn base_url_rejects_private_and_local() {
  // 本机/内网 IP
  assert!(validate_base_url("https://127.0.0.1:11434/v1").is_err());
  assert!(validate_base_url("https://localhost:11434/v1").is_err());
  assert!(validate_base_url("https://192.168.1.1/v1").is_err());
  assert!(validate_base_url("https://10.0.0.1/v1").is_err());
  assert!(validate_base_url("https://172.16.0.1/v1").is_err());
  assert!(validate_base_url("https://169.254.169.254/latest/meta-data").is_err());
  // 内网域名后缀
  assert!(validate_base_url("https://router.local/v1").is_err());
  assert!(validate_base_url("https://nas.home/v1").is_err());
  assert!(validate_base_url("https://myhost.lan/v1").is_err());
}

#[test]
fn base_url_rejects_userinfo() {
  assert!(validate_base_url("https://user:pass@apihub.agnes-ai.com/v1").is_err());
}

#[test]
fn base_url_rejects_ipv6_private_and_mapped() {
  // IPv6 内网/链路本地/站点本地
  assert!(validate_base_url("https://[::1]:11434/v1").is_err());
  assert!(validate_base_url("https://[fc00::1]/v1").is_err());
  assert!(validate_base_url("https://[fd12:3456::1]/v1").is_err());
  assert!(validate_base_url("https://[fe80::1]/v1").is_err());
  assert!(validate_base_url("https://[fec0::1]/v1").is_err());
  // IPv4-mapped IPv6：必须按 v4 判内网（SSRF 绕过点）
  assert!(validate_base_url("https://[::ffff:127.0.0.1]/v1").is_err());
  assert!(validate_base_url("https://[::ffff:192.168.1.1]/v1").is_err());
  assert!(validate_base_url("https://[::ffff:10.0.0.1]/v1").is_err());
  // 公网 v6 放行
  assert!(validate_base_url("https://[2606:4700::1111]/v1").is_ok());
}

#[test]
fn base_url_rejects_obfuscated_ipv4() {
  // 十六进制/八进制/十进制整数混淆 → 127.0.0.1
  assert!(validate_base_url("https://0x7f000001/v1").is_err());
  assert!(validate_base_url("https://017700000001/v1").is_err());
  assert!(validate_base_url("https://2130706433/v1").is_err());
  // 省略段混淆 → 127.0.0.1
  assert!(validate_base_url("https://127.1/v1").is_err());
  assert!(validate_base_url("https://127.0.1/v1").is_err());
  // 段内进制：127.0x0.0x0.1 → 127.0.0.1
  assert!(validate_base_url("https://127.0x0.0x0.1/v1").is_err());
  // 混淆的内网 192.168.1.1
  assert!(validate_base_url("https://0xc0a80101/v1").is_err());
  assert!(validate_base_url("https://3232235777/v1").is_err());
}

#[test]
fn parse_obfuscated_ipv4_works() {
  use std::net::Ipv4Addr;
  assert_eq!(
    parse_obfuscated_ipv4("0x7f000001"),
    Some(Ipv4Addr::new(127, 0, 0, 1))
  );
  assert_eq!(
    parse_obfuscated_ipv4("2130706433"),
    Some(Ipv4Addr::new(127, 0, 0, 1))
  );
  assert_eq!(
    parse_obfuscated_ipv4("127.1"),
    Some(Ipv4Addr::new(127, 0, 0, 1))
  );
  assert_eq!(
    parse_obfuscated_ipv4("127.0.1"),
    Some(Ipv4Addr::new(127, 0, 0, 1))
  );
  assert_eq!(
    parse_obfuscated_ipv4("127.0x0.0x0.1"),
    Some(Ipv4Addr::new(127, 0, 0, 1))
  );
  assert_eq!(
    parse_obfuscated_ipv4("0xc0a80101"),
    Some(Ipv4Addr::new(192, 168, 1, 1))
  );
  // 非法/非混淆格式返回 None
  assert_eq!(parse_obfuscated_ipv4("example.com"), None);
  assert_eq!(parse_obfuscated_ipv4("999.1.1.1"), None);
  assert_eq!(parse_obfuscated_ipv4("256.0.0.1"), None);
}

#[test]
fn pin_accepts_all_public_addrs_and_carries_port() {
  // 公网 v4 + 公网 v6 全部放行，pin 地址带上请求端口
  let ips = [
    "93.184.216.34".parse::<std::net::IpAddr>().unwrap(),
    "2606:4700::1111".parse::<std::net::IpAddr>().unwrap(),
  ];
  let addrs = check_resolved_ips("example.com", ips, 8443).unwrap();
  assert_eq!(addrs.len(), 2);
  assert!(addrs.iter().all(|a| a.port() == 8443));
}

#[test]
fn pin_rejects_dual_answer_dns_with_one_internal_ip() {
  // P0-6 核心场景：双答 DNS 公网+内网混答，必须 fail-closed 整体拒绝，
  // 不能"连到公网那个就行"——攻击者靠交替应答在校验后换内网
  let ips = [
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(93, 184, 216, 34)),
    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
  ];
  assert!(check_resolved_ips("rebind.evil", ips, 443).is_err());
  // 云元数据地址混答同样拒绝
  let ips2 = [
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(93, 184, 216, 34)),
    std::net::IpAddr::V4(std::net::Ipv4Addr::new(169, 254, 169, 254)),
  ];
  assert!(check_resolved_ips("rebind.evil", ips2, 443).is_err());
}

#[test]
fn pin_rejects_all_private_v4_ranges() {
  for bad in [
    "127.0.0.1",
    "10.1.2.3",
    "172.16.5.5",
    "192.168.9.9",
    "169.254.169.254",
    "0.0.0.0",
    "224.0.0.1",
    "255.255.255.255",
  ] {
    let ip: std::net::IpAddr = bad.parse().unwrap();
    assert!(check_resolved_ips("x.test", [ip], 443).is_err(), "{bad} 应拒绝");
  }
}

#[test]
fn pin_rejects_native_and_mapped_v6_internal() {
  for bad in [
    "::1",
    "fc00::1",
    "fdff::1",
    "fe80::1",
    "fec0::1",
    "ff02::1",
    // IPv4-mapped 绕过形态必须按 v4 判定
    "::ffff:127.0.0.1",
    "::ffff:169.254.169.254",
    "::ffff:10.0.0.1",
  ] {
    let ip: std::net::IpAddr = bad.parse().unwrap();
    assert!(check_resolved_ips("x.test", [ip], 443).is_err(), "{bad} 应拒绝");
  }
}

#[test]
fn pin_rejects_empty_dns_answer() {
  // 空解析结果不能被当成放行（旧实现遍历空集合直接 Ok）
  assert!(check_resolved_ips(
    "nx.example",
    std::iter::empty::<std::net::IpAddr>(),
    443
  )
  .is_err());
}

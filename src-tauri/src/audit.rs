//! 零遥测审计（阶段 2 · 隐私主权）：全量网络出口清单 + 可验证"无外联"
//!
//! 通过 /proc 枚举本应用**及其全部子进程**（WebKitWebProcess / NetworkProcess 等）
//! 当前持有的 TCP 连接，列出每个远端端点（IP:端口）——让"这个浏览器到底连了哪儿"
//! 可查、可验，对齐阶段 2 铁律「信任需要证明」。
//!
//! 说明：这是**只读**审计，不拦截、不改包。只读 /proc 的 fd 符号链接与
//! /proc/net/tcp{,6}，全程本地、零出网。

use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// 一条外联连接
#[derive(Debug, Serialize, Clone)]
pub struct EgressConn {
    /// 持有该连接的进程 PID（应用自身或子进程）
    pub pid: u32,
    /// 协议：tcp / tcp6
    pub proto: String,
    /// 远端 IP
    pub remote_ip: String,
    /// 远端端口
    pub remote_port: u16,
    /// 连接状态（ESTABLISHED / SYN_SENT）
    pub state: String,
}

/// 外联审计报告
#[derive(Debug, Serialize, Clone)]
pub struct EgressReport {
    /// 扫描的进程数（应用 + 子进程）
    pub processes: usize,
    /// 当前连接数
    pub connections: Vec<EgressConn>,
    /// 去重后的远端端点个数
    pub distinct_remotes: usize,
    /// 回环（本机）连接数
    pub loopback: usize,
    /// 外网连接数
    pub external: usize,
    /// 扫描时间（unix 秒）
    pub checked_at: i64,
    /// 说明
    pub note: String,
}

/// /proc/net/tcp{,6} 的一行解析结果
#[derive(Debug, Clone, PartialEq)]
pub struct TcpEntry {
    pub local_ip: String,
    pub local_port: u16,
    pub remote_ip: String,
    pub remote_port: u16,
    /// 内核状态码（16 进制解析后的 u8）
    pub state: u8,
    pub inode: u64,
}

/// 小端 hex → IPv4 字符串（/proc 里地址是 8 位 hex，字节序反转）
pub fn hex_le_ipv4(s: &str) -> Option<String> {
    if s.len() != 8 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    let b = n.to_le_bytes();
    Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]))
}

/// 小端 hex → IPv6 字符串（32 位 hex，4 组 u32 各反转字节）
pub fn hex_le_ipv6(s: &str) -> Option<String> {
    if s.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for g in 0..4 {
        let word = u32::from_str_radix(&s[g * 8..g * 8 + 8], 16).ok()?;
        bytes[g * 4..g * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Some(std::net::Ipv6Addr::from(bytes).to_string())
}

/// 内核 TCP 状态码 → 名称
pub fn state_name(st: u8) -> &'static str {
    match st {
        0x01 => "ESTABLISHED",
        0x02 => "SYN_SENT",
        0x03 => "SYN_RECV",
        0x04 => "FIN_WAIT1",
        0x05 => "FIN_WAIT2",
        0x06 => "TIME_WAIT",
        0x07 => "CLOSE",
        0x08 => "CLOSE_WAIT",
        0x09 => "LAST_ACK",
        0x0a => "LISTEN",
        0x0b => "CLOSING",
        _ => "UNKNOWN",
    }
}

/// 是否回环地址（本机，不算外联）
pub fn is_loopback(ip: &str) -> bool {
    ip.starts_with("127.") || ip == "::1"
}

/// 解析 /proc/net/tcp{,6} 全文
pub fn parse_net_tcp(content: &str, v6: bool) -> Vec<TcpEntry> {
    let mut out = Vec::new();
    for line in content.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 10 {
            continue;
        }
        let (local_ip, local_port) = match split_addr(f[1], v6) {
            Some(v) => v,
            None => continue,
        };
        let (remote_ip, remote_port) = match split_addr(f[2], v6) {
            Some(v) => v,
            None => continue,
        };
        let state = u8::from_str_radix(f[3], 16).unwrap_or(0);
        let inode: u64 = f[9].parse().unwrap_or(0);
        out.push(TcpEntry {
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            state,
            inode,
        });
    }
    out
}

/// 拆分 "HEXADDR:PORT" → (ip, port)
fn split_addr(s: &str, v6: bool) -> Option<(String, u16)> {
    let (addr, port) = s.split_once(':')?;
    let ip = if v6 {
        hex_le_ipv6(addr)?
    } else {
        hex_le_ipv4(addr)?
    };
    let port = u16::from_str_radix(port, 16).ok()?;
    Some((ip, port))
}

/// 递归收集进程及其全部子进程 PID
fn process_tree(root: u32) -> Vec<u32> {
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        let pid = all[i];
        let path = format!("/proc/{}/task/{}/children", pid, pid);
        if let Ok(s) = std::fs::read_to_string(path) {
            for t in s.split_whitespace() {
                if let Ok(c) = t.parse::<u32>() {
                    if !all.contains(&c) {
                        all.push(c);
                    }
                }
            }
        }
        i += 1;
    }
    all
}

/// 某进程持有的 socket inode 集合（读 /proc/<pid>/fd 符号链接）
fn socket_inodes(pid: u32) -> HashSet<u64> {
    let mut set = HashSet::new();
    if let Ok(rd) = std::fs::read_dir(format!("/proc/{}/fd", pid)) {
        for e in rd.flatten() {
            if let Ok(target) = std::fs::read_link(e.path()) {
                let s = target.to_string_lossy();
                if let Some(rest) = s.strip_prefix("socket:[") {
                    if let Some(num) = rest.strip_suffix(']') {
                        if let Ok(i) = num.parse::<u64>() {
                            set.insert(i);
                        }
                    }
                }
            }
        }
    }
    set
}

/// 采集当前外联清单（应用 + 子进程）
pub fn collect_egress() -> EgressReport {
    let me = std::process::id();
    let pids = process_tree(me);

    // inode → 持有进程 PID
    let mut inode_pid: HashMap<u64, u32> = HashMap::new();
    for &p in &pids {
        for i in socket_inodes(p) {
            inode_pid.insert(i, p);
        }
    }

    let mut conns: Vec<EgressConn> = Vec::new();
    for (path, v6, proto) in [
        ("/proc/net/tcp", false, "tcp"),
        ("/proc/net/tcp6", true, "tcp6"),
    ] {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        for e in parse_net_tcp(&content, v6) {
            // 只看已建立 / 正在建连的；LISTEN 等跳过
            if e.state != 0x01 && e.state != 0x02 {
                continue;
            }
            let Some(&pid) = inode_pid.get(&e.inode) else {
                continue;
            };
            conns.push(EgressConn {
                pid,
                proto: proto.to_string(),
                remote_ip: e.remote_ip,
                remote_port: e.remote_port,
                state: state_name(e.state).to_string(),
            });
        }
    }

    // 去重（同 pid+proto+remote）+ 排序（外网在前）
    conns.sort_by(|a, b| {
        is_loopback(&a.remote_ip)
            .cmp(&is_loopback(&b.remote_ip))
            .then(a.remote_ip.cmp(&b.remote_ip))
            .then(a.remote_port.cmp(&b.remote_port))
    });
    conns.dedup_by(|a, b| {
        a.pid == b.pid && a.proto == b.proto && a.remote_ip == b.remote_ip && a.remote_port == b.remote_port
    });

    let distinct: HashSet<(String, u16)> = conns
        .iter()
        .map(|c| (c.remote_ip.clone(), c.remote_port))
        .collect();
    let loopback = conns.iter().filter(|c| is_loopback(&c.remote_ip)).count();
    let external = conns.len() - loopback;

    EgressReport {
        processes: pids.len(),
        distinct_remotes: distinct.len(),
        connections: conns,
        loopback,
        external,
        checked_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0),
        note: format!(
            "扫描本应用及其 {} 个子进程的当前 TCP 连接（只读 /proc，零出网）",
            pids.len().saturating_sub(1)
        ),
    }
}

/// 零遥测审计：列出本应用当前的全部网络出口连接。
#[tauri::command]
pub async fn egress_audit() -> Result<EgressReport, String> {
    Ok(collect_egress())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_little_endian() {
        // 127.0.0.1 → 0100007F
        assert_eq!(hex_le_ipv4("0100007F").as_deref(), Some("127.0.0.1"));
        // 1.1.1.1 → 01010101（对称，验不出端序）；用 8.8.8.8 的反序
        // 8.8.8.8 小端 hex = 08080808
        assert_eq!(hex_le_ipv4("08080808").as_deref(), Some("8.8.8.8"));
        assert_eq!(hex_le_ipv4("0100007F00"), None);
        assert_eq!(hex_le_ipv4("zzzzzzzz"), None);
    }

    #[test]
    fn ipv6_little_endian() {
        // ::1 → 00000000000000000000000001000000
        assert_eq!(
            hex_le_ipv6("00000000000000000000000001000000").as_deref(),
            Some("::1")
        );
        assert_eq!(hex_le_ipv6("abc"), None);
    }

    #[test]
    fn loopback_detection() {
        assert!(is_loopback("127.0.0.1"));
        assert!(is_loopback("127.5.5.5"));
        assert!(is_loopback("::1"));
        assert!(!is_loopback("8.8.8.8"));
        assert!(!is_loopback("192.168.1.1"));
    }

    #[test]
    fn state_names() {
        assert_eq!(state_name(0x01), "ESTABLISHED");
        assert_eq!(state_name(0x02), "SYN_SENT");
        assert_eq!(state_name(0x0a), "LISTEN");
        assert_eq!(state_name(0xff), "UNKNOWN");
    }

    #[test]
    fn parse_net_tcp_line() {
        let content = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000000000000000 100 0 0 10 0\n\
   1: 0100007F:C350 5DB8D822:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 22222 1 0000000000000000 100 0 0 10 0\n";
        let entries = parse_net_tcp(content, false);
        assert_eq!(entries.len(), 2);
        // 第一行 LISTEN，远端 0.0.0.0:0
        assert_eq!(entries[0].state, 0x0a);
        assert_eq!(entries[0].remote_ip, "0.0.0.0");
        assert_eq!(entries[0].inode, 11111);
        // 第二行 ESTABLISHED，远端 34.216.184.93:443
        assert_eq!(entries[1].state, 0x01);
        assert_eq!(entries[1].remote_ip, "34.216.184.93");
        assert_eq!(entries[1].remote_port, 443);
        assert_eq!(entries[1].inode, 22222);
    }

    #[test]
    fn collect_egress_runs() {
        // 冒烟：应能正常返回（至少包含自身进程），不 panic
        let r = collect_egress();
        assert!(r.processes >= 1);
        assert!(r.note.contains("零出网"));
    }
}

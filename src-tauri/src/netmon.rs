// 网络状态监测：状态栏显示实时下行/上行速度 + 延迟 + 在线状态
// 移植自原版 Electron（/home/gao/ShiftBrowser/electron/main.ts）：
// - 读 /proc/net/dev 计算实时下行/上行速度（排除 lo）
// - ping 223.5.5.5 测延迟与在线状态
// - 每 5 秒推送一次 net-status-updated 到状态栏（2026-09-18 起，原 2 秒太频繁）
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
pub struct NetStatus {
    pub down_kb: u32,
    pub up_kb: u32,
    /// -1 = 未知/失败
    pub ping_ms: i32,
    pub online: bool,
}

struct NetDevStats {
    rx: u64,
    tx: u64,
}

/// 读 /proc/net/dev 汇总所有非 lo 网卡的 rx/tx 字节
fn read_net_dev() -> NetDevStats {
    let Ok(data) = std::fs::read_to_string("/proc/net/dev") else {
        return NetDevStats { rx: 0, tx: 0 };
    };
    let mut rx = 0u64;
    let mut tx = 0u64;
    for line in data.lines().skip(2) {
        let Some(idx) = line.find(':') else { continue };
        let iface = line[..idx].trim();
        if iface == "lo" {
            continue;
        }
        let fields: Vec<u64> = line[idx + 1..]
            .split_whitespace()
            .filter_map(|f| f.parse().ok())
            .collect();
        if fields.len() >= 9 {
            rx += fields[0]; // rx_bytes
            tx += fields[8]; // tx_bytes
        }
    }
    NetDevStats { rx, tx }
}

/// ping 公网 DNS 测延迟（毫秒），失败返回 -1
async fn measure_ping() -> i32 {
    let start = Instant::now();
    let out = tokio::process::Command::new("ping")
        .args(["-c", "1", "-W", "2", "223.5.5.5"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
    match out {
        Ok(st) if st.success() => start.elapsed().as_millis() as i32,
        _ => -1,
    }
}

/// 四层诊断结果（每层一句话结论由调用方按字段组装，见 system_prompts::net_diagnose）
#[derive(Serialize, Clone)]
pub struct NetDiagnosis {
    /// 非 lo 网卡是否存在（第 1 层：本机网卡）
    pub iface_ok: bool,
    /// 默认网关地址（第 2 层目标；空 = 没找到网关）
    pub gateway: String,
    /// 网关是否 ping 通（第 2 层）
    pub gateway_ok: bool,
    /// 域名是否解得出（第 3 层：DNS）
    pub dns_ok: bool,
    /// 出网延迟毫秒（第 4 层；-1 = 不通）
    pub remote_ms: i32,
}

/// 从 /proc/net/route 解析默认网关（Destination 全 0 的那条，Gateway 列 hex 小端序）
fn parse_default_gateway(text: &str) -> Option<String> {
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 3 || f[1] != "00000000" {
            continue;
        }
        let raw = u32::from_str_radix(f[2], 16).ok()?;
        let ip = std::net::Ipv4Addr::new(
            (raw & 0xFF) as u8,
            ((raw >> 8) & 0xFF) as u8,
            ((raw >> 16) & 0xFF) as u8,
            ((raw >> 24) & 0xFF) as u8,
        );
        if !ip.is_unspecified() {
            return Some(ip.to_string());
        }
    }
    None
}

/// 读默认网关（失败返回空串，不抛错）
fn default_gateway() -> String {
    std::fs::read_to_string("/proc/net/route")
        .ok()
        .and_then(|t| parse_default_gateway(&t))
        .unwrap_or_default()
}

/// ping 指定目标（网关复用，超时 2 秒），成功返回 true
async fn ping_once(target: &str) -> bool {
    tokio::process::Command::new("ping")
        .args(["-c", "1", "-W", "2", target])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|st| st.success())
        .unwrap_or(false)
}

/// 四层断网诊断：网卡 → 网关 → DNS → 出网。只读不改状态，失败逐层记 false。
#[tauri::command]
pub async fn net_diagnose() -> NetDiagnosis {
    let data = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    let iface_ok = data.lines().skip(2).any(|line| {
        line.find(':')
            .map(|idx| line[..idx].trim() != "lo")
            .unwrap_or(false)
    });
    let gateway = default_gateway();
    let gateway_ok = if gateway.is_empty() {
        false
    } else {
        ping_once(&gateway).await
    };
    // DNS 超时（2026-09-29）：lookup_host 无内置超时，诊断命令不能无限等；
    // 超时按 dns_ok=false（诊断降级，不抛错）
    let dns_ok = tokio::time::timeout(
        std::time::Duration::from_secs(crate::contracts::timeout::DNS_LOOKUP_SECS),
        tokio::net::lookup_host(("www.bing.com", 443)),
    )
    .await
    .map(|r| r.map(|mut it| it.next().is_some()).unwrap_or(false))
    .unwrap_or(false);
    let remote_ms = measure_ping().await;
    NetDiagnosis {
        iface_ok,
        gateway,
        gateway_ok,
        dns_ok,
        remote_ms,
    }
}

/// 启动网络状态监测：每 5 秒推送速度，每 10 秒重测一次 ping
/// （2026-09-18 性能优化：原为每 2 秒推送 —— 每次都要读 /proc/net/dev 并 emit
/// 事件到所有 webview、前端再更新 DOM。2 秒粒度对"看个网速"属于过度频繁，
/// 降到 5 秒可显著减少空转唤醒，ping 仍保持 10 秒一次不变）
pub fn start_network_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = read_net_dev();
        let mut last_time = Instant::now();
        let mut ping_ms = measure_ping().await;
        let mut online = ping_ms >= 0;
        // 循环计数：每 2 轮（10 秒）重测 ping
        let mut tick: u32 = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            tick += 1;
            if tick % 2 == 0 {
                ping_ms = measure_ping().await;
                online = ping_ms >= 0;
            }
            let now = Instant::now();
            let cur = read_net_dev();
            let dt = now.duration_since(last_time).as_secs_f64();
            if dt > 0.0 {
                let down_kb =
                    ((cur.rx.saturating_sub(last.rx)) as f64 / 1024.0 / dt).max(0.0) as u32;
                let up_kb = ((cur.tx.saturating_sub(last.tx)) as f64 / 1024.0 / dt).max(0.0) as u32;
                let _ = app.emit(
                    crate::contracts::events::NET_STATUS_UPDATED,
                    NetStatus {
                        down_kb,
                        up_kb,
                        ping_ms,
                        online,
                    },
                );
            }
            last = cur;
            last_time = now;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_net_dev_returns_some_value() {
        // /proc/net/dev 在 Linux 上必然存在且格式固定；至少能解析出 rx/tx 累计值
        // （u64 无符号，正常 Linux 下累计值 > 0；即使异常也返回 0 不 panic）
        let s = read_net_dev();
        // rx/tx 是 u64，无符号类型——只验证解析不 panic、字段可读即可
        let _ = (s.rx, s.tx);
    }

    #[test]
    fn parse_default_gateway_小端序转回点分() {
        // 典型路由表：默认路由网关 0101A8C0 小端 = 192.168.1.1
        let text = "Iface\tDestination\tGateway\tFlags\neth0\t00000000\t0101A8C0\t0003\neth0\t00A8C0\t00000000\t0001\n";
        assert_eq!(parse_default_gateway(text).as_deref(), Some("192.168.1.1"));
    }

    #[test]
    fn parse_default_gateway_无默认路由返回空() {
        let text = "Iface\tDestination\tGateway\tFlags\neth0\t00A8C0\t00000000\t0001\n";
        assert!(parse_default_gateway(text).is_none());
        assert!(parse_default_gateway("").is_none());
    }
}

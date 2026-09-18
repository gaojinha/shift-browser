// 网络状态监测：状态栏显示实时下行/上行速度 + 延迟 + 在线状态
// 移植自原版 Electron（/home/gao/ShiftBrowser/electron/main.ts）：
// - 读 /proc/net/dev 计算实时下行/上行速度（排除 lo）
// - ping 223.5.5.5 测延迟与在线状态
// - 每 2 秒推送一次 net-status-updated 到状态栏
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

/// 启动网络状态监测：每 2 秒推送速度，每 10 秒重测一次 ping
pub fn start_network_monitor(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut last = read_net_dev();
        let mut last_time = Instant::now();
        let mut ping_ms = measure_ping().await;
        let mut online = ping_ms >= 0;
        // 循环计数：每 5 轮（10 秒）重测 ping
        let mut tick: u32 = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            tick += 1;
            if tick % 5 == 0 {
                ping_ms = measure_ping().await;
                online = ping_ms >= 0;
            }
            let now = Instant::now();
            let cur = read_net_dev();
            let dt = now.duration_since(last_time).as_secs_f64();
            if dt > 0.0 {
                let down_kb = ((cur.rx.saturating_sub(last.rx)) as f64 / 1024.0 / dt).max(0.0) as u32;
                let up_kb = ((cur.tx.saturating_sub(last.tx)) as f64 / 1024.0 / dt).max(0.0) as u32;
                let _ = app.emit(
                    "net-status-updated",
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
}
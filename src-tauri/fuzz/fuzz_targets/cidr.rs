//! CIDR 白名单解析模糊目标：第一行喂 IP，第二行喂 CIDR。
//! 任意输入不得 panic，结果码必须落在约定区间 [-2, 1]。
#![no_main]

use libfuzzer_sys::fuzz_target;
use shift_browser_lib::fuzz_bridge::fuzz_helpers;

fuzz_target!(|data: &[u8]| {
    let mut parts = data.splitn(2, |&b| b == b'\n');
    let ip = String::from_utf8_lossy(parts.next().unwrap_or(b""));
    let cidr = String::from_utf8_lossy(parts.next().unwrap_or(b""));
    let code = fuzz_helpers::cidr_contains(&ip, &cidr);
    assert!((-2..=1).contains(&code), "非法结果码: {code}");
});

//! 手写 HTTP/1.1 请求解析器模糊目标
//!
//! 输入原始字节原样作为 TCP 流；用前 2 字节做种子，以 xorshift 确定性生成
//! 0..4 个随机切点，驱动 ChaosReader 模拟任意拆包/粘包/提前断连。
#![no_main]

use libfuzzer_sys::fuzz_target;
use shift_browser_lib::fuzz_bridge::fuzz_helpers;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    // 解析器只读内存 reader，不需要 I/O 驱动；单线程 runtime 足够且构造廉价
    RT.get_or_init(|| Runtime::new().expect("fuzz tokio runtime"))
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    // 前 2 字节决定拆包方式；其余（含这 2 字节）全部作为网络字节
    let mut x = u16::from_le_bytes([data[0], data[1]]) as u64 | 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let cut_count = next() % 5; // 0..4 个切点
    let cuts: Vec<usize> = (0..cut_count)
        .map(|_| (next() as usize) % (data.len() + 1))
        .collect();

    let code = runtime().block_on(fuzz_helpers::parse_http(data.to_vec(), cuts));
    assert!(code <= 2, "解析器返回非法结果码: {code}");
});

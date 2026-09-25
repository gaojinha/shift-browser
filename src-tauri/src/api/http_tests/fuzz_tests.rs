//! 确定性模糊测试（HANDOFF P2-5；R1 2026-09-25 从 http_tests.rs 拆分）
//!
//! 固定种子 xorshift，不引第三方依赖，CI 每次跑同一序列；
//! 覆盖引导深挖由 fuzz/ 下 cargo-fuzz 目标承担（每周 CI），二者互补。
//! 核心安全性质：任意输入 + 任意 TCP 拆包，解析器**不得 panic / 死循环**。

use super::super::*;

struct Rng {
  state: u64,
}

impl Rng {
  fn new(seed: u64) -> Self {
    Self { state: seed | 1 }
  }
  fn next_u64(&mut self) -> u64 {
    let mut x = self.state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    self.state = x;
    x
  }
  fn below(&mut self, n: usize) -> usize {
    (self.next_u64() % n as u64) as usize
  }
  fn byte(&mut self) -> u8 {
    (self.next_u64() & 0xff) as u8
  }
  fn percent(&mut self, p: usize) -> bool {
    self.below(100) < p
  }
}

/// 偏向解析器分支的字节字母表（CR/LF/冒号/数字/chunked 关键字等）
const FUZZ_ALPHABET: &[u8] =
  b"\r\n\r\n :0123456789Content-LengthTransfer-EncodingchunkedPOST/%.\t-";

fn fuzz_byte(rng: &mut Rng) -> u8 {
  if rng.percent(55) {
    FUZZ_ALPHABET[rng.below(FUZZ_ALPHABET.len())]
  } else {
    rng.byte()
  }
}

/// 对种子做随机替换/插入/删除
fn mutate(rng: &mut Rng, bytes: &mut Vec<u8>) {
  for _ in 0..rng.below(10) {
    if bytes.is_empty() {
      if rng.percent(50) {
        bytes.push(fuzz_byte(rng));
      }
      continue;
    }
    match rng.below(3) {
      0 => {
        let i = rng.below(bytes.len());
        bytes[i] = fuzz_byte(rng);
      }
      1 => {
        let i = rng.below(bytes.len() + 1);
        bytes.insert(i, fuzz_byte(rng));
      }
      _ => {
        let i = rng.below(bytes.len());
        bytes.remove(i);
      }
    }
  }
}

/// 生成 0..4 个随机绝对切点（ChaosReader 自行排序/去重/补末尾）
fn random_cuts(rng: &mut Rng, len: usize) -> Vec<usize> {
  let mut cuts = Vec::new();
  for _ in 0..rng.below(5) {
    cuts.push(rng.below(len + 1));
  }
  cuts
}

#[tokio::test]
async fn fuzz_http_parser_arbitrary_streams() {
  let mut rng = Rng::new(0x5f3a_2026_0924);
  for iter in 0..20_000 {
    let mut data = if rng.percent(65) {
      // 结构化种子：合法 POST 模板 + 各类边界 Content-Length
      let body_len = rng.below(40);
      let body: Vec<u8> = (0..body_len).map(|_| rng.byte()).collect();
      let cl = match rng.below(7) {
        0 => "0".to_string(),
        1 => body_len.to_string(),
        2 => body_len
          .saturating_add(rng.below(5))
          .saturating_add(1)
          .to_string(), // 声明比实际长 → 提前 EOF
        3 => (8 * 1024 * 1024 + 1).to_string(), // 超 8MB 上限
        4 => format!("{}abc", rng.below(100)),  // 含非数字
        5 => "-1".to_string(),                  // 负数
        _ => format!(" {}", body_len),          // 前导空格
      };
      let mut seed = String::from("POST /api HTTP/1.1\r\nHost: x\r\n");
      if rng.percent(30) {
        seed.push_str("Transfer-Encoding: chunked\r\n");
      }
      seed.push_str(&format!("Content-Length: {cl}\r\n\r\n"));
      let mut bytes = seed.into_bytes();
      bytes.extend_from_slice(&body);
      mutate(&mut rng, &mut bytes);
      // 随机截断
      bytes.truncate(rng.below(bytes.len() + 1));
      // 20% 概率追加垃圾尾巴
      if rng.percent(20) {
        let n = rng.below(30);
        bytes.extend((0..n).map(|_| fuzz_byte(&mut rng)));
      }
      bytes
    } else {
      // 纯随机字节流
      (0..rng.below(300)).map(|_| fuzz_byte(&mut rng)).collect()
    };
    // 每 500 次造一个 >64KB 无终止头，钉死头部上限分支
    if iter % 500 == 0 {
      data.extend(std::iter::repeat(b'a').take(70_000));
    }

    let cuts = random_cuts(&mut rng, data.len());
    // 与 cargo-fuzz 目标走同一个 shim 入口，只校验结果码
    let code = crate::api::fuzz_helpers::parse_http(data.clone(), cuts.clone()).await;
    assert!(code <= 2, "shim 返回非法结果码: {code}");
    // 再直接驱动私有解析器拿 head/body 做弱不变量校验
    let mut reader = crate::api::fuzz_helpers::ChaosReader::new(data, cuts);
    match read_http_request(&mut reader).await {
      HttpReq::Complete(head, body) => {
        // 弱不变量：声明了合法 CL 时，body 不会膨胀超 CL 的 3 倍（lossy 替换上限）
        if let Some(cl) = content_length_of(&head) {
          assert!(
            body.is_empty() || body.len() <= cl.saturating_mul(3),
            "body 超长 cl={cl} body_len={}",
            body.len()
          );
        }
      }
      HttpReq::TooLarge | HttpReq::Bad => {}
    }
  }
}

#[test]
fn fuzz_codec_arbitrary_inputs() {
  let mut rng = Rng::new(0xc0de_2026_0924);
  for _ in 0..30_000 {
    // 任意字符串（含残缺 %XX、+、非 ASCII）不得 panic
    let raw: String = (0..rng.below(40))
      .map(|_| {
        let b = fuzz_byte(&mut rng);
        // 映射成合法 char：保留可打印 ASCII 与少量多字节范围
        char::from_u32(if rng.percent(80) {
          (b % 95 + 32) as u32
        } else {
          0x4e00 + (b as u32)
        })
        .unwrap_or('?')
      })
      .collect();
    // 与 cargo-fuzz codec 目标走同一 shim
    let _ = crate::api::fuzz_helpers::url_decode(&raw);
    let _ = crate::api::fuzz_helpers::kv_value(&raw, "token");
    let _ = crate::api::fuzz_helpers::content_length(&raw);
    let _ = crate::api::fuzz_helpers::is_chunked(&raw);
  }

  // 性质 1：unreserved 字符经 encode→decode 往返不变
  let unreserved = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.~";
  for _ in 0..5_000 {
    let s: String = (0..rng.below(30) + 1)
      .map(|_| unreserved[rng.below(unreserved.len())] as char)
      .collect();
    assert_eq!(
      crate::api::fuzz_helpers::url_decode(&crate::api::url_encode(&s)),
      s,
      "unreserved 往返失败: {s}"
    );
  }

  // 性质 2：任意可打印 ASCII（除 & 分隔符）编码后都能从 form 中精确取回
  for _ in 0..5_000 {
    let v: String = (0..rng.below(20) + 1)
      .map(|_| {
        let mut b = (rng.below(0x5f) + 0x20) as u8; // 空格(0x20)~~(0x7e)
        if b == b'&' {
          b = b'x';
        }
        b as char
      })
      .collect();
    let form = format!("token={}", crate::api::url_encode(&v));
    assert_eq!(
      crate::api::fuzz_helpers::kv_value(&form, "token"),
      Some(v),
      "form 往返失败"
    );
  }
}

#[test]
fn fuzz_cidr_arbitrary_inputs() {
  let mut rng = Rng::new(0xc1d4_2026_0924);
  // 任意字符串不得 panic
  for _ in 0..10_000 {
    let ip: String = (0..rng.below(40))
      .map(|_| (rng.below(95) + 32) as u8 as char)
      .collect();
    let cidr: String = (0..rng.below(40))
      .map(|_| (rng.below(95) + 32) as u8 as char)
      .collect();
    let code = crate::api::fuzz_helpers::cidr_contains(&ip, &cidr);
    assert!((-2..=1).contains(&code));
  }
  // 确定性性质
  let cidr = |ip: &str, c: &str| crate::api::fuzz_helpers::cidr_contains(ip, c);
  assert_eq!(cidr("8.8.8.8", "0.0.0.0/0"), 1);
  assert_eq!(cidr("2001:db8::1", "::/0"), 1);
  assert_eq!(cidr("192.168.1.5", "192.168.1.5/32"), 1);
  assert_eq!(cidr("192.168.1.6", "192.168.1.5/32"), 0);
  assert_eq!(cidr("192.168.1.5", "192.168.0.0/16"), 1);
  assert_eq!(cidr("10.0.0.1", "192.168.0.0/16"), 0);
  assert_eq!(cidr("::1", "127.0.0.0/8"), 0, "v6/v4 族不匹配");
  assert_eq!(cidr("1.2.3.4", "1.2.3.4/99"), -1, "/99 非法前缀");
  assert_eq!(cidr("1.2.3.4", "not-a-cidr"), -1);
  assert_eq!(cidr("999.1.1.1", "0.0.0.0/0"), -2, "非法 IP");
}

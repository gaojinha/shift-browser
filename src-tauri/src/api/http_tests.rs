mod http_req_tests {
    use super::super::*;
    use std::net::IpAddr;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, ReadBuf};

    /// 每次 poll_read 最多吐出 max_chunk 字节，模拟 TCP 跨包/拆包
    struct SlowReader {
        data: Vec<u8>,
        pos: usize,
        max_chunk: usize,
    }

    impl AsyncRead for SlowReader {
        fn poll_read(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let this = self.get_mut();
            let remain = this.data.len() - this.pos;
            if remain == 0 {
                return Poll::Ready(Ok(()));
            }
            let n = remain.min(this.max_chunk).min(buf.remaining());
            buf.put_slice(&this.data[this.pos..this.pos + n]);
            this.pos += n;
            Poll::Ready(Ok(()))
        }
    }

    fn post_request(body: &str) -> Vec<u8> {
        format!(
            "POST /api HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .into_bytes()
    }

    #[tokio::test]
    async fn small_body_single_read() {
        let mut r = SlowReader {
            data: post_request("hello"),
            pos: 0,
            max_chunk: 4096,
        };
        match read_http_request(&mut r).await {
            HttpReq::Complete(head, body) => {
                assert!(head.starts_with("POST /api HTTP/1.1"));
                assert_eq!(body, "hello");
            }
            other => panic!("期望 Complete，实际 {other:?}"),
        }
    }

    /// 回归：旧实现只读一次 64KB，200KB body 的长对话必然被截断
    #[tokio::test]
    async fn large_body_read_full_across_packets() {
        let body = "a".repeat(200 * 1024);
        let mut r = SlowReader {
            data: post_request(&body),
            pos: 0,
            max_chunk: 4096,
        };
        match read_http_request(&mut r).await {
            HttpReq::Complete(_, got) => assert_eq!(got.len(), 200 * 1024),
            other => panic!("期望 Complete，实际 {other:?}"),
        }
    }

    /// 头部本身跨多个 TCP 包（\r\n\r\n 被拆开）也能正确定位
    #[tokio::test]
    async fn headers_split_across_packets() {
        let mut r = SlowReader {
            data: post_request("xy"),
            pos: 0,
            max_chunk: 13,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Complete(_, b) if b == "xy"));
    }

    #[tokio::test]
    async fn oversized_body_rejected() {
        let body = "a".repeat(HTTP_MAX_BODY + 1);
        let mut r = SlowReader {
            data: post_request(&body),
            pos: 0,
            max_chunk: 16 * 1024,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::TooLarge));
    }

    #[tokio::test]
    async fn body_without_content_length_uses_tail_bytes() {
        let raw = b"POST /api HTTP/1.1\r\nHost: x\r\n\r\nabc".to_vec();
        let mut r = SlowReader {
            data: raw,
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Complete(_, b) if b == "abc"));
    }

    #[tokio::test]
    async fn closed_connection_is_bad() {
        let mut r = SlowReader {
            data: vec![],
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Bad));
    }

    #[tokio::test]
    async fn chunked_unsupported() {
        let raw =
            b"POST /api HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n"
                .to_vec();
        let mut r = SlowReader {
            data: raw,
            pos: 0,
            max_chunk: 4096,
        };
        assert!(matches!(read_http_request(&mut r).await, HttpReq::Bad));
    }

    #[test]
    fn content_length_parsing() {
        let head = "POST /api HTTP/1.1\r\ncontent-length: 42\r\n";
        assert_eq!(content_length_of(head), Some(42));
        assert_eq!(content_length_of("POST / HTTP/1.1\r\nHost: x\r\n"), None);
        assert_eq!(find_header_end(b"abc\r\n\r\nbody"), Some(7),);
        assert_eq!(find_header_end(b"abc\r\n\r"), None);
    }

    #[test]
    fn ip_allowlist_empty_allows_all() {
        assert!(crate::api::auth::ip_allowed("8.8.8.8", ""));
        assert!(crate::api::auth::ip_allowed("8.8.8.8", "   "));
    }

    #[test]
    fn ip_allowlist_bare_prefix_no_longer_matches_neighbor_segment() {
        // 旧 bug：裸 starts_with("192.168.1") 误放行 192.168.10.x
        assert!(!crate::api::auth::ip_allowed("192.168.10.5", "192.168.1"));
        // 裸前缀现在是非法条目（不是精确 IP/CIDR），同样不放行
        assert!(!crate::api::auth::ip_allowed("192.168.1.5", "192.168.1"));
    }

    #[test]
    fn ip_allowlist_dotted_segment_prefix_backcompat() {
        assert!(crate::api::auth::ip_allowed("192.168.1.5", "192.168.1."));
        assert!(crate::api::auth::ip_allowed("192.168.1.200", "192.168.1."));
        assert!(!crate::api::auth::ip_allowed("192.168.10.5", "192.168.1."));
        assert!(!crate::api::auth::ip_allowed("192.169.1.5", "192.168.1."));
    }

    #[test]
    fn ip_allowlist_exact_and_cidr() {
        assert!(crate::api::auth::ip_allowed("192.168.1.5", "192.168.1.5"));
        assert!(!crate::api::auth::ip_allowed("192.168.1.6", "192.168.1.5"));
        assert!(crate::api::auth::ip_allowed("192.168.1.1", "192.168.1.0/24"));
        assert!(crate::api::auth::ip_allowed("192.168.1.254", "192.168.1.0/24"));
        assert!(!crate::api::auth::ip_allowed("192.168.2.1", "192.168.1.0/24"));
        assert!(crate::api::auth::ip_allowed("10.1.2.3", "10.0.0.0/8"));
        assert!(!crate::api::auth::ip_allowed("11.1.2.3", "10.0.0.0/8"));
        // 多值 + 空白容忍
        assert!(crate::api::auth::ip_allowed("1.2.3.4", "  10.0.0.0/8 , 1.2.3.4 "));
        assert!(!crate::api::auth::ip_allowed("5.6.7.8", "10.0.0.0/8,1.2.3.4"));
        // 非法 CIDR（前缀越界）与垃圾条目不影响合法条目
        assert!(crate::api::auth::ip_allowed("10.0.0.1", "garbage,10.0.0.0/8,99.0.0.0/33"));
    }

    #[test]
    fn ip_allowlist_ipv6_and_mapped_v4() {
        assert!(crate::api::auth::ip_allowed("::1", "::1"));
        assert!(crate::api::auth::ip_allowed("fe80::1234", "fe80::/10"));
        assert!(!crate::api::auth::ip_allowed("2001::1", "fe80::/10"));
        // IPv4-mapped IPv6 归一化后按 v4 规则判定（防 v6 形态绕过）
        assert!(crate::api::auth::ip_allowed("::ffff:192.168.1.5", "192.168.1.0/24"));
        assert!(crate::api::auth::ip_allowed("::ffff:192.168.1.5", "192.168.1."));
        assert!(!crate::api::auth::ip_allowed("::ffff:192.168.2.5", "192.168.1.0/24"));
        // 跨族不匹配
        assert!(!crate::api::auth::ip_allowed("::1", "127.0.0.0/8"));
    }

    #[test]
    fn backoff_exponential_caps_at_300() {
        assert_eq!(crate::api::auth::backoff_secs(1), 1);
        assert_eq!(crate::api::auth::backoff_secs(2), 2);
        assert_eq!(crate::api::auth::backoff_secs(3), 4);
        assert_eq!(crate::api::auth::backoff_secs(9), 256);
        assert_eq!(crate::api::auth::backoff_secs(10), 300);
        assert_eq!(crate::api::auth::backoff_secs(100), 300);
    }

    #[test]
    fn login_guard_loopback_never_locked() {
        let g = crate::api::auth::LoginGuard::default();
        let t0 = Instant::now();
        let v4: IpAddr = "127.0.0.1".parse().unwrap();
        let v6: IpAddr = "::1".parse().unwrap();
        assert_eq!(g.record_failure(&v4, t0), 0);
        assert!(g.check(&v4, t0).is_none());
        g.record_success(&v6); // 不 panic、不产生状态
    }

    #[test]
    fn login_guard_locks_then_recovers_on_success() {
        let g = crate::api::auth::LoginGuard::default();
        let ip: IpAddr = "192.168.5.5".parse().unwrap();
        let t0 = Instant::now();
        assert_eq!(g.record_failure(&ip, t0), 1);
        // 封禁窗口内 429
        assert!(g.check(&ip, t0).is_some());
        assert!(g.check(&ip, t0 + Duration::from_millis(500)).is_some());
        // 1s 后窗口过了，放行（fails 保留）
        assert!(g.check(&ip, t0 + Duration::from_secs(2)).is_none());
        // 再失败：fails=2 → 封 2s
        assert_eq!(g.record_failure(&ip, t0 + Duration::from_secs(2)), 2);
        assert!(g.check(&ip, t0 + Duration::from_secs(3)).is_some());
        // 成功认证清零：立刻放行，且下一次失败从 1s 重新计
        g.record_success(&ip);
        assert!(g.check(&ip, t0 + Duration::from_secs(3)).is_none());
        assert_eq!(g.record_failure(&ip, t0 + Duration::from_secs(3)), 1);
    }

    #[test]
    fn login_guard_peers_independent() {
        let g = crate::api::auth::LoginGuard::default();
        let t0 = Instant::now();
        g.record_failure(&"192.168.0.1".parse().unwrap(), t0);
        assert!(g.check(&"192.168.0.2".parse().unwrap(), t0).is_none());
    }

    #[test]
    fn dashboard_ticket_single_use() {
        let tk = crate::api::auth::DashboardTickets::new();
        let t = tk.issue();
        assert_eq!(t.len(), 32, "门票为 32hex");
        assert!(tk.consume(&t));
        assert!(!tk.consume(&t), "用后即焚");
        assert!(!tk.consume("deadbeefdeadbeefdeadbeefdeadbeef"));
        // 错误票不影响另一张合法票
        let t2 = tk.issue();
        assert!(tk.consume(&t2));
    }

    #[test]
    fn kv_value_and_url_decode() {
        assert_eq!(
            kv_value("a=1&token=ab%20c&d", "token").as_deref(),
            Some("ab c")
        );
        assert_eq!(kv_value("token=a+b", "token").as_deref(), Some("a b"));
        assert_eq!(kv_value("x=1", "token"), None);
        assert_eq!(kv_value("", "token"), None);
        assert_eq!(url_decode("100%25"), "100%");
        assert_eq!(url_decode("%zz"), "%zz");
    }

    #[test]
    fn bearer_header_check() {
        assert!(bearer_token_ok(
            "POST /api HTTP/1.1\r\nAuthorization: Bearer secret123\r\n",
            "secret123"
        ));
        assert!(!bearer_token_ok(
            "POST /api HTTP/1.1\r\nAuthorization: Bearer wrong\r\n",
            "secret123"
        ));
        assert!(bearer_token_ok(
            "POST /api HTTP/1.1\r\nauthorization: bearer secret123\r\n",
            "secret123"
        ));
        assert!(!bearer_token_ok("POST /api HTTP/1.1\r\nX-A: 1\r\n", "secret123"));
    }

    // ── 确定性模糊测试（HANDOFF P2-5）──────────────────────────────────────
    // 固定种子 xorshift，不引第三方依赖，CI 每次跑同一序列；
    // 覆盖引导深挖由 fuzz/ 下 cargo-fuzz 目标承担（每周 CI），二者互补。
    // 核心安全性质：任意输入 + 任意 TCP 拆包，解析器**不得 panic / 死循环**。
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
}

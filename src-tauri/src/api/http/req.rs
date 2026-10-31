//! HTTP 请求读取与头部解析（R1 拆分：从 api/http.rs 抽出，逻辑零改动）
//!
//! 手写 HTTP/1.x 请求读取：头部循环读到 `\r\n\r\n`，body 按 Content-Length 读满。
//! 安全边界：头部 64KB / 请求体 8MB 上限，拒绝 chunked 编码。

/// HTTP 请求头上限（64KB，远超正常请求头；超出按畸形请求断开）
const HTTP_MAX_HEADER: usize = 64 * 1024;
/// HTTP 请求体上限（8MB；模型网关转发长对话 chat/completions 必需）
pub(crate) const HTTP_MAX_BODY: usize = 8 * 1024 * 1024;

/// HTTP 请求读取结果
#[derive(Debug)]
pub(crate) enum HttpReq {
    /// (请求头文本[含结尾 \r\n\r\n；读侧均按行解析，不依赖此尾巴], body 文本)
    Complete(String, String),
    /// Content-Length 超过 [`HTTP_MAX_BODY`]
    TooLarge,
    /// 连接提前关闭 / 请求畸形 / 头部超限 / chunked 不支持
    Bad,
}

/// 从任意 AsyncRead 完整读取一个 HTTP/1.x 请求：
/// - 头部循环读到 `\r\n\r\n`（头部可能跨 TCP 包到达，旧实现只读一次必然漏读）；
/// - 按 Content-Length 把 body 读满（body 也可能跨包，或与头部在同一次 read 中到达）；
/// - 无 Content-Length 时 body 取已随头部读到的剩余字节（兼容简单客户端）。
pub(crate) async fn read_http_request<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> HttpReq {
    use tokio::io::AsyncReadExt;
    let mut buf: Vec<u8> = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        let n = match r.read(&mut chunk).await {
            Ok(0) => return HttpReq::Bad,
            Ok(n) => n,
            Err(_) => return HttpReq::Bad,
        };
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find_header_end(&buf) {
            break i;
        }
        if buf.len() > HTTP_MAX_HEADER {
            return HttpReq::Bad;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    // Transfer-Encoding: chunked 不支持（本地 API 面向程序客户端，均带 Content-Length）
    if is_chunked(&head) {
        return HttpReq::Bad;
    }
    let target_len = match content_length_of(&head) {
        Some(len) => {
            if len > HTTP_MAX_BODY {
                return HttpReq::TooLarge;
            }
            header_end.saturating_add(len)
        }
        None => buf.len(),
    };
    while buf.len() < target_len {
        let n = match r.read(&mut chunk).await {
            Ok(0) => return HttpReq::Bad,
            Ok(n) => n,
            Err(_) => return HttpReq::Bad,
        };
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[header_end..target_len]).to_string();
    HttpReq::Complete(head, body)
}

/// 在已读字节中定位 HTTP 头部结束位置（`\r\n\r\n` 之后的偏移）
pub(crate) fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4)
}

/// 请求头是否声明 `Transfer-Encoding: chunked`
pub(crate) fn is_chunked(head: &str) -> bool {
    head.lines().skip(1).any(|l| {
        l.split_once(':')
            .map(|(k, v)| {
                k.trim().eq_ignore_ascii_case("transfer-encoding")
                    && v.split(',')
                        .any(|te| te.trim().eq_ignore_ascii_case("chunked"))
            })
            .unwrap_or(false)
    })
}

/// 从请求头解析 Content-Length
pub(crate) fn content_length_of(head: &str) -> Option<usize> {
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if !k.trim().eq_ignore_ascii_case("content-length") {
            return None;
        }
        v.trim().parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    fn reader(bytes: &[u8]) -> BufReader<std::io::Cursor<Vec<u8>>> {
        BufReader::new(std::io::Cursor::new(bytes.to_vec()))
    }

    #[test]
    fn find_header_end_locates_terminator() {
        // 精确定位（返回终止符之后的偏移；头部含 \r\n\r\n 的 27 字节例）
        assert_eq!(
            find_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some(27)
        );
        // 终止符在最前 / 取首次出现
        assert_eq!(find_header_end(b"\r\n\r\n"), Some(4));
        assert_eq!(find_header_end(b"\r\n\r\nX\r\n\r\n"), Some(4));
        // 不完整 / 过短 / 空 / 纯 LF 均不算（HTTP 终止符是 CRLF）
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\nHost: x\r\n"), None);
        assert_eq!(find_header_end(b"\r\n"), None);
        assert_eq!(find_header_end(b""), None);
        assert_eq!(find_header_end(b"GET / HTTP/1.1\nHost: x\n\n"), None);
    }

    #[test]
    fn is_chunked_matches_key_and_value_case_insensitively() {
        // 请求行被跳过（含 "chunked" 字样的路径不算，无 TE 头不算）
        assert!(is_chunked(
            "GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nHost: x"
        ));
        assert!(!is_chunked("GET /chunked HTTP/1.1\r\nHost: x"));
        // 大小写不敏感 + 逗号列表（任一分量为 chunked 即命中）
        assert!(is_chunked("POST / HTTP/1.1\r\ntransfer-encoding: CHUNKeD"));
        assert!(is_chunked(
            "POST / HTTP/1.1\r\nTransfer-Encoding: gzip, chunked"
        ));
        assert!(is_chunked(
            "POST / HTTP/1.1\r\nTransfer-Encoding: chunked , gzip"
        ));
        // 非 chunked / 无 TE / 前缀相似键均不算
        assert!(!is_chunked("POST / HTTP/1.1\r\nTransfer-Encoding: gzip"));
        assert!(!is_chunked("POST / HTTP/1.1\r\nHost: x"));
        assert!(!is_chunked(
            "POST / HTTP/1.1\r\nX-Transfer-Encoding: chunked"
        ));
    }

    #[test]
    fn content_length_of_first_wins_and_garbage_tolerated() {
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: 42"),
            Some(42)
        );
        // 大小写不敏感 / 行尾 \r / 零值
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\ncontent-length: 7\r"),
            Some(7)
        );
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: 0"),
            Some(0)
        );
        // 重复头：首个生效
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: 10\r\nContent-Length: 20"),
            Some(10)
        );
        // 缺失 / 非法数字 / 负数 / 前缀相似键 → None
        assert_eq!(content_length_of("POST / HTTP/1.1\r\nHost: x"), None);
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: abc"),
            None
        );
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: -5"),
            None
        );
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nX-Content-Length: 99"),
            None
        );
        // 超大值原样返回（上限判定在调用方，本函数不做 clamp）
        assert_eq!(
            content_length_of("POST / HTTP/1.1\r\nContent-Length: 8388608"),
            Some(8 * 1024 * 1024)
        );
    }

    #[tokio::test]
    async fn read_http_request_splits_head_and_body() {
        // 基本分帧：body 按 Content-Length 读满。注意约定：head 含结尾
        // \r\n\r\n（与 HttpReq::Complete 文档一致；读侧按行解析，不依赖此尾巴）
        let req = b"GET /a HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\nabc".to_vec();
        match read_http_request(&mut reader(&req)).await {
            HttpReq::Complete(head, body) => {
                assert_eq!(
                    head,
                    "GET /a HTTP/1.1\r\nHost: x\r\nContent-Length: 3\r\n\r\n"
                );
                assert_eq!(body, "abc");
            }
            other => panic!("期望 Complete: {other:?}"),
        }
        // body 内含 \r\n\r\n 不干扰分帧（按字节偏移切分，不靠二次定位终止符）
        // 注意 body `x\r\n\r\ny` 是 6 字节，CL 必须精确（多 1 字节会多读一次 → EOF → Bad）
        let req = b"POST /b HTTP/1.1\r\nContent-Length: 6\r\n\r\nx\r\n\r\ny".to_vec();
        match read_http_request(&mut reader(&req)).await {
            HttpReq::Complete(_, body) => assert_eq!(body, "x\r\n\r\ny"),
            other => panic!("期望 Complete: {other:?}"),
        }
        // CL=0 → 空 body；无 CL → body 取已随头部读到的剩余字节
        let req = b"HEAD /c HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_vec();
        match read_http_request(&mut reader(&req)).await {
            HttpReq::Complete(_, body) => assert_eq!(body, ""),
            other => panic!("期望 Complete: {other:?}"),
        }
        let req = b"GET /d HTTP/1.1\r\n\r\nleftover".to_vec();
        match read_http_request(&mut reader(&req)).await {
            HttpReq::Complete(_, body) => assert_eq!(body, "leftover"),
            other => panic!("期望 Complete: {other:?}"),
        }
        // >4KB 迫使 read 循环多次（头部循环 + body 循环各走多轮）
        let body = "B".repeat(9000);
        let req = format!("POST /e HTTP/1.1\r\nContent-Length: 9000\r\n\r\n{body}").into_bytes();
        match read_http_request(&mut reader(&req)).await {
            HttpReq::Complete(head, b2) => {
                assert_eq!(head, "POST /e HTTP/1.1\r\nContent-Length: 9000\r\n\r\n");
                assert_eq!(b2.len(), 9000);
            }
            other => panic!("期望 Complete: {other:?}"),
        }
    }

    #[tokio::test]
    async fn read_http_request_rejects_bad_and_too_large() {
        // 空连接 → Bad
        assert!(matches!(
            read_http_request(&mut reader(b"")).await,
            HttpReq::Bad
        ));
        // chunked 不支持 → Bad
        let req = b"POST /f HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec();
        assert!(matches!(
            read_http_request(&mut reader(&req)).await,
            HttpReq::Bad
        ));
        // body 未达 CL 连接先断 → Bad
        let req = b"GET /g HTTP/1.1\r\nContent-Length: 10\r\n\r\nabc".to_vec();
        assert!(matches!(
            read_http_request(&mut reader(&req)).await,
            HttpReq::Bad
        ));
        // CL 超 8MB → TooLarge（头部阶段即判定，不读 body）
        let req = b"POST /h HTTP/1.1\r\nContent-Length: 8388609\r\n\r\n".to_vec();
        assert!(matches!(
            read_http_request(&mut reader(&req)).await,
            HttpReq::TooLarge
        ));
        // 头部超 64KB 且无终止符 → Bad
        let long = "a".repeat(65 * 1024);
        let req = format!("GET /i HTTP/1.1\r\nX-Large: {long}").into_bytes();
        assert!(matches!(
            read_http_request(&mut reader(&req)).await,
            HttpReq::Bad
        ));
    }
}

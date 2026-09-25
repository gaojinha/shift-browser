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
    /// (请求头文本[不含结尾 \r\n\r\n], body 文本)
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

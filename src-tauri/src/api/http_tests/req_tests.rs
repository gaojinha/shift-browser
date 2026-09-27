//! HTTP 请求解析测试：跨包读取、Content-Length、chunked 拒绝（R1 2026-09-25 从 http_tests.rs 拆分）

use super::super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
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
        b"POST /api HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n".to_vec();
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

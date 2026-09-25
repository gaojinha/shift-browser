//! HTTP 响应写入与小工具（R1 拆分：从 api/http.rs 抽出，逻辑零改动）
//!
//! write_http 写完整 HTTP/1.1 响应（连接固定 close）；
//! bearer_token_ok / kv_value / url_decode 为鉴权与表单解析小工具。

use tokio::io::AsyncWriteExt;

use crate::api::auth;

/// 写一个完整 HTTP/1.1 响应（连接固定 close；`extra_headers` 每行需自带 `\r\n`）
pub(crate) async fn write_http(
    stream: &mut tokio::net::TcpStream,
    status: u16,
    reason: &str,
    content_type: &str,
    extra_headers: &str,
    body: &[u8],
) {
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra_headers}Connection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(body).await;
}

/// 校验 Authorization 头是否为匹配的 Bearer token（常量时间比较）
pub(crate) fn bearer_token_ok(head: &str, token: &str) -> bool {
    head.lines().any(|l| {
        if !l.to_ascii_lowercase().starts_with("authorization:") {
            return false;
        }
        l.split_once(':')
            .map(|(_, v)| {
                let v = v.trim();
                let v = v
                    .strip_prefix("Bearer ")
                    .or_else(|| v.strip_prefix("bearer "))
                    .unwrap_or(v)
                    .trim();
                auth::ct_eq(v, token)
            })
            .unwrap_or(false)
    })
}

/// 从 `a=b&c=d` 形式串（query 或 form body）取值并 percent-decode
pub(crate) fn kv_value(encoded: &str, key: &str) -> Option<String> {
    encoded.split('&').find_map(|kv| {
        let mut it = kv.splitn(2, '=');
        let k = it.next()?;
        (k == key).then(|| url_decode(it.next().unwrap_or("")))
    })
}

/// 最小 percent-decode（`%XX` 与 `+`；非法转义原样保留）
pub(crate) fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let hex = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    };
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                    out.push(h * 16 + l);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

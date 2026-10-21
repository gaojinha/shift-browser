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
    if let Err(e) = stream.write_all(head.as_bytes()).await {
        log::debug!("[http] 写响应头失败（对端可能已断连）: {}", e);
    }
    if let Err(e) = stream.write_all(body).await {
        log::debug!("[http] 写响应体失败（对端可能已断连）: {}", e);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_token_matches_case_insensitively() {
        // 标准形态 / 小写头 + 小写 Bearer / 多行时第二行命中
        assert!(bearer_token_ok("Authorization: Bearer s3cret", "s3cret"));
        assert!(bearer_token_ok("authorization: bearer s3cret", "s3cret"));
        assert!(bearer_token_ok(
            "Host: x\r\nAuthorization: Bearer s3cret",
            "s3cret"
        ));
        // 无 Bearer 前缀的裸值也接受（现行口径：strip 失败用原值比较）
        assert!(bearer_token_ok("Authorization: s3cret", "s3cret"));
        // 否定路径：错 token / 无该头 / 空头
        assert!(!bearer_token_ok("Authorization: Bearer wrong", "s3cret"));
        assert!(!bearer_token_ok("Host: x", "s3cret"));
        assert!(!bearer_token_ok("", "s3cret"));
    }

    #[test]
    fn kv_value_extracts_with_decode() {
        assert_eq!(kv_value("a=1&b=2", "b"), Some("2".into()));
        // percent-decode 生效
        assert_eq!(kv_value("q=a%20b&x=1", "q"), Some("a b".into()));
        // '+' 转空格
        assert_eq!(kv_value("q=a+b", "q"), Some("a b".into()));
        // 值里带 = 不截断（splitn(2)）
        assert_eq!(kv_value("tok=a=b=c", "tok"), Some("a=b=c".into()));
        // 无值键 → 空串；缺键 → None
        assert_eq!(kv_value("flag=", "flag"), Some("".into()));
        assert_eq!(kv_value("a=1", "zz"), None);
    }

    #[test]
    fn url_decode_rules_and_garbage_tolerance() {
        assert_eq!(url_decode("%41%42"), "AB");
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("plain"), "plain");
        // 非法/截断转义原样保留，不 panic
        assert_eq!(url_decode("%ZZ"), "%ZZ");
        assert_eq!(url_decode("%2"), "%2");
        assert_eq!(url_decode("%"), "%");
        // 非法保留时后续字符继续正常解码
        assert_eq!(url_decode("%ZZ%41"), "%ZZA");
    }
}

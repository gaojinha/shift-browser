use super::*;

// ---- host_of：origin_url → host（对齐 F-68 host_of 口径） ----

#[test]
fn host_of_extracts_host_and_strips_userinfo() {
    assert_eq!(
        host_of("https://example.com/login").as_deref(),
        Some("example.com")
    );
    assert_eq!(
        host_of("http://sub.example.com:8443/x?y=1").as_deref(),
        Some("sub.example.com:8443")
    );
    assert_eq!(
        host_of("https://u:p@host.example.com/").as_deref(),
        Some("host.example.com"),
        "userinfo 剥离"
    );
    // 非 http(s) origin 防御性拒收（logins 里不该有，防垃圾 domain）
    assert_eq!(host_of("chrome://settings"), None);
    assert_eq!(host_of("ftp://x.com"), None);
    assert_eq!(host_of(""), None);
}

// ---- decrypt_login_blob：透传 F-90 原语（黄金向量语义在此再锁一层） ----

const GOLDEN_V10_CT_HEX: &str =
    "8a2f0711b2b584b62bd4f134bd2d425636de3c42f4b17b617ba3fa739ad827a7";
const GOLDEN_V11_SECRET: &str = "q7b7aG9hbm9yaXNlaGVpZGVp";
const GOLDEN_V11_CT_HEX: &str = "035436fe29e7cbbcf01298db6336bf4f";

fn hex(s: &str) -> Vec<u8> {
    hex::decode(s).unwrap()
}

#[test]
fn v10_blob_decrypts_without_keyring() {
    let blob = [b"v10".as_slice(), &hex(GOLDEN_V10_CT_HEX)].concat();
    assert_eq!(
        decrypt_login_blob(&blob, None).unwrap(),
        "https://example.com/"
    );
}

#[test]
fn v11_blob_needs_keyring_key() {
    let blob = [b"v11".as_slice(), &hex(GOLDEN_V11_CT_HEX)].concat();
    assert_eq!(
        decrypt_login_blob(&blob, None),
        Err(super::super::DecryptError::NoKeyringKey),
        "v11 无 keyring key → 降级 Err（不猜）"
    );
    assert_eq!(
        decrypt_login_blob(&blob, Some(GOLDEN_V11_SECRET.as_bytes())).unwrap(),
        "p@ssw0rd-é中"
    );
}

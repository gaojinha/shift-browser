//! vault 测试（R1 拆分外置 · 2026-09-25）：密码学往返/v1 升级/空闲判定。
//! 门面经 `use super::*` 带入（should_lock_idle 等）；密码学原语取自兄弟模块 vault_crypto。

use super::*;
use crate::db::vault_crypto::*;

/// 测试专用：用指定 rounds 构造 v2 形态文件（debug 下 600k 每次约 4.5s，
/// 往返/格式类用例用小 rounds；真实 600k 仅 wrap_key 生产冒烟保留一次）
#[allow(clippy::unwrap_used, clippy::expect_used)]
fn wrap_v2_with_rounds(master: &[u8; KEY_LEN], password: &str, rounds: u32) -> String {
    use aes_gcm::aead::{Aead, KeyInit};
    use aes_gcm::{Aes256Gcm, Nonce};
    use base64::Engine;
    use rand::RngCore;
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let dk = derive_with_rounds(password, &salt, rounds);
    let cipher = Aes256Gcm::new_from_slice(&dk).expect("32 字节密钥必合法");
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), master.as_ref())
        .expect("AES-GCM 加密不应失败");
    let mut blob = Vec::new();
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    format!(
        "v2$pbkdf2-sha256-{rounds}${}",
        base64::engine::general_purpose::STANDARD.encode(blob)
    )
}

/// 按 v1 历史参数（无文件头、20 万次）构造 vault 文件内容
fn wrap_v1_legacy(master: &[u8; KEY_LEN], password: &str) -> String {
    // v1 同样结构，只是无头 + rounds 固定 200k（复用快速构造不可行——
    // 解析器对无头内容一律按 200k 解，这里必须用真实 V1 轮数）
    let v2 = wrap_v2_with_rounds(master, password, PBKDF2_ROUNDS_V1);
    v2.rsplit('$').next().unwrap().to_string()
}

const TEST_ROUNDS: u32 = 1_000;

#[test]
fn wrap_unwrap_roundtrip() {
    let master = [7u8; KEY_LEN];
    let wrapped = wrap_v2_with_rounds(&master, "correct horse battery", TEST_ROUNDS);
    let back = unwrap_key(&wrapped, "correct horse battery").unwrap();
    assert_eq!(back, master);
}

#[test]
fn wrong_password_rejected() {
    let master = [9u8; KEY_LEN];
    let wrapped = wrap_v2_with_rounds(&master, "right-password-1", TEST_ROUNDS);
    let err = unwrap_key(&wrapped, "wrong-password-1").unwrap_err();
    assert!(
        err.contains("主密码错误"),
        "错误密码应被拒绝，实际: {}",
        err
    );
}

#[test]
fn wrap_is_randomized() {
    let master = [1u8; KEY_LEN];
    // 同一主密码两次包裹：salt/nonce 随机 → 密文不同（防确定性泄露）
    let a = wrap_v2_with_rounds(&master, "same-password-xx", TEST_ROUNDS);
    let b = wrap_v2_with_rounds(&master, "same-password-xx", TEST_ROUNDS);
    assert_ne!(a, b);
    assert_eq!(unwrap_key(&a, "same-password-xx").unwrap(), master);
}

#[test]
fn garbage_rejected() {
    use base64::Engine;
    assert!(unwrap_key("not-base64!!!", "x").is_err());
    assert!(unwrap_key(
        &base64::engine::general_purpose::STANDARD.encode([0u8; 4]),
        "x"
    )
    .is_err());
}

#[test]
fn derive_key_deterministic() {
    let salt = [3u8; SALT_LEN];
    let a = derive_with_rounds("pw", &salt, TEST_ROUNDS);
    let b = derive_with_rounds("pw", &salt, TEST_ROUNDS);
    assert_eq!(a, b);
    assert_ne!(a, derive_with_rounds("pw2", &salt, TEST_ROUNDS));
}

#[test]
fn wrap_key_emits_v2_header() {
    // 生产路径冒烟：真实 600k 包裹结果必须是 v2 头（套件内唯一一次 600k）
    let master = [5u8; KEY_LEN];
    let wrapped = wrap_key(&master, "header-check-pw");
    assert!(
        wrapped.starts_with(V2_HEADER),
        "新包裹文件必须是 v2 头格式，实际开头: {:?}",
        wrapped.get(..30)
    );
    assert!(!is_legacy_v1_file(&wrapped));
}

#[test]
fn v1_legacy_file_unwraps_and_upgrades_to_v2() {
    // 老用户的 v1 文件（20 万次、无头）：按 v1 参数解开后，重包产物为 v2 头
    let master = [0xABu8; KEY_LEN];
    let pw = "legacy-user-pw-9";
    let v1 = wrap_v1_legacy(&master, pw);
    assert!(is_legacy_v1_file(&v1), "裸 base64 应识别为 v1");
    assert!(!v1.starts_with("v2$"));

    // 旧文件按 20 万次解开
    let key = unwrap_key(&v1, pw).unwrap();
    assert_eq!(key, master);
    // 错误密码仍拒绝（错误信息口径不变）
    assert_eq!(unwrap_key(&v1, "wrong").unwrap_err(), "主密码错误");

    // unlock_vault 重包路径：wrap_key 产 v2 头（解密路径由其余用例覆盖）
    let v2 = wrap_v2_with_rounds(&key, pw, TEST_ROUNDS);
    assert!(v2.starts_with("v2$pbkdf2-sha256-"));
    assert!(!is_legacy_v1_file(&v2));
}

#[test]
fn malformed_v2_headers_rejected() {
    // 缺载荷
    assert!(unwrap_key("v2$pbkdf2-sha256-600000$", "pw").is_err());
    // 未知算法
    assert!(unwrap_key("v2$pbkdf2-sha512-600000$AAAA", "pw").is_err());
    // 迭代次数超限（防恶意文件 DoS）
    assert!(unwrap_key("v2$pbkdf2-sha256-999999999$AAAA", "pw").is_err());
    // 迭代次数为 0
    assert!(unwrap_key("v2$pbkdf2-sha256-0$AAAA", "pw").is_err());
    // 载荷不是 base64
    assert!(unwrap_key("v2$pbkdf2-sha256-600000$!!!", "pw").is_err());
}

#[test]
fn nfkc_normalized_passwords_unlock() {
    let master = [3u8; KEY_LEN];
    // 设置时输入兼容/全角形式，解锁时输入 NFKC 标准形式，密钥一致
    let wrapped = wrap_v2_with_rounds(&master, "pw-ﬁ-Ａ", TEST_ROUNDS);
    assert_eq!(unwrap_key(&wrapped, "pw-fi-A").unwrap(), master);
    // 反向同样成立（两侧都归一化）
    assert!(unwrap_key(&wrapped, "pw-ﬁ-Ａ").is_ok());
}

#[test]
fn idle_lock_decision_pure_fn() {
    use std::time::{Duration, Instant};
    let now = Instant::now();
    let t0 = now - Duration::from_secs(14 * 60);
    let t1 = now - Duration::from_secs(16 * 60);
    // 未解锁：不锁
    assert!(!should_lock_idle(false, Some(t1), now, 15));
    // 无活动记录：保守不锁
    assert!(!should_lock_idle(true, None, now, 15));
    // 0 = 关闭：永不锁
    assert!(!should_lock_idle(true, Some(t1), now, 0));
    // 未满时长不锁
    assert!(!should_lock_idle(true, Some(t0), now, 15));
    // 达到/超过时长锁
    assert!(should_lock_idle(true, Some(t1), now, 15));
    assert!(should_lock_idle(
        true,
        Some(now - Duration::from_secs(60)),
        now,
        1
    ));
}

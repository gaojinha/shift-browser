//! vault.key 加解密与文件格式（R1 拆分 · 2026-09-25）
//!
//! 与 [`super::vault`] 的分工：本文件只管"主密码 ↔ 包裹密钥 ↔ vault.key 文件"
//! 的纯密码学/文件 IO（PBKDF2-HMAC-SHA256、AES-256-GCM、v1/v2 格式、600 权限落盘）；
//! 内存解锁会话、空闲锁定守护与 Tauri 命令留在 vault.rs 门面。
//!
//! 存储格式（vault.key）：
//! - v2（当前）：`v2$pbkdf2-sha256-600000$<base64(salt(16) || nonce(12) || 密文)>`
//! - v1（历史，无文件头）：`<base64(salt(16) || nonce(12) || 密文)>`，PBKDF2 20 万次。

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

use crate::db::{DbError, DbResult};

/// v2 PBKDF2 迭代次数（OWASP 2023 对 PBKDF2-HMAC-SHA256 建议下限 600k）
pub(super) const PBKDF2_ROUNDS_V2: u32 = 600_000;
/// v1（无文件头）历史迭代次数，仅用于解锁旧文件
pub(super) const PBKDF2_ROUNDS_V1: u32 = 200_000;
/// v2 文件头前缀（完整行：v2$pbkdf2-sha256-600000$<base64…>）。
/// `$` 不在标准 base64 字符表中，有无前缀无歧义。
pub(super) const V2_HEADER: &str = "v2$pbkdf2-sha256-600000$";
/// 文件头中自报迭代次数的合法上限（防恶意文件填天文数字拖死解锁）
const MAX_HEADER_ROUNDS: u32 = 10_000_000;
pub(super) const SALT_LEN: usize = 16;
pub(super) const NONCE_LEN: usize = 12;
pub(super) const KEY_LEN: usize = 32;

/// 主密码 NFKC 归一化：设置/解锁/移除所有入口经派生前都过这里，
/// 全角字符（ＡＢＣ）与兼容组合（ﬁ→fi）在两侧得到同一密钥。
pub(super) fn normalize_password(password: &str) -> String {
    password.nfkc().collect()
}

/// PBKDF2-HMAC-SHA256 派生包裹密钥（指定迭代次数；NFKC 归一化在派生内统一完成）
pub(super) fn derive_with_rounds(password: &str, salt: &[u8], rounds: u32) -> [u8; KEY_LEN] {
    let pw = normalize_password(password);
    let mut out = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(pw.as_bytes(), salt, rounds, &mut out);
    out
}

/// PBKDF2-HMAC-SHA256 派生包裹密钥（当前 v2 参数 600k）
pub(super) fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    derive_with_rounds(password, salt, PBKDF2_ROUNDS_V2)
}

/// 包裹主密钥 → v2 文件内容：`v2$pbkdf2-sha256-600000$<base64(salt||nonce||密文)>`
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(super) fn wrap_key(master: &[u8; KEY_LEN], password: &str) -> String {
    let mut salt = [0u8; SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let dk = derive_key(password, &salt);
    let cipher = Aes256Gcm::new_from_slice(&dk).expect("32 字节密钥必合法");
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), master.as_ref())
        .expect("AES-GCM 加密不应失败");
    let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ct.len());
    blob.extend_from_slice(&salt);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    format!("{V2_HEADER}{}", BASE64.encode(blob))
}

/// vault 文件格式解析结果
struct ParsedVault {
    /// salt || nonce || 密文
    blob: Vec<u8>,
    /// 该文件使用的 PBKDF2 迭代次数
    rounds: u32,
    /// 是否为 v1 历史格式（无文件头，20 万次）——解锁后需透明重包
    is_legacy_v1: bool,
}

/// 解析 vault 文件内容：v2 带头文本 / v1 裸 base64
fn parse_vault_file(encoded: &str) -> Result<ParsedVault, String> {
    let trimmed = encoded.trim();
    if let Some(rest) = trimmed.strip_prefix("v2$") {
        // v2$pbkdf2-sha256-600000$<base64…>
        let mut parts = rest.splitn(2, '$');
        let algo = parts.next().unwrap_or("");
        let b64 = parts
            .next()
            .ok_or_else(|| "主密码文件 v2 头缺少载荷".to_string())?;
        let rounds = algo
            .strip_prefix("pbkdf2-sha256-")
            .and_then(|n| n.parse::<u32>().ok())
            .filter(|n| (1..=MAX_HEADER_ROUNDS).contains(n))
            .ok_or_else(|| "主密码文件 v2 头中的 KDF 参数不合法".to_string())?;
        let blob = BASE64
            .decode(b64.trim())
            .map_err(|e| format!("主密码文件格式非法: {}", e))?;
        Ok(ParsedVault {
            blob,
            rounds,
            is_legacy_v1: false,
        })
    } else {
        // v1：裸 base64（标准 base64 字符表不含 '$'/'v' 前缀冲突风险——v 是合法
        // base64 字符，但 "v2$" 中 '$' 不可能出现在合法 v1 内容里）
        let blob = BASE64
            .decode(trimmed)
            .map_err(|e| format!("主密码文件格式非法: {}", e))?;
        Ok(ParsedVault {
            blob,
            rounds: PBKDF2_ROUNDS_V1,
            is_legacy_v1: true,
        })
    }
}

/// 解包主密钥（主密码错误返回 Err；自动识别 v1/v2 头与各自迭代次数）
pub(super) fn unwrap_key(encoded: &str, password: &str) -> Result<[u8; KEY_LEN], String> {
    let parsed = parse_vault_file(encoded)?;
    if parsed.blob.len() < SALT_LEN + NONCE_LEN + 16 {
        return Err("主密码文件长度不足".to_string());
    }
    let salt = &parsed.blob[..SALT_LEN];
    let nonce = &parsed.blob[SALT_LEN..SALT_LEN + NONCE_LEN];
    let ct = &parsed.blob[SALT_LEN + NONCE_LEN..];
    let dk = derive_with_rounds(password, salt, parsed.rounds);
    let cipher = Aes256Gcm::new_from_slice(&dk).map_err(|e| format!("密钥派生失败: {}", e))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| "主密码错误".to_string())?;
    if pt.len() != KEY_LEN {
        return Err("解出的主密钥长度非法".to_string());
    }
    let mut key = [0u8; KEY_LEN];
    key.copy_from_slice(&pt);
    Ok(key)
}

/// 文件内容是否为 v1 历史格式（解锁成功后需透明重包）
pub(super) fn is_legacy_v1_file(encoded: &str) -> bool {
    parse_vault_file(encoded)
        .map(|p| p.is_legacy_v1)
        .unwrap_or(false)
}

/// 写入 vault.key（权限 600，create_new 防并发双写）
pub(super) fn write_vault_file(path: &Path, encoded: &str) -> DbResult<()> {
    let map_err = |e: std::io::Error| DbError::Other(format!("写入主密码文件失败: {}", e));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(map_err)?;
        f.write_all(encoded.as_bytes()).map_err(map_err)?;
        f.sync_all().map_err(map_err)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, encoded).map_err(map_err)
    }
}

/// 原子替换 vault.key（v1→v2 透明重包用）：同目录临时文件(600) → fsync → rename。
/// 同目录保证 rename 原子；重包失败不影响原文件，下次解锁会再试。
pub(super) fn replace_vault_file(path: &Path, encoded: &str) -> DbResult<()> {
    let map_err = |e: std::io::Error| DbError::Other(format!("重写主密码文件失败: {}", e));
    let mut rand_bytes = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut rand_bytes);
    let suffix: String = rand_bytes.iter().map(|b| format!("{b:02x}")).collect();
    let tmp = path.with_extension(format!("key.upgrade-{suffix}.tmp"));
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(map_err)?;
        f.write_all(encoded.as_bytes()).map_err(map_err)?;
        f.sync_all().map_err(map_err)?;
        std::fs::rename(&tmp, path).map_err(map_err)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp, encoded).map_err(map_err)?;
        std::fs::rename(&tmp, path).map_err(map_err)
    }
}

pub(super) fn read_vault_file(path: &Path) -> DbResult<String> {
    std::fs::read_to_string(path).map_err(|e| DbError::Other(format!("读取主密码文件失败: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验 NFKC 归一化：全角与兼容字符归一到相同形态
    #[test]
    fn test_normalize_password() {
        assert_eq!(normalize_password("ＡＢＣ"), "ABC");
        assert_eq!(normalize_password("ﬁ"), "fi");
        assert_eq!(normalize_password("hello 123"), "hello 123");
    }

    /// 校验密钥派生：相同参数输出确定，NFKC 两侧派生一致，低轮次快速验证
    #[test]
    fn test_derive_with_rounds_deterministic_and_nfkc() {
        let salt = [42u8; SALT_LEN];
        let k1 = derive_with_rounds("ＡＢＣ-pass", &salt, 100);
        let k2 = derive_with_rounds("ABC-pass", &salt, 100);
        assert_eq!(k1, k2);

        let k3 = derive_with_rounds("ABC-pass", &salt, 101);
        assert_ne!(k1, k3);
    }

    /// 校验 v2 wrap_key 与 unwrap_key 完整闭环，且支持 NFKC 归一化解锁
    #[test]
    fn test_wrap_unwrap_roundtrip_v2() {
        let master = [7u8; KEY_LEN];
        let wrapped = wrap_key(&master, "my-secret-密码-ﬁ");

        assert!(wrapped.starts_with(V2_HEADER));
        assert!(!is_legacy_v1_file(&wrapped));

        // 相同密码解锁成功
        let unwrapped = unwrap_key(&wrapped, "my-secret-密码-ﬁ").unwrap();
        assert_eq!(unwrapped, master);

        // 兼容字符形态解锁成功
        let unwrapped_nfkc = unwrap_key(&wrapped, "my-secret-密码-fi").unwrap();
        assert_eq!(unwrapped_nfkc, master);

        // 错误密码解锁必须失败并返回友好提示
        let err = unwrap_key(&wrapped, "wrong-password").unwrap_err();
        assert_eq!(err, "主密码错误");
    }

    /// 校验 v1 历史格式（无头裸 base64）解析、判定及解锁闭环
    #[test]
    fn test_legacy_v1_format_parse_and_unwrap() {
        let master = [9u8; KEY_LEN];
        let salt = [1u8; SALT_LEN];
        let nonce = [2u8; NONCE_LEN];

        // 按照 v1 参数手动构造一个裸 base64 payload
        let dk = derive_with_rounds("v1-legacy-pass", &salt, PBKDF2_ROUNDS_V1);
        let cipher = Aes256Gcm::new_from_slice(&dk).unwrap();
        let ct = cipher.encrypt(Nonce::from_slice(&nonce), master.as_ref()).unwrap();

        let mut blob = Vec::new();
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ct);
        let raw_b64 = BASE64.encode(blob);

        assert!(is_legacy_v1_file(&raw_b64));

        let unwrapped = unwrap_key(&raw_b64, "v1-legacy-pass").unwrap();
        assert_eq!(unwrapped, master);

        let err = unwrap_key(&raw_b64, "wrong-pass").unwrap_err();
        assert_eq!(err, "主密码错误");
    }

    /// 校验畸形 payload / 篡改密文与非法 header 的防护
    #[test]
    fn test_malformed_and_tampered_payloads() {
        // 1. 乱码 base64
        assert!(unwrap_key("not-valid-base64!@#$", "pw").is_err());

        // 2. 长度不足
        let too_short = BASE64.encode([0u8; 10]);
        let err_short = unwrap_key(&too_short, "pw").unwrap_err();
        assert_eq!(err_short, "主密码文件长度不足");

        // 3. v2 头缺少载荷（无第二段 `$`）与空载荷
        let err_no_payload = unwrap_key("v2$pbkdf2-sha256-600000", "pw").unwrap_err();
        assert!(err_no_payload.contains("缺少载荷"));

        let err_empty_b64 = unwrap_key("v2$pbkdf2-sha256-600000$", "pw").unwrap_err();
        assert!(err_empty_b64.contains("格式非法") || err_empty_b64.contains("长度不足"));

        // 4. v2 头中 KDF 轮次非法（0 或超出 MAX_HEADER_ROUNDS）
        let err_zero_rounds = unwrap_key("v2$pbkdf2-sha256-0$AAAA", "pw").unwrap_err();
        assert!(err_zero_rounds.contains("KDF 参数不合法"));

        let err_huge_rounds = unwrap_key("v2$pbkdf2-sha256-9999999999$AAAA", "pw").unwrap_err();
        assert!(err_huge_rounds.contains("KDF 参数不合法"));

        // 5. 密文篡改拦截（GCM 认证标签校验失败）
        let master = [3u8; KEY_LEN];
        let wrapped = wrap_key(&master, "test-pass");
        let mut tampered = wrapped.into_bytes();
        let len = tampered.len();
        tampered[len - 2] = if tampered[len - 2] == b'A' { b'B' } else { b'A' };
        let tampered_str = String::from_utf8(tampered).unwrap();
        let err_tamper = unwrap_key(&tampered_str, "test-pass").unwrap_err();
        assert_eq!(err_tamper, "主密码错误");
    }

    /// 校验 write_vault_file / read_vault_file / replace_vault_file 文件原子操作
    #[test]
    fn test_vault_file_io_operations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vault.key");

        let content = "v2$pbkdf2-sha256-600000$test-content";
        write_vault_file(&path, content).unwrap();

        // 校验读取内容一致
        let read = read_vault_file(&path).unwrap();
        assert_eq!(read, content);

        // create_new 防二次覆盖
        assert!(write_vault_file(&path, "new-content").is_err());

        // 原子替换 replace_vault_file
        let replaced_content = "v2$pbkdf2-sha256-600000$replaced-content";
        replace_vault_file(&path, replaced_content).unwrap();
        let read2 = read_vault_file(&path).unwrap();
        assert_eq!(read2, replaced_content);
    }
}

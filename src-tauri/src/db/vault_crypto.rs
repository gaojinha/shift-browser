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
    parse_vault_file(encoded).map(|p| p.is_legacy_v1).unwrap_or(false)
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
    std::fs::read_to_string(path)
        .map_err(|e| DbError::Other(format!("读取主密码文件失败: {}", e)))
}

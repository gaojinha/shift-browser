//! 密码库主密码（阶段 2 · 数据主权底线）
//!
//! 现状：`master.key` 是机器自动生成的密钥文件（权限 600），只防"数据库文件泄露"
//! ——同机拿到 `master.key` 即可解密全部密码。
//!
//! 本模块补上**用户主密码**：
//! - 用主密码经 PBKDF2-HMAC-SHA256（随机 salt）派生包裹密钥
//! - 用包裹密钥以 AES-256-GCM 加密「真正的主密钥」，存 `vault.key`（权限 600）
//! - 设了主密码后，密码库启动即**锁定**：必须先 `unlock_vault` 解锁，
//!   主密钥才在内存中可用——同机拿到文件也无法解密
//!
//! 存储格式（vault.key）：
//! - v2（当前）：`v2$pbkdf2-sha256-600000$<base64(salt(16) || nonce(12) || 密文)>`
//! - v1（历史，无文件头）：`<base64(salt(16) || nonce(12) || 密文)>`，PBKDF2 20 万次。
//!   解锁成功后用同一密码透明重包为 v2 落盘（沿用 users.rs 的旧哈希升级模式）。
//!
//! 与 `master.key` 分离：vault.key 只包裹主密钥，不直接存密码。

use crate::db::{Db, DbError, DbResult};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use rand::RngCore;
use serde::Serialize;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tauri::State;
use unicode_normalization::UnicodeNormalization;

/// v2 PBKDF2 迭代次数（OWASP 2023 对 PBKDF2-HMAC-SHA256 建议下限 600k）
const PBKDF2_ROUNDS_V2: u32 = 600_000;
/// v1（无文件头）历史迭代次数，仅用于解锁旧文件
const PBKDF2_ROUNDS_V1: u32 = 200_000;
/// v2 文件头前缀（完整行：v2$pbkdf2-sha256-600000$<base64…>）。
/// `$` 不在标准 base64 字符表中，有无前缀无歧义。
const V2_HEADER: &str = "v2$pbkdf2-sha256-600000$";
/// 文件头中自报迭代次数的合法上限（防恶意文件填天文数字拖死解锁）
const MAX_HEADER_ROUNDS: u32 = 10_000_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
/// 主密码最短长度
const MIN_PASSWORD_LEN: usize = 8;
/// 空闲自动锁定设置键（分钟；0 = 关闭），缺省 15
const IDLE_LOCK_SETTING_KEY: &str = "vault_idle_lock_minutes";
const DEFAULT_IDLE_LOCK_MINUTES: u64 = 15;

/// 内存中的解锁会话（Some = 已解锁，持有主密钥）
static UNLOCKED: OnceLock<Mutex<Option<[u8; KEY_LEN]>>> = OnceLock::new();
/// 最近一次解锁活动时刻（解锁本身 / 取用主密钥都算活动），供空闲自动锁定判断
static LAST_ACTIVITY: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();

fn unlocked_cell() -> &'static Mutex<Option<[u8; KEY_LEN]>> {
    UNLOCKED.get_or_init(|| Mutex::new(None))
}

fn activity_cell() -> &'static Mutex<Option<std::time::Instant>> {
    LAST_ACTIVITY.get_or_init(|| Mutex::new(None))
}

/// 是否已设主密码（vault.key 存在）
pub fn is_protected(db: &Db) -> bool {
    db.vault_path.exists()
}

/// 当前会话是否已解锁
pub fn is_unlocked() -> bool {
    unlocked_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
}

/// 取解锁后的主密钥（未解锁返回 None）；取用本身记一次活动，刷新空闲计时
pub fn unlocked_key() -> Option<[u8; KEY_LEN]> {
    let key = *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner());
    if key.is_some() {
        touch_activity();
    }
    key
}

/// 记录解锁活动（刷新空闲自动锁定计时）
fn touch_activity() {
    *activity_cell()
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(std::time::Instant::now());
}

/// 空闲判定纯函数：已解锁且有活动记录、空闲达到 timeout（分钟，0=关闭）即应锁定。
/// last=None（解锁后从未记录活动，理论上不会出现）按"不锁定"保守处理。
fn should_lock_idle(
    unlocked: bool,
    last: Option<std::time::Instant>,
    now: std::time::Instant,
    timeout_minutes: u64,
) -> bool {
    match (unlocked, last, timeout_minutes) {
        (true, Some(last), t) if t > 0 => {
            now.duration_since(last).as_secs() >= t * 60
        }
        _ => false,
    }
}

/// 锁定：清除内存中的主密钥与活动计时
pub fn lock() {
    *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
    *activity_cell().lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn set_unlocked(key: [u8; KEY_LEN]) {
    *unlocked_cell().lock().unwrap_or_else(|e| e.into_inner()) = Some(key);
    touch_activity();
}

/// 主密码 NFKC 归一化：设置/解锁/移除所有入口经派生前都过这里，
/// 全角字符（ＡＢＣ）与兼容组合（ﬁ→fi）在两侧得到同一密钥。
fn normalize_password(password: &str) -> String {
    password.nfkc().collect()
}

/// PBKDF2-HMAC-SHA256 派生包裹密钥（指定迭代次数；NFKC 归一化在派生内统一完成）
fn derive_with_rounds(password: &str, salt: &[u8], rounds: u32) -> [u8; KEY_LEN] {
    let pw = normalize_password(password);
    let mut out = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<sha2::Sha256>(pw.as_bytes(), salt, rounds, &mut out);
    out
}

/// PBKDF2-HMAC-SHA256 派生包裹密钥（当前 v2 参数 600k）
pub fn derive_key(password: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    derive_with_rounds(password, salt, PBKDF2_ROUNDS_V2)
}

/// 包裹主密钥 → v2 文件内容：`v2$pbkdf2-sha256-600000$<base64(salt||nonce||密文)>`
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub fn wrap_key(master: &[u8; KEY_LEN], password: &str) -> String {
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
pub fn unwrap_key(encoded: &str, password: &str) -> Result<[u8; KEY_LEN], String> {
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
fn is_legacy_v1_file(encoded: &str) -> bool {
    parse_vault_file(encoded).map(|p| p.is_legacy_v1).unwrap_or(false)
}

/// 写入 vault.key（权限 600，create_new 防并发双写）
fn write_vault_file(path: &Path, encoded: &str) -> DbResult<()> {
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
fn replace_vault_file(path: &Path, encoded: &str) -> DbResult<()> {
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

fn read_vault_file(path: &Path) -> DbResult<String> {
    std::fs::read_to_string(path)
        .map_err(|e| DbError::Other(format!("读取主密码文件失败: {}", e)))
}

/// 密码库状态
#[derive(Debug, Serialize, Clone)]
pub struct VaultStatus {
    /// 是否已设主密码
    pub protected: bool,
    /// 当前会话是否已解锁
    pub unlocked: bool,
    /// 空闲自动锁定分钟数（0 = 关闭，缺省 15）
    pub idle_lock_minutes: u64,
}

/// 读取空闲自动锁定配置（分钟；非法/缺失取缺省 15）
async fn idle_lock_minutes(db: &Db) -> u64 {
    sqlx::query_scalar::<_, String>("SELECT value FROM settings WHERE key = ?")
        .bind(IDLE_LOCK_SETTING_KEY)
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_IDLE_LOCK_MINUTES)
}

/// 查询密码库状态
#[tauri::command]
pub async fn vault_status(db: State<'_, Db>) -> Result<VaultStatus, String> {
    Ok(VaultStatus {
        protected: is_protected(&db),
        unlocked: is_unlocked(),
        idle_lock_minutes: idle_lock_minutes(&db).await,
    })
}

/// 设置空闲自动锁定分钟数（0 = 关闭，上限 1440/24 小时）
#[tauri::command]
pub async fn set_vault_idle_lock(db: State<'_, Db>, minutes: u64) -> Result<(), String> {
    if minutes > 1440 {
        return Err("空闲锁定时间不能超过 1440 分钟（24 小时）；0 表示关闭".to_string());
    }
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(IDLE_LOCK_SETTING_KEY)
    .bind(minutes.to_string())
    .execute(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    log::info!("[vault] 空闲自动锁定设置为 {} 分钟", if minutes == 0 { "关闭".to_string() } else { minutes.to_string() });
    Ok(())
}

/// 空闲自动锁定后台守护：每 30 秒检查一次，达到配置的空闲时长即清除内存主密钥。
/// 独立于前端轮询（设置对话框不开也生效），在 app setup 中 spawn 一次。
pub fn spawn_idle_lock_watchdog(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        use tauri::{Emitter, Manager};
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        tick.tick().await; // 首次 tick 立即返回，跳过
        loop {
            tick.tick().await;
            let Some(db) = app.try_state::<Db>() else { continue };
            // 未解锁直接跳过，不碰 DB
            if !is_unlocked() {
                continue;
            }
            let timeout = idle_lock_minutes(&db).await;
            let last = *activity_cell()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if should_lock_idle(true, last, std::time::Instant::now(), timeout) {
                lock();
                log::info!("[vault] 空闲 {} 分钟，已自动锁定", timeout);
                let _ = app.emit(crate::contracts::events::VAULT_AUTO_LOCKED, ());
            }
        }
    });
}

/// 设置主密码（把当前主密钥用主密码包裹存起来）
#[tauri::command]
pub async fn set_vault_password(db: State<'_, Db>, password: String) -> Result<(), String> {
    // 长度按 NFKC 归一化后的形式计数，与派生前归一化口径一致
    let password = normalize_password(&password);
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(format!("主密码至少 {} 位", MIN_PASSWORD_LEN));
    }
    if is_protected(&db) {
        return Err("密码库已设置主密码（如需更换请先移除）".to_string());
    }
    // 取当前主密钥（此时尚未设主密码，走文件路径）
    let master = crate::db::passwords::master_key(&db)
        .await
        .map_err(|e| e.to_string())?;
    let wrapped = wrap_key(&master, &password);
    write_vault_file(&db.vault_path, &wrapped).map_err(|e| e.to_string())?;
    // 设置完成即视为已解锁（用户刚输入过密码）
    set_unlocked(master);
    log::info!("[vault] 主密码已设置（v2/pbkdf2-sha256-600000），密码库进入保护状态");
    Ok(())
}

/// 解锁密码库（验证主密码并把主密钥载入内存）
#[tauri::command]
pub async fn unlock_vault(db: State<'_, Db>, password: String) -> Result<(), String> {
    if !is_protected(&db) {
        return Err("密码库未设主密码".to_string());
    }
    let encoded = read_vault_file(&db.vault_path).map_err(|e| e.to_string())?;
    let master = unwrap_key(&encoded, &password)?;
    set_unlocked(master);
    // 透明升级：v1（无文件头，20 万次）解锁成功后用同一密码重包为 v2/60 万次落盘。
    // 失败不阻断解锁（密钥已在内存），下次解锁再试。
    if is_legacy_v1_file(&encoded) {
        let upgraded = wrap_key(&master, &password);
        match replace_vault_file(&db.vault_path, &upgraded) {
            Ok(()) => log::info!("[vault] 主密码文件已透明升级 v1(200k) → v2(600k)"),
            Err(e) => log::warn!("[vault] 主密码文件升级 v2 失败（不影响本次解锁）: {e}"),
        }
    }
    log::info!("[vault] 密码库已解锁");
    Ok(())
}

/// 锁定密码库（清除内存主密钥）
#[tauri::command]
pub async fn lock_vault() -> Result<(), String> {
    lock();
    log::info!("[vault] 密码库已锁定");
    Ok(())
}

/// 移除主密码（验证密码后删除 vault.key，回到"仅密钥文件"模式）
#[tauri::command]
pub async fn remove_vault_password(db: State<'_, Db>, password: String) -> Result<(), String> {
    if !is_protected(&db) {
        return Err("密码库未设主密码".to_string());
    }
    let encoded = read_vault_file(&db.vault_path).map_err(|e| e.to_string())?;
    let _ = unwrap_key(&encoded, &password)?; // 验证主密码
    std::fs::remove_file(&db.vault_path)
        .map_err(|e| format!("删除主密码文件失败: {}", e))?;
    // 移除后主密钥回到文件模式，保持内存缓存可用
    lock();
    log::info!("[vault] 主密码已移除");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试专用：用指定 rounds 构造 v2 形态文件（debug 下 600k 每次约 4.5s，
    /// 往返/格式类用例用小 rounds；真实 600k 仅 wrap_key 生产冒烟保留一次）
    #[allow(clippy::unwrap_used, clippy::expect_used)]
    fn wrap_v2_with_rounds(master: &[u8; KEY_LEN], password: &str, rounds: u32) -> String {
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
        format!("v2$pbkdf2-sha256-{rounds}${}", BASE64.encode(blob))
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
        assert!(err.contains("主密码错误"), "错误密码应被拒绝，实际: {}", err);
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
        assert!(unwrap_key("not-base64!!!", "x").is_err());
        assert!(unwrap_key(&BASE64.encode([0u8; 4]), "x").is_err());
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
        assert!(should_lock_idle(true, Some(now - Duration::from_secs(60)), now, 1));
    }
}

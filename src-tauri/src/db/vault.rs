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

// R1 拆分（2026-09-25）：vault.key 的 PBKDF2/AES-GCM 加解密与 v1/v2 文件格式
// 见同模块 super::vault_crypto；本文件保留内存会话、空闲守护与 Tauri 命令。

use serde::Serialize;
use std::sync::{Mutex, OnceLock};
use tauri::State;

use crate::db::Db;

// 加解密/文件格式原语（兄弟模块 vault_crypto）
use super::vault_crypto::{
    is_legacy_v1_file, normalize_password, read_vault_file, replace_vault_file, unwrap_key,
    wrap_key, write_vault_file, KEY_LEN,
};

/// CPU 密集派生扔阻塞池：PBKDF2 60 万次硬算占满一个 async worker，
/// 该 worker 期间无法服务别的 IPC。数据全 move 进去，无借用。
async fn unwrap_key_blocking(encoded: String, password: String) -> Result<[u8; KEY_LEN], String> {
    tokio::task::spawn_blocking(move || unwrap_key(&encoded, &password))
        .await
        .map_err(|e| format!("派生任务异常: {}", e))?
}

async fn wrap_key_blocking(master: [u8; KEY_LEN], password: String) -> String {
    // 兜底份：spawn 失败时降级同步算一次（闭包无 panic 路径，正常走不到）
    let (m2, p2) = (master, password.clone());
    tokio::task::spawn_blocking(move || wrap_key(&m2, &p2))
        .await
        .unwrap_or_else(|e| {
            log::error!("[vault] 派生任务异常，降级同步派生: {}", e);
            wrap_key(&master, &password)
        })
}
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
    let wrapped = wrap_key_blocking(master, password).await;
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
    let master = unwrap_key_blocking(encoded.clone(), password.clone()).await?;
    set_unlocked(master);
    // 透明升级：v1（无文件头，20 万次）解锁成功后用同一密码重包为 v2/60 万次落盘。
    // 失败不阻断解锁（密钥已在内存），下次解锁再试。
    if is_legacy_v1_file(&encoded) {
        let upgraded = wrap_key_blocking(master, password).await;
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
    let _ = unwrap_key_blocking(encoded, password).await?; // 验证主密码
    std::fs::remove_file(&db.vault_path)
        .map_err(|e| format!("删除主密码文件失败: {}", e))?;
    // 移除后主密钥回到文件模式，保持内存缓存可用
    lock();
    log::info!("[vault] 主密码已移除");
    Ok(())
}

// 密码学往返/v1 升级/空闲判定等测试随功能外置（避免门面超长）
#[cfg(test)]
mod tests;

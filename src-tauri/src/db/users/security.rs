//! 密码哈希、账号校验与登录防爆破（R2-1 拆分）

use std::collections::HashMap;
use std::sync::Mutex;

// ===== 密码哈希（PBKDF2-SHA512，迭代次数版本化）=====
//
// hash_algo 字段记录参数，便于日后升级迭代次数而不锁死老用户：
// - 历史值 'pbkdf2'（早期版本）→ 按 10 万次校验
// - 当前值 'pbkdf2-sha512-210000' → 21 万次（OWASP 2023 对 PBKDF2-SHA512 的建议下限）
// 校验成功后若发现是旧参数，用当前参数重算并透明升级落库。
//
// 为什么不再调高：本应用是本地锁屏/登录，迭代次数直接等于用户等待时间。
// release 用 opt-level="s"（体积优化），实测 60 万次要接近 1 秒，交互明显卡顿；
// 21 万次约 0.2-0.4 秒，在本地离线介质（数据库文件权限 600）威胁模型下足够。

/// 当前迭代次数
pub(crate) const PBKDF2_ITER: u32 = 210_000;

/// 历史迭代次数（老哈希只有 'pbkdf2' 标记）
pub(crate) const LEGACY_PBKDF2_ITER: u32 = 100_000;

/// 当前算法标记（写入 users.hash_algo）
pub(crate) const HASH_ALGO_CURRENT: &str = "pbkdf2-sha512-210000";

pub(crate) fn hash_password_with_iter(password: &str, salt: &str, iter: u32) -> String {
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha512;
    let mut out = [0u8; 64];
    pbkdf2_hmac::<Sha512>(password.as_bytes(), salt.as_bytes(), iter, &mut out);
    hex::encode(out)
}

/// KDF 移出 async worker（R4-3 / 方案 Q-12，2026-10-14）：
/// PBKDF2 是 CPU 密集纯计算（21 万次 SHA512 ≈ 0.2-0.4s），直接在 async fn
/// 里同步跑会占住 tokio worker 线程，阻塞并发的 E_DB 查询等其他任务。
/// 统一经 `spawn_blocking` 进阻塞池，与 vault 侧 `unwrap_key_blocking`/
/// `wrap_key_blocking`（vault.rs）同款先例。语义与同步版完全一致。
pub(crate) async fn hash_password_blocking(password: String, salt: String, iter: u32) -> String {
    let task = (password.clone(), salt.clone(), iter);
    match tokio::task::spawn_blocking(move || hash_password_with_iter(&task.0, &task.1, task.2))
        .await
    {
        Ok(hash) => hash,
        // 毒化恢复（AGENTS.md：panic 不许裸穿透）：闭包是纯计算无 panic 点，
        // 此分支理论不可达；一旦发生降级为主线程同步重算，保证结果正确性
        Err(e) => {
            log::error!("[users] KDF blocking 任务异常（{e}），降级同步重算");
            hash_password_with_iter(&password, &salt, iter)
        }
    }
}

/// 从 hash_algo 解析校验所需迭代次数（未知值按历史参数兜底）
pub(crate) fn iterations_for_algo(algo: &str) -> u32 {
    algo.strip_prefix("pbkdf2-sha512-")
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(LEGACY_PBKDF2_ITER)
}

pub(crate) fn generate_salt() -> String {
    use rand::RngCore;
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    hex::encode(salt)
}

pub(crate) fn valid_username(name: &str) -> bool {
    let len = name.chars().count();
    (2..=20).contains(&len)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

pub(crate) static LOGIN_GUARD: Mutex<Option<HashMap<String, (u32, u64)>>> = Mutex::new(None);

pub(crate) const MAX_LOGIN_FAILURES: u32 = 5;

pub(crate) const LOCK_MS: u64 = 30_000;

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn guard_get(name: &str) -> (u32, u64) {
    let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    map.get(name).copied().unwrap_or((0, 0))
}

pub(crate) fn guard_record_failure(name: &str) {
    let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    let (f, _) = map.get(name).copied().unwrap_or((0, 0));
    let f = f + 1;
    if f >= MAX_LOGIN_FAILURES {
        map.insert(name.to_string(), (0, now_ms() + LOCK_MS));
    } else {
        map.insert(name.to_string(), (f, 0));
    }
}

pub(crate) fn guard_clear(name: &str) {
    let mut g = LOGIN_GUARD.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(map) = g.as_mut() {
        map.remove(name);
    }
}

// ===== 数据库辅助 =====

pub(crate) fn today_str() -> String {
    // 简单 YYYY-MM-DD（UTC 近似，够文件名用）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// 天数 → 公历日期（Howard Hinnant 算法）
pub(crate) fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, (m as i64), (d as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 校验用户名字符集与长度（2..=20）边界
    #[test]
    fn test_valid_username_rules() {
        // 合法长度与字符
        assert!(valid_username("ab"));
        assert!(valid_username("user_123"));
        assert!(valid_username("test-user"));
        assert!(valid_username("12345678901234567890")); // 20 字符

        // 长度越界
        assert!(!valid_username(""));
        assert!(!valid_username("a"));
        assert!(!valid_username("123456789012345678901")); // 21 字符

        // 非法字符拦截
        assert!(!valid_username("user name"));
        assert!(!valid_username("user@domain"));
        assert!(!valid_username("user.name"));
        assert!(!valid_username("user#1"));
    }

    /// 校验算法标识与迭代次数映射，以及未知算法的向下兼容兜底
    #[test]
    fn test_iterations_for_algo() {
        assert_eq!(iterations_for_algo(HASH_ALGO_CURRENT), PBKDF2_ITER);
        assert_eq!(iterations_for_algo("pbkdf2-sha512-100000"), 100_000);
        assert_eq!(iterations_for_algo("pbkdf2-sha512-50000"), 50_000);

        // 未知或老格式 'pbkdf2' 均兜底为历史参数 100,000
        assert_eq!(iterations_for_algo("pbkdf2"), LEGACY_PBKDF2_ITER);
        assert_eq!(iterations_for_algo("unknown-algo"), LEGACY_PBKDF2_ITER);
        assert_eq!(iterations_for_algo("pbkdf2-sha512-notanumber"), LEGACY_PBKDF2_ITER);
    }

    /// 校验密码哈希在相同/不同参数下的确定性与雪崩效应
    #[test]
    fn test_hash_password_with_iter_deterministic() {
        let pw = "mypassword123";
        let salt = "a1b2c3d4e5f60718";

        let h1 = hash_password_with_iter(pw, salt, 100);
        let h2 = hash_password_with_iter(pw, salt, 100);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 128); // 64 字节 SHA-512 hex 编码后为 128 字符

        let h3 = hash_password_with_iter(pw, salt, 101);
        assert_ne!(h1, h3);

        let h_diff_pw = hash_password_with_iter("mypassword124", salt, 100);
        assert_ne!(h1, h_diff_pw);

        let h_diff_salt = hash_password_with_iter(pw, "b1b2c3d4e5f60718", 100);
        assert_ne!(h1, h_diff_salt);
    }

    /// 校验异步 blocking 版 hash_password_blocking 计算一致性
    #[tokio::test]
    async fn test_hash_password_blocking_consistency() {
        let pw = "async_test_pw".to_string();
        let salt = "deadbeefcafebabe".to_string();
        let iter = 200;

        let sync_hash = hash_password_with_iter(&pw, &salt, iter);
        let async_hash = hash_password_blocking(pw, salt, iter).await;
        assert_eq!(sync_hash, async_hash);
    }

    /// 校验随机 salt 长度（16 字节 = 32 hex 字符）与唯一性
    #[test]
    fn test_generate_salt() {
        let s1 = generate_salt();
        let s2 = generate_salt();
        assert_eq!(s1.len(), 32);
        assert_eq!(s2.len(), 32);
        assert_ne!(s1, s2);
        assert!(s1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// 校验防爆破守卫：失败递增、锁定触发、锁定到期时间及成功清理
    #[test]
    fn test_login_guard_failure_and_lockout() {
        let test_user = "test_guard_user";
        guard_clear(test_user);

        // 初始状态
        assert_eq!(guard_get(test_user), (0, 0));

        // 记录 1 到 4 次失败（未达到 5 次，不锁定）
        for expected_f in 1..MAX_LOGIN_FAILURES {
            guard_record_failure(test_user);
            let (f, lock_until) = guard_get(test_user);
            assert_eq!(f, expected_f);
            assert_eq!(lock_until, 0);
        }

        // 第 5 次失败，触发锁定：失败计数清 0，lock_until 设置为未来时间
        let before_now = now_ms();
        guard_record_failure(test_user);
        let (f, lock_until) = guard_get(test_user);
        assert_eq!(f, 0);
        assert!(lock_until >= before_now + LOCK_MS);

        // 成功登录后清除守卫状态
        guard_clear(test_user);
        assert_eq!(guard_get(test_user), (0, 0));
    }

    /// 校验历法换算算法 civil_from_days 关键基准点
    #[test]
    fn test_civil_from_days_benchmarks() {
        // 1970-01-01 -> epoch day 0
        assert_eq!(civil_from_days(0), (1970, 1, 1));

        // 2000-01-01 -> 10957 days
        assert_eq!(civil_from_days(10957), (2000, 1, 1));

        // 闰年：2024-02-29 -> 19782 days
        assert_eq!(civil_from_days(19782), (2024, 2, 29));

        // 2024-03-01 -> 19783 days
        assert_eq!(civil_from_days(19783), (2024, 3, 1));
    }
}

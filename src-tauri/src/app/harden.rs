//! 敏感文件权限加固（安全 · 2026-09-22）

/// 收紧文件/目录权限（unix）。安全加固的唯一实现：失败**不再静默**——权限收紧失败
/// （回退 umask 常见 644/664）会让同机其他用户读到 db/密码/cookie，必须留痕
/// （F-79 D2：`let _ =` 吞掉安全相关错误）。
#[cfg(unix)]
pub(crate) fn restrict_mode(path: &std::path::Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
        log::warn!("[安全] 收紧权限失败 {:o} {:?}: {}", mode, path, e);
    }
}

/// Cookie 库/侧车 WAL 按 umask 落盘时常见 664（组/其他用户可读），会话 Cookie
/// 泄露风险。应用数据目录虽为 0700 兜底，但纵深防御要求文件自身也为 0600。
/// 运行时 WebKit 可能重建这些文件，故需周期重扫（见 bootstrap 守卫）。
#[cfg(unix)]
pub fn harden_sensitive_files(dir: &std::path::Path) {
    const SENSITIVE: &[&str] = &[
        "cookies",
        "cookies.sqlite",
        "cookies.bak",
        "cookies-wal",
        "cookies-shm",
        "cookies.sqlite-wal",
        "cookies.sqlite-shm",
        "shift-browser.db-wal",
        "shift-browser.db-shm",
    ];
    for name in SENSITIVE {
        let p = dir.join(name);
        if p.exists() {
            restrict_mode(&p, 0o600);
        }
    }
    // 日志目录同样收紧（隐私：日志虽已脱敏，仍不应组/其他可读）
    let logs = dir.join("logs");
    if logs.exists() {
        restrict_mode(&logs, 0o700);
        if let Ok(rd) = std::fs::read_dir(&logs) {
            for e in rd.flatten() {
                restrict_mode(&e.path(), 0o600);
            }
        }
    }
}

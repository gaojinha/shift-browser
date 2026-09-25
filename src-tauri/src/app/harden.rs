//! 敏感文件权限加固（安全 · 2026-09-22）

/// Cookie 库/侧车 WAL 按 umask 落盘时常见 664（组/其他用户可读），会话 Cookie
/// 泄露风险。应用数据目录虽为 0700 兜底，但纵深防御要求文件自身也为 0600。
/// 运行时 WebKit 可能重建这些文件，故需周期重扫（见 bootstrap 守卫）。
#[cfg(unix)]
pub fn harden_sensitive_files(dir: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
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
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600));
        }
    }
    // 日志目录同样收紧（隐私：日志虽已脱敏，仍不应组/其他可读）
    let logs = dir.join("logs");
    if logs.exists() {
        let _ = std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o700));
        if let Ok(rd) = std::fs::read_dir(&logs) {
            for e in rd.flatten() {
                let _ = std::fs::set_permissions(e.path(), std::fs::Permissions::from_mode(0o600));
            }
        }
    }
}

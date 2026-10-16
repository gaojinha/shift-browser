//! 公共面安全加固集成测试：`harden_sensitive_files`（tempfile 真文件，默认可跑）。
//!
//! tests/ 目录此前只有 3 个 `#[ignore]` 真机验收（live），本文件是第一个
//! 默认运行的集成测试，从外部消费者视角锁死 `pub use` 公共契约：
//! 9 个敏感文件名 → 0600；logs 目录 → 0700、目录内文件 → 0600。
//!
//! 背景：Cookie 库/侧车 WAL 按 umask 落盘时常见 664（组/其他可读），
//! 会话 Cookie 泄露风险；WebKit 可能运行时重建文件，bootstrap 守卫周期重扫，
//! 本测试锁死"重扫一次后权限必须收敛"的行为契约（防 SENSITIVE 名单/模式漂移）。

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;

use shift_browser_lib::harden_sensitive_files;

/// 与实现内 SENSITIVE 名单同源的 9 个文件名（漂移时本文件编译/断言会暴露差异）
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

fn mode_of(p: &std::path::Path) -> u32 {
    std::fs::metadata(p).unwrap().permissions().mode()
}

fn set_mode(p: &std::path::Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn sensitive_files_converge_to_0600() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    // 复现真实场景：umask 落盘成 664（组/其他可读）
    for n in SENSITIVE {
        let p = d.join(n);
        std::fs::write(&p, b"x").unwrap();
        set_mode(&p, 0o664);
    }
    harden_sensitive_files(d);
    for n in SENSITIVE {
        assert_eq!(mode_of(&d.join(n)) & 0o777, 0o600, "{n} 应收敛到 0600");
    }
}

#[test]
fn logs_dir_converges_to_0700_and_files_to_0600() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let logs = d.join("logs");
    std::fs::create_dir(&logs).unwrap();
    set_mode(&logs, 0o755);
    let f = logs.join("app.log");
    std::fs::write(&f, b"x").unwrap();
    set_mode(&f, 0o644);

    harden_sensitive_files(d);

    assert_eq!(mode_of(&logs) & 0o777, 0o700, "logs 目录应收敛到 0700");
    assert_eq!(mode_of(&f) & 0o777, 0o600, "日志文件应收敛到 0600");
}

#[test]
fn missing_files_and_logs_are_noop() {
    // 空目录（无敏感文件、无 logs）→ 不 panic、无副作用
    let dir = tempfile::tempdir().unwrap();
    harden_sensitive_files(dir.path());
}

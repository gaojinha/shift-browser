//! 平台适配层（P0-2 · 2026-09-22）
//!
//! 目标：把散落在 `tabs.rs` / `lib.rs` 里的 WebKitGTK/GTK 直接调用收敛到本层，
//! 为跨平台（Windows WebView2 / Android 系统 WebView）留出边界。
//!
//! 约定：**业务模块不直接 use webkit2gtk/gtk**，一切经 `engine::webview` 或本层。

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(target_os = "linux"))]
pub mod unsupported;

// 统一转发（业务模块只认 engine::platform::xxx，不关心平台）
#[cfg(target_os = "linux")]
pub use linux::{
    apply_cookie_policy, clear_web_data, download_dir, init_cookie_persistence,
    setup_web_extensions_dir, web_extensions_dir,
};

#[cfg(not(target_os = "linux"))]
pub use unsupported::{
    apply_cookie_policy, clear_web_data, download_dir, init_cookie_persistence,
    setup_memory_pressure, setup_web_extensions_dir, web_extensions_dir,
};

// ── 下载目录用户覆盖（跨平台状态，供 GTK 同步回调读）─────────────────────
// 用户可在设置里指定自定义下载目录；空串 = 未覆盖，回退系统 XDG 目录。
// 用 RwLock 而非原子量：路径是变长字符串，且写频极低（设置保存时才写一次）。
use std::sync::RwLock;

static DOWNLOAD_DIR_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// 设置下载目录覆盖值（空串 = 清除覆盖，回退系统默认）
pub fn set_download_dir_override(path: &str) {
    let trimmed = path.trim();
    *DOWNLOAD_DIR_OVERRIDE.write().unwrap() =
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
}

/// 获取下载目录覆盖值（None = 未设置，调用方回退系统默认）
pub fn get_download_dir_override() -> Option<String> {
    DOWNLOAD_DIR_OVERRIDE.read().unwrap().clone()
}

/// 解析下载目标目录（**真实运行逻辑**，与 GTK 下载回调所用完全一致）。
///
/// 优先级：用户设置覆盖（`download_dir` 键）> 系统 XDG 目录 > `~/Downloads` 兜底。
/// 028 件一真机等价验证：把这段原本内联在 GTK 回调里的解析抽成本纯函数，
/// 使「覆盖态生效」可单测（行为保持——gtk.rs 现调用本函数）。
pub fn resolve_download_dir() -> String {
    get_download_dir_override()
        .or_else(|| download_dir().map(|p| p.to_string_lossy().to_string()))
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            format!("{}/Downloads", home)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::backend::unique_download_path;

    // 覆盖态是全局 RwLock，多个测试并行线程写它会竞态，
    // 用静态锁把测试串行化（cargo test 默认并行跑）。
    static OVERRIDE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// 028 件一真机等价验证：`download_dir` 覆盖态在实际运行环境生效。
    ///
    /// 本环境无显示、无法起 GUI 触发真实 WebKit 下载；故走「等价可信验证」：
    /// 覆盖态 → 解析出的目录（gtk.rs 实际调用的 `resolve_download_dir`）→ 真实落盘路径，
    /// 证明覆盖值确实被下载链路读取并优先于 XDG/`~/Downloads`。
    #[test]
    fn download_dir_override_resolution_and_landing() {
        let _g = OVERRIDE_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        // 唯一覆盖目录：pid + 纳秒，避免与其他测试/运行串扰
        let override_dir = std::env::temp_dir().join(format!(
            "shift-dl-verify-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let override_str = override_dir.to_string_lossy().to_string();

        // 1) 覆盖态生效：覆盖值 > 系统 XDG > ~/Downloads
        set_download_dir_override(&override_str);
        assert_eq!(
            resolve_download_dir(),
            override_str,
            "设置覆盖后必须取最高优先级"
        );

        // 2) 真实落盘路径：建目录 + 唯一命名，与真实下载回调同一链路
        std::fs::create_dir_all(&override_dir).unwrap();
        let dir = resolve_download_dir();
        let dest = unique_download_path(&dir, "evidence.txt");
        assert!(
            dest.starts_with(&override_str),
            "落盘路径 {} 必须落在覆盖目录 {} 下",
            dest,
            override_str
        );
        assert_eq!(dest, format!("{}/evidence.txt", override_str));

        // 3) 同名去重：已存在则加 (1) 后缀，绝不覆盖
        std::fs::write(&dest, b"first").unwrap();
        let dest2 = unique_download_path(&dir, "evidence.txt");
        assert_eq!(dest2, format!("{}/evidence(1).txt", override_str));

        // 4) 空串覆盖 = 清除：回退系统默认（非空，且不再是覆盖目录）
        set_download_dir_override("");
        let fallback = resolve_download_dir();
        assert!(!fallback.is_empty(), "回退目录不得为空");
        assert_ne!(
            fallback, override_str,
            "清除覆盖后不得再指向覆盖目录"
        );

        // 清理
        let _ = std::fs::remove_dir_all(&override_dir);
    }

    /// 覆盖值需去首尾空白（用户可能误输空格）。
    #[test]
    fn download_dir_override_trims_whitespace() {
        let _g = OVERRIDE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_download_dir_override("   /tmp/trim-check   ");
        assert_eq!(resolve_download_dir(), "/tmp/trim-check");
        set_download_dir_override(""); // 还原，避免串扰
    }
}

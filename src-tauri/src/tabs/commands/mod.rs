//! 标签命令门面（R2-1 拆分 · 2026-09-24）
//!
//! 29 个 #[tauri::command] 从 tabs.rs 迁出，按主题拆为 tabs_cmds（生命周期/导航）
//! 与 ui_cmds（窗口/布局/可见性）两个子模块，本文件仅声明并重导出。

// P2-1：本模块现有 unwrap/expect 均为安全用法（Mutex 毒化 / Response::builder 硬编码 header）

mod tabs_cmds;
mod ui_cmds;

pub use tabs_cmds::*;
pub use ui_cmds::*;

#[cfg(test)]
mod tests {
    use crate::tabs::*;
    use crate::tabs::util::sanitize_filename;

    #[test]
    fn internal_newtab_url_detection() {
        // shift:// 协议（当前实际使用）
        assert!(is_internal_newtab_url("shift://newtab"));
        assert!(is_internal_newtab_url("shift://newtab/"));
        assert!(is_internal_newtab_url("shift://worldnav"));
        assert!(is_internal_newtab_url("shift://worldnav/world-nav.html"));
        // 旧 file:// 路径（兼容）
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/index.html"
        ));
        assert!(is_internal_newtab_url(
            "file:///home/gao/.local/share/com.shiftbrowser.app/newtab/world-nav.html"
        ));
        // 普通网页/其他协议不是启动页
        assert!(!is_internal_newtab_url("https://example.com"));
        assert!(!is_internal_newtab_url("http://example.com/newtab/"));
        assert!(!is_internal_newtab_url("file:///etc/passwd"));
        assert!(!is_internal_newtab_url("shift://__translate__/text"));
    }

    #[test]
    fn unique_path_adds_suffix_when_exists() {
        let dir = std::env::temp_dir().join("sb-unique-test");
        let _ = std::fs::create_dir_all(&dir);
        let dir = dir.to_str().unwrap();

        // 不存在 → 原文件名
        let p1 = unique_download_path(dir, "a.pdf");
        assert_eq!(p1, format!("{}/a.pdf", dir));

        // 创建后 → a(1).pdf
        std::fs::write(&p1, "x").unwrap();
        let p2 = unique_download_path(dir, "a.pdf");
        assert_eq!(p2, format!("{}/a(1).pdf", dir));

        // 再创建 → a(2).pdf
        std::fs::write(&p2, "x").unwrap();
        let p3 = unique_download_path(dir, "a.pdf");
        assert_eq!(p3, format!("{}/a(2).pdf", dir));

        // 无扩展名
        let p4 = unique_download_path(dir, "noext");
        assert_eq!(p4, format!("{}/noext", dir));
        std::fs::write(&p4, "x").unwrap();
        let p5 = unique_download_path(dir, "noext");
        assert_eq!(p5, format!("{}/noext(1)", dir));

        let _ = std::fs::remove_dir_all(std::env::temp_dir().join("sb-unique-test"));
    }

    #[test]
    fn sanitize_filename_blocks_path_traversal() {
        // 路径分隔符剥离
        assert_eq!(sanitize_filename("a/../b.txt"), "a..b.txt");
        assert_eq!(sanitize_filename("..\\evil.txt"), "evil.txt");
        // 纯穿越段 → 默认名
        assert_eq!(sanitize_filename(".."), "download");
        assert_eq!(sanitize_filename("."), "download");
        assert_eq!(sanitize_filename(""), "download");
        // 空字节/控制字符剥离
        assert_eq!(sanitize_filename("a\0b.txt"), "ab.txt");
        assert_eq!(sanitize_filename("a\nb.txt"), "ab.txt");
        // 正常名保留
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        // 首尾点段剥离（防隐藏文件）
        assert_eq!(sanitize_filename(".secret"), "secret");
        assert_eq!(sanitize_filename("..hidden"), "hidden");
        // 超长截断
        let long = "x".repeat(500);
        assert_eq!(sanitize_filename(&long).len(), 200);
    }
}


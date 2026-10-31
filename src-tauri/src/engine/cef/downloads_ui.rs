//! CEF 工具栏的下载面板后端（F-CEF-28，只做「清空已结束」）。
//!
//! 数据层在 `db/downloads.rs`：`clear_finished` 只删非 in_progress 记录
//! （同 `insert_download` 的清理排除口径），返回删除行数。
//! 清空走 `SHIFT_ACT:dlclear`，成功后调 `shell::request_downloads_refresh()` 刷新面板。
//!
//! 接线现状（2026-10-31 二狗核实，已全部接通）：
//! - `shell.rs::handle_action` 已路由到本模块 `handle(cmd, arg)`；
//! - 面板开合/列表刷新由 `shell.rs` 的 `Panel` 注册表与 `push_downloads` 负责；
//! - `cef_cmd.rs` 启动时调 `downloads_ui::init(app)`。
//!
//! 接线完成后已移除本文件的 `#![allow(dead_code)]`（不再需要 scoped 豁免）。

use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, _arg: &str) -> bool {
    match cmd {
        "dlclear" => {
            clear_finished_downloads();
            true
        }
        _ => false,
    }
}

/// 清空已结束下载记录（dlclear 动作）。
fn clear_finished_downloads() {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    match tauri::async_runtime::block_on(crate::db::downloads::clear_finished(&db.pool)) {
        Ok(n) => {
            log::info!("[cef] 已清空已结束下载记录 {n} 条");
            super::shell::request_downloads_refresh();
        }
        Err(e) => log::warn!("[cef] 清空已结束下载记录失败: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_cmd_not_handled() {
        // 不吞他模块的命令
        assert!(!handle("sessdel", "1"));
        assert!(!handle("groupdel", "1"));
        assert!(!handle("usdel", "1"));
        assert!(!handle("nav", "https://example.com"));
    }

    #[test]
    fn own_cmds_swallowed() {
        // dlclear 无 APP 时静默返回（不触 DB），仍视为已处理
        assert!(handle("dlclear", ""));
        assert!(handle("dlclear", "  "));
    }
}

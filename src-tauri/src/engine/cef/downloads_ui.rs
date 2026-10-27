//! CEF 工具栏的下载面板后端（F-CEF-28，只做「清空已结束」）。
//!
//! 数据层在 `db/downloads.rs`：`clear_finished` 只删非 in_progress 记录
//! （同 `insert_download` 的清理排除口径），返回删除行数。
//! 清空走 `SHIFT_ACT:dlclear`，删除后列表刷新留 TODO（协调器接线时补 `push_downloads`）。
//!
//! 接线说明（协调器接线时补，本模块不改 shell.rs / toolbar.rs / cef_cmd.rs）：
//! - `shell.rs::handle_action` 在分组分支之后同模式路由：`super::downloads_ui::handle(cmd, arg)`；
//! - 面板开合/列表刷新：shell.rs 注册表（`enum Panel`）加 `Download` 变体
//!   （`class()`/`extra_px()`/`atom()` + `Panel::ALL`，**别再造手写清单**），
//!   并加 `set_download_panel(open)` + `push_downloads()`（同 `set_groups_panel`/`push_groups` 模式）；
//!   接通后删掉本文件 `dlclear` 分支里的 TODO，改为真实调用 `push_downloads()`；
//! - 工具栏 HTML 生成处（cef_cmd.rs 的 `groups_ui::init/groups_json` 旁）加
//!   `downloads_ui::init(app)`；
//! - 工具栏 JS 加「清空已结束」按钮（二次确认在页面侧）。
//!
//! 本模块先交付后接线：协调器接 `shell.rs::handle_action` 与 cef_cmd.rs 之前
//! （本卡禁改这几文件），整模块暂无业务引用——scoped dead_code 豁免（同
//! `groups_ui.rs` 交付期），**接线完成后删掉下一行 `#![allow]`**。
#![allow(dead_code)]

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
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::downloads::clear_finished(&db.pool)) {
        Ok(n) => {
            log::info!("[cef] 已清空已结束下载记录 {n} 条");
            // TODO(接线，二狗补)：清空成功后经 push_downloads() 重渲染列表
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

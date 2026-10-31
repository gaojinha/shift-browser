//! CEF 工具栏的密码面板后端（F-CEF-12）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（只含 id/站点/用户名，不含密码）；
//! 复制/删除走 `SHIFT_ACT:pwcopy|<id>` / `pwdel|<id>`。明文只在复制瞬间存在于
//! Rust 内存，写入系统剪贴板后立即 zeroize，剪贴板 30 秒后若仍是该内容则清空。

use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use zeroize::Zeroize;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// 剪贴板自动清空时限
const CLIPBOARD_CLEAR_SECS: u64 = 30;

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 密码元信息 JSON（不含密码、不含备注），供内嵌进工具栏页面
pub fn passwords_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    let rows = tauri::async_runtime::block_on(crate::db::passwords::list_passwords_inner(&db));
    match rows {
        Ok(rows) => {
            log::info!("[cef] 密码列表同步读取 {} 条（仅元信息）", rows.len());
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|p| {
                    serde_json::json!({ "id": p.id, "domain": p.domain, "username": p.username })
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
        }
        Err(e) => {
            log::warn!("[cef] 读密码列表失败: {e}");
            "[]".to_string()
        }
    }
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        "pwcopy" => {
            if let Ok(id) = arg.parse::<i64>() {
                copy_password(id);
            }
            true
        }
        "pwpanel" => {
            super::shell::set_pw_panel(arg == "1");
            true
        }
        "pwdel" => {
            if let Ok(id) = arg.parse::<i64>() {
                delete_password(id);
            }
            true
        }
        _ => false,
    }
}

fn copy_password(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    let res = tauri::async_runtime::block_on(
        crate::db::passwords::get_password_plain_trusted_inner(&db, id),
    );
    let plain = match res {
        Ok(Some(p)) => p,
        Ok(None) => {
            log::warn!("[cef] 复制密码：id={id} 不存在");
            return;
        }
        Err(e) => {
            log::warn!("[cef] 复制密码失败 id={id}: {e}");
            return;
        }
    };
    let digest = Sha256::digest(plain.as_bytes());
    // F-CEF-24d：`gtk::Clipboard` 与 `glib::*_add_local` 只能在 GTK 主线程，
    // 而本函数由 `handle_action`（CEF UI 线程）进入——必须经 gtk_ui 投递，
    // 否则 multi_threaded=1 下 glib 报 "main context already acquired" panic。
    super::shell::gtk_ui(move || {
        let mut plain = plain;
        let clip = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
        clip.set_text(&plain);
        plain.zeroize();
        log::info!("[cef] 密码 id={id} 已复制到剪贴板，{CLIPBOARD_CLEAR_SECS}s 后自动清空");

        gtk::glib::timeout_add_local_once(
            std::time::Duration::from_secs(CLIPBOARD_CLEAR_SECS),
            move || {
                let clip = gtk::Clipboard::get(&gtk::gdk::SELECTION_CLIPBOARD);
                let still_ours = clip
                    .wait_for_text()
                    .map(|t| Sha256::digest(t.as_bytes()) == digest)
                    .unwrap_or(false);
                if still_ours {
                    clip.clear();
                    log::info!("[cef] 剪贴板中的密码已自动清空");
                }
            },
        );
    });
}

fn delete_password(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    match tauri::async_runtime::block_on(crate::db::passwords::delete_password_inner(&db, id)) {
        Ok(()) => log::info!("[cef] 已删除密码 id={id}"),
        Err(e) => log::warn!("[cef] 删除密码失败 id={id}: {e}"),
    }
}

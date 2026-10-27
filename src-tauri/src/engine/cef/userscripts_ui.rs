//! CEF 工具栏的用户脚本面板后端（F-CEF-22）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（只含面板展示字段：id/name/enabled/url_patterns/created_at；
//! **不输出** `code` 源码——注入路径走 `db::userscripts` 的注入缓存，面板不需要源码）。
//! 启停/删除走 `SHIFT_ACT:ustoggle|<id>` / `SHIFT_ACT:usdel|<id>`（userscripts 表自增 id 即唯一标识；
//! 删除二次确认在页面侧）。
//!
//! 接线说明（协调器接线时补，本模块不改 shell.rs / toolbar.rs）：
//! - `shell.rs::handle_action` 在证书分支之后同模式路由：`super::userscripts_ui::handle(cmd, arg)`；
//! - 面板开合/视图加高：shell.rs 注册表（`enum Panel`）加 `Userscript` 变体
//!   （`class()`/`extra_px()`/`atom()` + `Panel::ALL`，**别再造手写清单**），
//!   并加 `set_userscript_panel(open)`（同 `set_cert_panel` 模式），uspanel 分支接通；
//! - `usrefresh` 经 `shell::run_toolbar_js` 执行 `window.__shiftUserscripts(json)` 重渲染列表
//!   （`run_toolbar_js` 是 shell.rs 私有函数，接线时一并处理，同 push_certs 模式）；
//! - 工具栏 HTML 生成处（cef_cmd.rs 的 `certificates_ui::init/certificates_json` 旁）加
//!   `userscripts_ui::init(app)` + `userscripts_json(app)` 内嵌；
//! - 启停/删除后 CEF 页面注入缓存 `extensions::cef_inject::build_and_cache` 需重建（已接通：
//!   `rebuild_cef_inject_cache`，在 `toggle_script`/`delete_script` 成功后调用）；
//! - 工具栏 JS 加 `__shiftUserscripts` 渲染函数（列表 + 启停 checkbox + 删除二次确认，页面侧）。
//!
//! 本模块先交付后接线：协调器接 `shell.rs::handle_action` 与 cef_cmd.rs HTML 内嵌之前
//! （本卡禁改这两文件），整模块暂无业务引用——scoped dead_code 豁免（同 `certificates_ui.rs`），
//! **接线完成后删掉下一行 `#![allow]`**。
// 接线已完成（2026-10-27）：shell.rs 注册表 / toolbar.rs / cef_cmd.rs 已接通，dead_code 豁免移除

use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 用户脚本元信息 JSON（id/name/enabled/url_patterns/created_at），供内嵌进工具栏页面。
/// `url_patterns` = DB `matches` 字段（换行分隔的匹配模式）；表无 `updated_at` 列，
/// 时间字段为 `created_at`（回执已注明口径）。不输出 `code`（源码）。
pub fn userscripts_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    match tauri::async_runtime::block_on(crate::db::userscripts::list_userscripts(&db.pool)) {
        Ok(rows) => {
            log::info!("[cef] 用户脚本列表同步读取 {} 条（仅展示字段）", rows.len());
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|u| {
                    serde_json::json!({
                        "id": u.id,
                        "name": u.name,
                        "enabled": u.enabled,
                        "url_patterns": u.matches,
                        "created_at": u.created_at,
                    })
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
        }
        Err(e) => {
            log::warn!("[cef] 读用户脚本列表失败: {e}");
            "[]".to_string()
        }
    }
}

/// 解析动作参数 `<id>`（userscripts 表自增 id，≥ 1）
fn parse_id(arg: &str) -> Option<i64> {
    let v = arg.trim().parse::<i64>().ok()?;
    (v > 0).then_some(v)
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        "uspanel" => {
            super::shell::set_userscript_panel(arg == "1");
            if arg == "1" {
                super::shell::push_userscripts();
            }
            true
        }
        "usrefresh" => {
            super::shell::push_userscripts();
            true
        }
        "ustoggle" => {
            match parse_id(arg) {
                Some(id) => toggle_script(id),
                None => log::warn!("[cef] ustoggle 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        "usdel" => {
            match parse_id(arg) {
                Some(id) => delete_script(id),
                None => log::warn!("[cef] usdel 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        _ => false,
    }
}

fn rebuild_cef_inject_cache(app: &AppHandle) {
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    tauri::async_runtime::block_on(async {
        if let Err(e) = crate::extensions::cef_inject::build_and_cache(&db).await {
            log::warn!("[cef] 注入缓存重建失败（下条标签重新载入时恢复）: {e}");
        }
    });
}

fn toggle_script(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::userscripts::toggle_userscript(&db.pool, id)) {
        Ok(Some(enabled)) => {
            log::info!("[cef] 用户脚本 {id} 启用状态 → {enabled}");
            // 注入缓存保持同步（WebKit 路径复用同一份）
            tauri::async_runtime::block_on(crate::db::userscripts::refresh_cache(&db.pool));
            // 重建 CEF 页面注入缓存 + 重渲染工具栏列表
            rebuild_cef_inject_cache(app);
            super::shell::push_userscripts();
        }
        Ok(None) => log::warn!("[cef] ustoggle: 用户脚本 {id} 不存在"),
        Err(e) => log::warn!("[cef] 切换用户脚本 {id} 失败: {e}"),
    }
}

fn delete_script(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::userscripts::delete_userscript_inner(
        &db.pool,
        id,
    )) {
        Ok(()) => {
            log::info!("[cef] 已删除用户脚本 {id}");
            tauri::async_runtime::block_on(crate::db::userscripts::refresh_cache(&db.pool));
            // 重建 CEF 页面注入缓存 + 重渲染工具栏列表
            rebuild_cef_inject_cache(app);
            super::shell::push_userscripts();
        }
        Err(e) => log::warn!("[cef] 删除用户脚本 {id} 失败: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_id_forms() {
        assert_eq!(parse_id("7"), Some(7));
        assert_eq!(parse_id(" 7 "), Some(7));
        assert_eq!(parse_id("0"), None);
        assert_eq!(parse_id("-1"), None);
        assert_eq!(parse_id("abc"), None);
        assert_eq!(parse_id(""), None);
        assert_eq!(parse_id("99999999999999999999"), None); // 溢出
    }

    #[test]
    fn unknown_cmd_not_handled() {
        // 不吞他模块的命令
        assert!(!handle("certdel", "example.com:443"));
        assert!(!handle("nav", "https://example.com"));
        assert!(!handle("pwcopy", "1"));
    }

    #[test]
    fn own_cmds_swallowed() {
        // 接线后 uspanel/usrefresh 走 shell setter（APP 未初始化，走 no-op 路径）；
        // usdel/ustoggle 参数无效分支吞（warn 日志，不触 DB）
        assert!(handle("uspanel", "1"));
        assert!(handle("usrefresh", ""));
        assert!(handle("usdel", "abc"));
        assert!(handle("ustoggle", ""));
    }
}

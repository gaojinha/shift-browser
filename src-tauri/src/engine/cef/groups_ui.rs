//! CEF 工具栏的分组/工作区面板后端（F-CEF-27）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（`groups_json` 只给面板展示字段：
//! 分组 id/name/color/collapsed；工作区 id/name/color/sort_order/session_count。
//! **不输出** sessions.data 标签快照等大字段——不进工具栏 HTML）。
//!
//! 口径说明：
//! - 分组「标签数」是前端运行时概念（`tab.groupId`，见 `stores/groups.ts`），
//!   数据库无对应列，本 JSON 不给——UI 需要时由前端 TabManager 补；
//! - 工作区 `session_count` 是库内精确值（sessions.workspace_id 计数）。
//!
//! 删除走 `SHIFT_ACT:groupdel|<id>` / `spacedel|<id>`（两表自增 id 即唯一标识，
//! 二次确认在页面侧）。删除成功后调 `super::shell::push_groups()` 刷新面板。
//!
//! 接线现状（2026-10-31 二狗核实，已全部接通）：
//! - `shell.rs::handle_action` 已路由到本模块 `handle(cmd, arg)`；
//! - `shell.rs` 面板注册表 `enum Panel` 已有 `Groups` 变体，`set_groups_panel`/`push_groups` 就绪；
//! - 工具栏 HTML 由 `cef_cmd.rs` 内嵌 `groups_json`，页面侧 `__shiftGroups` 渲染。
//!
//! 本文件只负责动作处理与 JSON 生成，不再有"待接线"分支。

use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

const EMPTY_JSON: &str = r#"{"groups":[],"workspaces":[]}"#;

/// 分组/工作区元信息 JSON（`{groups:[...], workspaces:[...]}`），供内嵌进工具栏页面。
/// 只给展示字段：分组 id/name/color/collapsed；工作区 id/name/color/sort_order/session_count。
/// 不输出 sessions.data（标签快照全量）——可能很大，面板也只需会话数。
pub fn groups_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return EMPTY_JSON.to_string();
    };
    let pool = db.pool.clone();
    match tauri::async_runtime::block_on(async {
        let groups = crate::db::tab_groups::list_tab_groups_inner(&pool).await?;
        let workspaces = crate::db::workspaces::list_workspaces_inner(&pool).await?;
        // 工作区会话数：只取 workspace_id 一列聚合，不拉 data 快照
        let rows: Vec<(Option<i64>,)> = sqlx::query_as("SELECT workspace_id FROM sessions")
            .fetch_all(&pool)
            .await?;
        let mut counts: std::collections::HashMap<i64, i64> = std::collections::HashMap::new();
        for (wid,) in rows {
            if let Some(w) = wid {
                *counts.entry(w).or_insert(0) += 1;
            }
        }
        Ok::<_, crate::db::DbError>((groups, workspaces, counts))
    }) {
        Ok((groups, workspaces, counts)) => {
            log::info!(
                "[cef] 分组/工作区列表同步读取：分组 {} 条 / 工作区 {} 条（仅展示字段，不含 data 全量）",
                groups.len(),
                workspaces.len()
            );
            let glist: Vec<serde_json::Value> = groups
                .iter()
                .map(|g| {
                    serde_json::json!({
                        "id": g.id,
                        "name": g.name,
                        "color": g.color,
                        "collapsed": g.collapsed,
                    })
                })
                .collect();
            let wlist: Vec<serde_json::Value> = workspaces
                .iter()
                .map(|w| {
                    serde_json::json!({
                        "id": w.id,
                        "name": w.name,
                        "color": w.color,
                        "sort_order": w.sort_order,
                        "session_count": counts.get(&w.id).copied().unwrap_or(0),
                    })
                })
                .collect();
            serde_json::to_string(&serde_json::json!({ "groups": glist, "workspaces": wlist }))
                .unwrap_or_else(|_| EMPTY_JSON.to_string())
        }
        Err(e) => {
            log::warn!("[cef] 读分组/工作区列表失败: {e}");
            EMPTY_JSON.to_string()
        }
    }
}

/// 解析动作参数 `<id>`（自增 id，≥ 1）
fn parse_id(arg: &str) -> Option<i64> {
    let v = arg.trim().parse::<i64>().ok()?;
    (v > 0).then_some(v)
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        // 面板开合与刷新：直接走 shell 的注册表 setter / 推送
        "groupspanel" => {
            super::shell::set_groups_panel(arg == "1");
            if arg == "1" {
                super::shell::push_groups();
            }
            true
        }
        "groupsrefresh" => {
            super::shell::push_groups();
            true
        }
        "groupdel" => {
            match parse_id(arg) {
                Some(id) => delete_group(id),
                None => log::warn!("[cef] groupdel 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        "spacedel" => {
            match parse_id(arg) {
                Some(id) => delete_workspace(id),
                None => log::warn!("[cef] spacedel 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        _ => false,
    }
}

fn delete_group(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    match tauri::async_runtime::block_on(crate::db::tab_groups::delete_tab_group_inner(
        &db.pool, id,
    )) {
        Ok(()) => {
            log::info!("[cef] 已删除分组 {id}");
            super::shell::push_groups();
        }
        Err(e) => log::warn!("[cef] 删除分组 {id} 失败: {e}"),
    }
}

fn delete_workspace(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    match tauri::async_runtime::block_on(crate::db::workspaces::delete_workspace_inner(
        &db.pool, id,
    )) {
        Ok(()) => {
            log::info!("[cef] 已删除工作区 {id}");
            super::shell::push_groups();
        }
        Err(e) => log::warn!("[cef] 删除工作区 {id} 失败: {e}"),
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
        assert!(!handle("sessdel", "1"));
        assert!(!handle("usdel", "1"));
        assert!(!handle("certdel", "example.com:443"));
        assert!(!handle("nav", "https://example.com"));
    }

    #[test]
    fn own_cmds_swallowed() {
        // groupspanel/groupsrefresh 走 shell setter（APP 未初始化时静默返回，不触 DB）；
        // groupdel/spacedel 参数无效分支吞（warn 日志，不触 DB）
        assert!(handle("groupspanel", "1"));
        assert!(handle("groupspanel", "0"));
        assert!(handle("groupsrefresh", ""));
        assert!(handle("groupdel", "abc"));
        assert!(handle("groupdel", ""));
        assert!(handle("spacedel", "abc"));
        assert!(handle("spacedel", ""));
    }
}

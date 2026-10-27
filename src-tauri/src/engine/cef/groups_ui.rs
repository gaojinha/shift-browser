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
//! 二次确认在页面侧）。删除后刷新留 TODO，协调器接线时补（`push_groups`）。
//!
//! 接线说明（协调器接线时补，本模块不改 shell.rs / toolbar.rs / cef_cmd.rs）：
//! - `shell.rs::handle_action` 在会话分支之后同模式路由：`super::groups_ui::handle(cmd, arg)`；
//! - 面板开合/视图加高：shell.rs 注册表（`enum Panel`）加 `Groups` 变体
//!   （`class()`/`extra_px()`/`atom()` + `Panel::ALL`，**别再造手写清单**），
//!   并加 `set_groups_panel(open)`（同 `set_session_panel` 模式）+ `push_groups()`（同 `push_sessions` 模式）；
//!   接通后删掉本文件 `groupspanel`/`groupsrefresh` TODO 分支里的 warn，改为真实调用，
//!   并接通 `delete_group`/`delete_workspace` 内的删除后刷新 TODO；
//! - 工具栏 HTML 生成处（cef_cmd.rs 的 `sessions_ui::init/sessions_json` 旁）加
//!   `groups_ui::init(app)` + `groups_json(app)` 内嵌；
//! - 工具栏 JS 加 `__shiftGroups` 渲染函数（分组/工作区两个列表 + 删除二次确认，页面侧）。
//!
//! 本模块先交付后接线：协调器接 `shell.rs::handle_action` 与 cef_cmd.rs HTML 内嵌之前
//! （本卡禁改这几文件），整模块暂无业务引用——scoped dead_code 豁免（同
//! `sessions_ui.rs` 交付期），**接线完成后删掉下一行 `#![allow]`**。
#![allow(dead_code)]

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
        let rows: Vec<(Option<i64>,)> =
            sqlx::query_as("SELECT workspace_id FROM sessions").fetch_all(&pool).await?;
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
        // TODO(接线，二狗补)：shell.rs 尚无 set_groups_panel/push_groups（本卡禁改 shell.rs）。
        // 接线时替换为：
        //   "groupspanel" => { super::shell::set_groups_panel(arg == "1"); if arg == "1" { super::shell::push_groups(); } true }
        //   "groupsrefresh" => { super::shell::push_groups(); true }
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
        "groupspanel_old" => {
            log::warn!("[cef] {cmd} 待接线：shell.rs 无 set_groups_panel/push_groups（F-CEF-27 先交付后接线）");
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
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::tab_groups::delete_tab_group_inner(&db.pool, id))
    {
        Ok(()) => {
            log::info!("[cef] 已删除分组 {id}");
            // TODO(接线，二狗补)：删除成功后经 push_groups() 重渲染列表
        }
        Err(e) => log::warn!("[cef] 删除分组 {id} 失败: {e}"),
    }
}

fn delete_workspace(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::workspaces::delete_workspace_inner(&db.pool, id))
    {
        Ok(()) => {
            log::info!("[cef] 已删除工作区 {id}");
            // TODO(接线，二狗补)：删除成功后经 push_groups() 重渲染列表
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
        // groupspanel/groupsrefresh 走 TODO no-op 分支（warn 日志，不触 DB）；
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

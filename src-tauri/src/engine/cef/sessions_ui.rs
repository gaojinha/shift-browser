//! CEF 工具栏的会话面板后端（F-CEF-26）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（`sessions_json` 只给面板展示字段：
//! id/name/created_at/标签数；**不输出** `data` 全量——标签快照可能很大，不进工具栏 HTML）。
//! 删除走 `SHIFT_ACT:sessdel|<id>`（sessions 表自增 id 即唯一标识，二次确认在页面侧）。
//!
//! 接线说明（协调器接线时补，本模块不改 shell.rs / toolbar.rs / cef_cmd.rs）：
//! - `shell.rs::handle_action` 在用户脚本分支之后同模式路由：`super::sessions_ui::handle(cmd, arg)`；
//! - 面板开合/视图加高：shell.rs 注册表（`enum Panel`）加 `Session` 变体
//!   （`class()`/`extra_px()`/`atom()` + `Panel::ALL`，**别再造手写清单**），
//!   并加 `set_session_panel(open)`（同 `set_userscript_panel` 模式）+ `push_sessions()`（同 `push_userscripts` 模式）；
//!   接通后删掉本文件 `sesspanel`/`sessrefresh` 两个 TODO 分支里的 warn，改为真实调用；
//! - `sessdel` 删除成功后经 `push_sessions()` 重渲染列表（下方 `delete_session` 内的 TODO）；
//! - 工具栏 HTML 生成处（cef_cmd.rs 的 `userscripts_ui::init/userscripts_json` 旁）加
//!   `sessions_ui::init(app)` + `sessions_json(app)` 内嵌；
//! - 工具栏 JS 加 `__shiftSessions` 渲染函数（列表 + 标签数 + 删除二次确认，页面侧）；
//! - 恢复会话（restore）：需主应用 TabManager，CEF 侧先不做（卡面明确留 TODO，接线时再评估）。
//!
//! 本模块先交付后接线：协调器接 `shell.rs::handle_action` 与 cef_cmd.rs HTML 内嵌之前
//! （本卡禁改这几文件），整模块暂无业务引用——scoped dead_code 豁免（同
//! `certificates_ui.rs`），**接线完成后删掉下一行 `#![allow]`**。
#![allow(dead_code)]

use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 会话元信息 JSON（id/name/created_at/标签数），供内嵌进工具栏页面。
/// 不输出 `data`（标签快照全量）——可能很大，且面板只需展示标签数。
pub fn sessions_json(app: &AppHandle) -> String {
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return "[]".to_string();
    };
    match tauri::async_runtime::block_on(crate::db::sessions::list_sessions_inner(&db.pool)) {
        Ok(rows) => {
            log::info!("[cef] 会话列表同步读取 {} 条（仅展示字段，不含 data 全量）", rows.len());
            let list: Vec<serde_json::Value> = rows
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id,
                        "name": s.name,
                        "created_at": s.created_at,
                        "tab_count": parse_tab_count(&s.data),
                    })
                })
                .collect();
            serde_json::to_string(&list).unwrap_or_else(|_| "[]".to_string())
        }
        Err(e) => {
            log::warn!("[cef] 读会话列表失败: {e}");
            "[]".to_string()
        }
    }
}

/// 从会话 data 快照（`[{"url":...,"title":...}]`）数标签数。
/// 纯函数：只回计数、不暴露 data 本身（data 可能很大，不进工具栏 HTML）。
/// data 损坏/非数组 → 0。
fn parse_tab_count(data: &str) -> i64 {
    serde_json::from_str::<Vec<serde_json::Value>>(data)
        .map(|v| v.len() as i64)
        .unwrap_or(0)
}

/// 解析动作参数 `<id>`（sessions 表自增 id，≥ 1）
fn parse_id(arg: &str) -> Option<i64> {
    let v = arg.trim().parse::<i64>().ok()?;
    (v > 0).then_some(v)
}

/// 处理工具栏动作。返回 true = 已处理。必须在 GTK 主线程调用（handle_action 本就在主线程）。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        // TODO(接线，二狗补)：shell.rs 尚无 set_session_panel/push_sessions（本卡禁改 shell.rs）。
        // 接线时替换为：
        //   "sesspanel" => { super::shell::set_session_panel(arg == "1"); if arg == "1" { super::shell::push_sessions(); } true }
        //   "sessrefresh" => { super::shell::push_sessions(); true }
        "sesspanel" => {
            super::shell::set_session_panel(arg == "1");
            if arg == "1" {
                super::shell::push_sessions();
            }
            true
        }
        "sessrefresh" => {
            super::shell::push_sessions();
            true
        }
        "sesspanel_old" => {
            log::warn!("[cef] {cmd} 待接线：shell.rs 无 set_session_panel/push_sessions（F-CEF-26 先交付后接线）");
            true
        }
        "sessdel" => {
            match parse_id(arg) {
                Some(id) => delete_session(id),
                None => log::warn!("[cef] sessdel 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        // restore：需主应用 TabManager，CEF 侧先不做（卡面明确留 TODO）
        _ => false,
    }
}

fn delete_session(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    match tauri::async_runtime::block_on(crate::db::sessions::delete_session_inner(&db.pool, id)) {
        Ok(()) => {
            log::info!("[cef] 已删除会话 {id}");
            super::shell::push_sessions();
        }
        Err(e) => log::warn!("[cef] 删除会话 {id} 失败: {e}"),
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
    fn parse_tab_count_forms() {
        assert_eq!(parse_tab_count("[]"), 0);
        assert_eq!(parse_tab_count(r#"[{"url":"https://a.com"},{"url":"https://b.com"}]"#), 2);
        assert_eq!(parse_tab_count(r#"[{"url":"https://a.com","title":"t","groupId":3}]"#), 1);
        // 损坏 / 非数组 → 0
        assert_eq!(parse_tab_count("not json"), 0);
        assert_eq!(parse_tab_count("123"), 0); // 数字不是数组
        assert_eq!(parse_tab_count("{}"), 0); // 对象不是数组
        assert_eq!(parse_tab_count(""), 0);
    }

    #[test]
    fn unknown_cmd_not_handled() {
        // 不吞他模块的命令
        assert!(!handle("certdel", "example.com:443"));
        assert!(!handle("usdel", "1"));
        assert!(!handle("nav", "https://example.com"));
    }

    #[test]
    fn own_cmds_swallowed() {
        // 接线后 sesspanel/sessrefresh 走 shell setter（本阶段是 TODO no-op 分支，APP 未初始化也不触 DB）；
        // sessdel 参数无效分支吞（warn 日志，不触 DB）
        assert!(handle("sesspanel", "1"));
        assert!(handle("sessrefresh", ""));
        assert!(handle("sessdel", "abc"));
        assert!(handle("sessdel", ""));
    }
}

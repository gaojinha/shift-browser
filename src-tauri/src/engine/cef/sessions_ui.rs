//! CEF 工具栏的会话面板后端（F-CEF-26）。
//!
//! 列表数据在生成工具栏 HTML 时内嵌（`sessions_json` 只给面板展示字段：
//! id/name/created_at/标签数；**不输出** `data` 全量——标签快照可能很大，不进工具栏 HTML）。
//! 删除走 `SHIFT_ACT:sessdel|<id>`（sessions 表自增 id 即唯一标识，二次确认在页面侧）。
//!
//! 接线状态（F-CEF-26，2026-10-27 已接线）：`shell.rs::handle_action` 路由
//! `super::sessions_ui::handle`；面板注册走 `Panel::Session`（注册表四处齐）；
//! `set_session_panel` / `push_sessions` 已实现；cef_cmd 已 `init` + HTML 内嵌
//! `sessions_json`；工具栏 JS 有 `__shiftSessions`。
//!
//! 未做：恢复会话（restore）需主应用 TabManager，CEF 侧留待 M2。


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
        // 已接线（F-CEF-26）：面板开合走 shell::set_session_panel，列表推送走 push_sessions。
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
        "sessdel" => {
            match parse_id(arg) {
                Some(id) => delete_session(id),
                None => log::warn!("[cef] sessdel 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        "sessrestore" => {
            match parse_id(arg) {
                Some(id) => restore_session(id),
                None => log::warn!("[cef] sessrestore 参数无效: {arg}（应为 <id>）"),
            };
            true
        }
        _ => false,
    }
}

/// 恢复会话：读出 `data` 里的 URL 列表，逐个开标签（F-CEF-32）。
fn restore_session(id: i64) {
    let Some(app) = APP.get() else { return };
    let Some(db) = app.try_state::<crate::db::Db>() else { return };
    let rows = match tauri::async_runtime::block_on(crate::db::sessions::list_sessions_inner(
        &db.pool,
    )) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[cef] 恢复会话查库失败: {e}");
            return;
        }
    };
    let Some(s) = rows.into_iter().find(|x| x.id == id) else {
        log::warn!("[cef] 恢复会话：id={id} 不存在");
        return;
    };
    let urls: Vec<String> = serde_json::from_str::<Vec<serde_json::Value>>(&s.data)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            v.get("url")
                .and_then(|u| u.as_str())
                .map(|x| x.to_string())
        })
        .collect();
    if urls.is_empty() {
        log::warn!("[cef] 恢复会话 {id}：data 里没有可恢复的 URL");
        return;
    }
    let n = super::shell::open_tabs(&urls);
    log::info!("[cef] 已恢复会话 {id}：{n}/{} 个标签", urls.len());
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

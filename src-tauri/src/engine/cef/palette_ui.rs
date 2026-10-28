//! 命令面板后端（F-CEF-29）。
//!
//! 四组候选：**标签页 / 历史 / 书签 / 命令**，由 `palette_json` 聚合后推给工具栏；
//! `paletteexec` 负责执行选中项（`cmd:<id>` / `tab:<id>` / `url:<url>`）。
//!
//! 设计约束：
//! - 数据来源只用现有 inner 函数（`history::list_recent` / `history::search` /
//!   `bookmarks::get_bookmarks_inner`），不新写 SQL（单一来源）；
//! - 命令清单硬编码在本模块 `COMMANDS`，是全库唯一一份；
//! - 面板开合与撑高走 `shell::set_palette_panel`（注册表 `Panel::Palette`）。

use std::sync::OnceLock;

use tauri::{AppHandle, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// 命令清单：动作 id / 显示名 / 快捷键提示（唯一来源）
const COMMANDS: &[(&str, &str, &str)] = &[
    ("newtab", "新建标签页", "Ctrl+T"),
    ("closetab", "关闭当前标签", "Ctrl+W"),
    ("reopentab", "恢复关闭的标签", "Ctrl+Shift+T"),
    ("find", "页内查找", "Ctrl+F"),
    ("bookmarks", "书签管理器", "Ctrl+Shift+O"),
    ("history", "历史记录", "Ctrl+H"),
    ("downloads", "下载内容", "Ctrl+J"),
    ("settings", "设置", ""),
];

/// JSON 字符串转义（不引入 serde 结构体，字段少、手拼更直观）
fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// 大小写不敏感子串匹配；空查询恒真
fn hit(hay: &str, q: &str) -> bool {
    q.is_empty() || hay.to_lowercase().contains(&q.to_lowercase())
}

/// 命令面板候选 JSON（四组，空结果给空数组不给 null）
pub fn palette_json(app: &AppHandle, query: &str) -> String {
    let q = query.trim();
    // 空查询给「常用」的量，有查询给更多候选
    let cap = if q.is_empty() { 8 } else { 10 };

    // 标签页
    let mut tabs = Vec::new();
    for (id, title, url) in super::shell::tabs_snapshot() {
        if !hit(&title, q) && !hit(&url, q) {
            continue;
        }
        tabs.push(format!(
            "{{\"id\":{},\"title\":{},\"url\":{}}}",
            json_str(&id),
            json_str(&title),
            json_str(&url)
        ));
        if tabs.len() >= cap {
            break;
        }
    }

    // 历史 / 书签
    let mut history = Vec::new();
    let mut bookmarks = Vec::new();
    if let Some(db) = app.try_state::<crate::db::Db>() {
        let rows = if q.is_empty() {
            tauri::async_runtime::block_on(crate::db::history::list_recent(&db.pool, cap as i64))
        } else {
            tauri::async_runtime::block_on(crate::db::history::search(&db.pool, q, cap as i64))
        };
        if let Ok(rows) = rows {
            for r in rows {
                history.push(format!(
                    "{{\"title\":{},\"url\":{}}}",
                    json_str(r.title.as_deref().unwrap_or("")),
                    json_str(&r.url)
                ));
            }
        }

        if let Ok(rows) =
            tauri::async_runtime::block_on(crate::db::bookmarks::get_bookmarks_inner(&db.pool))
        {
            for b in rows {
                if !hit(&b.title, q) && !hit(&b.url, q) {
                    continue;
                }
                bookmarks.push(format!(
                    "{{\"title\":{},\"url\":{}}}",
                    json_str(&b.title),
                    json_str(&b.url)
                ));
                if bookmarks.len() >= cap {
                    break;
                }
            }
        }
    }

    // 命令
    let mut cmds = Vec::new();
    for (id, label, hint) in COMMANDS {
        if !hit(label, q) {
            continue;
        }
        cmds.push(format!(
            "{{\"id\":{},\"label\":{},\"hint\":{}}}",
            json_str(id),
            json_str(label),
            json_str(hint)
        ));
    }

    format!(
        "{{\"tabs\":[{}],\"history\":[{}],\"bookmarks\":[{}],\"commands\":[{}]}}",
        tabs.join(","),
        history.join(","),
        bookmarks.join(","),
        cmds.join(",")
    )
}

/// 处理工具栏动作。返回 true = 已处理。
pub fn handle(cmd: &str, arg: &str) -> bool {
    match cmd {
        "palette" => {
            super::shell::set_palette_panel(arg != "0");
            if arg != "0" {
                // 打开即给一次空查询的候选，输入框不用先敲字
                if let Some(app) = APP.get() {
                    let json = palette_json(app, "");
                    super::shell::push_palette(&json);
                }
            }
            true
        }
        "palettequery" => {
            if let Some(app) = APP.get() {
                let json = palette_json(app, arg);
                super::shell::push_palette(&json);
            }
            true
        }
        "paletteexec" => {
            exec(arg);
            true
        }
        _ => false,
    }
}

/// 执行选中项：`cmd:<id>` / `tab:<id>` / `url:<url>`
fn exec(arg: &str) {
    if let Some(id) = arg.strip_prefix("tab:") {
        if !super::shell::switch_tab_by_id(id) {
            log::warn!("[cef] 命令面板切标签失败：id={id} 不存在");
        }
        return;
    }
    if let Some(url) = arg.strip_prefix("url:") {
        super::shell::navigate(url);
        return;
    }
    let Some(id) = arg.strip_prefix("cmd:") else {
        log::warn!("[cef] paletteexec 参数无效: {arg}");
        return;
    };
    match id {
        "newtab" => super::shell::dispatch_action("tabnew"),
        "closetab" => {
            let idx = super::shell::active_tab_index();
            super::shell::dispatch_action(&format!("tabclose|{idx}"));
        }
        "reopentab" => {
            super::shell::reopen_last_closed();
        }
        "find" => super::shell::open_find_bar_pub(),
        "bookmarks" => super::shell::run_toolbar_js("setBmManage(true)"),
        "history" => super::shell::run_toolbar_js("setHistPanel(true)"),
        "downloads" => super::shell::run_toolbar_js("setDlPanel(true)"),
        "settings" => super::shell::run_toolbar_js("setSettingsPanel(true)"),
        _ => log::warn!("[cef] paletteexec 未知命令: {id}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_is_case_insensitive_and_empty_matches_all() {
        assert!(hit("新建标签页", ""));
        assert!(hit("GitHub", "hub"));
        assert!(hit("GitHub", "HUB"));
        assert!(!hit("GitHub", "gitlab"));
    }

    #[test]
    fn commands_have_unique_ids_and_labels() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|(id, _, _)| *id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "命令 id 有重复");
        assert!(n >= 8, "命令至少 8 条，实际 {n}");
        for (_, label, _) in COMMANDS {
            assert!(!label.is_empty(), "命令显示名不能为空");
        }
    }

    #[test]
    fn json_str_escapes_quotes_and_backslashes() {
        assert_eq!(json_str("a\"b"), "\"a\\\"b\"");
        assert_eq!(json_str("a\\b"), "\"a\\\\b\"");
        assert_eq!(json_str("正常"), "\"正常\"");
    }

    #[test]
    fn empty_query_matches_every_command() {
        let matched = COMMANDS.iter().filter(|(_, l, _)| hit(l, "")).count();
        assert_eq!(matched, COMMANDS.len());
    }
}

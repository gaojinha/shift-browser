//! 本地 API 命令注册表（R1 注册表化，2026-09-24）
//!
//! 历史：`api.rs::handle()` 内一个 700+ 行单体 `match`。现按域拆到本目录各文件，
//! 每个命令是签名统一的 async fn：`async fn(AppHandle, &Value) -> Value`，
//! 在 [`HANDLERS`] 表登记"命令名 → 处理函数"。新增命令两步：
//! 1. 在领域文件写 handler；
//! 2. 在下方 [`HANDLERS`] 表登记，并在 `capabilities.rs` 登记风险等级
//!    （否则本模块防漂移测试失败——只读闸门风险分类与命令表同源）。

mod content;
mod data;
mod page;
mod plugins;
mod system;
mod tabs;
mod tools;

use std::future::Future;
use std::pin::Pin;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// 统一 handler 形态：接收应用句柄与请求 JSON，返回响应 JSON
pub(super) type HandlerFn = for<'a> fn(
    &'a AppHandle,
    &'a Value,
) -> Pin<Box<dyn Future<Output = Value> + Send + 'a>>;

/// 命令分发表（命令名唯一；重复项在单测中断言）
pub(super) const HANDLERS: &[(&str, HandlerFn)] = &[
    // 元命令（能力/技能/插件清单；不经 capabilities 风险表，见 META 白名单）
    ("capabilities/list", |a, r| Box::pin(system::capabilities_list(a, r))),
    ("skills/list", |a, r| Box::pin(system::skills_list(a, r))),
    ("plugins/list", |a, r| Box::pin(plugins::list_plugins(a, r))),
    ("plugins/set_enabled", |a, r| Box::pin(plugins::set_enabled(a, r))),
    // 状态
    ("status", |a, r| Box::pin(system::status(a, r))),
    // 标签与窗口
    ("list_tabs", |a, r| Box::pin(tabs::list_tabs(a, r))),
    ("active_tab", |a, r| Box::pin(tabs::active_tab(a, r))),
    ("open_url", |a, r| Box::pin(tabs::open_url(a, r))),
    ("new_tab", |a, r| Box::pin(tabs::new_tab(a, r))),
    ("close_tab", |a, r| Box::pin(tabs::close_tab(a, r))),
    ("tabs/close_ai", |a, r| Box::pin(tabs::close_ai(a, r))),
    ("switch_tab", |a, r| Box::pin(tabs::switch_tab(a, r))),
    ("reload_tab", |a, r| Box::pin(tabs::reload_tab(a, r))),
    ("navigate", |a, r| Box::pin(tabs::navigate(a, r))),
    ("search", |a, r| Box::pin(tabs::search(a, r))),
    ("pin_tab", |a, r| Box::pin(tabs::pin_tab(a, r))),
    ("mute_tab", |a, r| Box::pin(tabs::mute_tab(a, r))),
    ("minimize_window", |a, r| Box::pin(tabs::minimize_window(a, r))),
    ("maximize_window", |a, r| Box::pin(tabs::maximize_window(a, r))),
    // 页面交互（截图 / JS 执行 / 页面读取）
    ("screenshot", |a, r| Box::pin(page::screenshot(a, r))),
    ("eval", |a, r| Box::pin(page::eval(a, r))),
    ("read_page", |a, r| Box::pin(page::read_page(a, r))),
    ("read_status", |a, r| Box::pin(page::read_status(a, r))),
    ("open_and_read", |a, r| Box::pin(page::open_and_read(a, r))),
    // 内容处理（P1a）：结构化提取 / 阅读模式正文 / 翻译 / 语义快照
    ("extract", |a, r| Box::pin(content::extract(a, r))),
    ("reader/extract", |a, r| Box::pin(content::reader_extract(a, r))),
    ("translate", |a, r| Box::pin(content::translate(a, r))),
    ("content/snapshot", |a, r| Box::pin(content::snapshot(a, r))),
    // MCP 工具网关
    ("tools", |a, r| Box::pin(tools::list_tools(a, r))),
    ("tool_call", |a, r| Box::pin(tools::call_tool(a, r))),
    // 只读数据查询
    ("history", |a, r| Box::pin(data::history(a, r))),
    ("bookmarks", |a, r| Box::pin(data::bookmarks(a, r))),
    ("list_downloads", |a, r| Box::pin(data::list_downloads(a, r))),
    ("list_sessions", |a, r| Box::pin(data::list_sessions(a, r))),
    ("list_groups", |a, r| Box::pin(data::list_groups(a, r))),
    ("list_spaces", |a, r| Box::pin(data::list_spaces(a, r))),
    ("list_userscripts", |a, r| Box::pin(data::list_userscripts(a, r))),
    ("search_history", |a, r| Box::pin(data::search_history(a, r))),
    ("get_setting", |a, r| Box::pin(data::get_setting(a, r))),
    ("list_settings", |a, r| Box::pin(data::list_settings(a, r))),
    // 审计（K/O3）：AI 操作记录查询
    ("audit/reads", |a, r| Box::pin(data::audit_reads(a, r))),
    // 审计（N3）：网络外发端点查询
    ("audit/egress", |a, r| Box::pin(data::audit_egress(a, r))),
];

/// 命令分发：解析后的请求 JSON + cmd 字符串 → 响应 JSON。
/// （api.rs 的 handle() 负责 JSON 解析与只读模式闸门，这里只查表分发）
///
/// 附带中央审计（K 类 / O3 持久化）：所有 API 命令执行后记一条到 `ai_audit`，
/// 使"AI 看了什么"可追溯。只记 命令/url/tab_id/结果码，**不记内容**（N5）。
pub(crate) async fn dispatch(app: &AppHandle, req: &Value, cmd: &str) -> Value {
    let resp = match HANDLERS.iter().find(|(name, _)| *name == cmd) {
        Some((_, handler)) => handler(app, req).await,
        None => AppError::validation(format!("未知命令: {}", cmd)).error_json(),
    };
    record_audit(app, req, cmd, &resp).await;
    resp
}

/// 中央审计（尽力而为）：跳过高频元命令与审计查询自身，避免噪声/自指。
async fn record_audit(app: &AppHandle, req: &Value, cmd: &str, resp: &Value) {
    const SKIP: &[&str] = &["capabilities/list", "skills/list", "tools", "audit/reads", "audit/egress"];
    if SKIP.contains(&cmd) {
        return;
    }
    let Some(db) = app.try_state::<crate::db::Db>() else {
        return;
    };
    let url = req.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let tab_id = req
        .get("tab_id")
        .and_then(|v| v.as_str())
        .or_else(|| req.get("id").and_then(|v| v.as_str()))
        .unwrap_or("");
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let error_code = resp.get("code").and_then(|v| v.as_str()).unwrap_or("");
    crate::db::ai_audit::record(&db.pool, cmd, url, tab_id, ok, error_code).await;
}

#[cfg(test)]
mod tests {
    use super::HANDLERS;
    use crate::capabilities;

    /// 不进 capabilities 风险表、但确属只读安全的元命令
    const META: &[&str] = &["capabilities/list", "skills/list", "plugins/list"];

    #[test]
    fn command_names_unique() {
        let mut names: Vec<&str> = HANDLERS.iter().map(|(n, _)| *n).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "HANDLERS 表存在重复命令名");
    }

    #[test]
    fn every_capability_has_handler() {
        // capabilities 注册表（MCP 清单）中的每个命令都必须能实际分发，
        // 防止"清单里宣称支持、dispatch 却返回未知命令"的漂移
        for name in capabilities::ids() {
            assert!(
                HANDLERS.iter().any(|(n, _)| *n == name),
                "capabilities 已登记命令 {name} 在 HANDLERS 表无 handler"
            );
        }
    }

    #[test]
    fn every_handler_is_risk_classified() {
        // 反向防漂移：每个 handler 必须有只读闸门风险分类
        // （capabilities 表 + extra_risk），或是显式 META 只读元命令，
        // 防止新增写命令忘记登记导致只读模式漏拦
        for (name, _) in HANDLERS {
            let classified = capabilities::risk_of(name).is_some() || META.contains(name);
            assert!(
                classified,
                "handler {name} 未在 capabilities 登记风险等级（读/写），只读模式无法判定"
            );
        }
    }
}

//! 系统/元命令 handler：能力清单、技能清单、运行状态

use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

/// 能力清单（单一来源：capabilities.rs）——MCP Server 启动时拉取，
/// 避免其内部再手写一份 schema（历史问题：25 工具曾三处各写一遍）
pub(super) async fn capabilities_list(_app: &AppHandle, _req: &Value) -> Value {
    json!({ "ok": true, "data": crate::capabilities::list_json() })
}

/// 技能清单（单一来源：contracts::skills，与前端生成物同源；P4）
pub(super) async fn skills_list(_app: &AppHandle, _req: &Value) -> Value {
    json!({ "ok": true, "data": crate::capabilities::skills_list_json() })
}

/// 运行状态：版本 + 当前标签数
pub(super) async fn status(app: &AppHandle, _req: &Value) -> Value {
    let tabs = app
        .try_state::<crate::tabs::TabManager>()
        .map(|s| s.all_tabs().len())
        .unwrap_or(0);
    json!({"ok": true, "data": {"version": env!("CARGO_PKG_VERSION"), "tabs": tabs}})
}

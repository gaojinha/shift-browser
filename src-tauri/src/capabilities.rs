// 能力注册表（工具 / 命令 / 动作的单一事实来源）
//
// 背景：同一批能力原先在 3 处各写一遍——MCP `tool_defs()`（25 个 JSON Schema）、
// 开放 API `match cmd`（30 个分支）、Agent 动作表（9 个）。三者号称"同名同参"，
// 但没有机制保证，新增能力要改 3 处、漏一处就"定义了工具却调不通"。
//
// 本模块把**能力清单**收敛为唯一来源：
//   - MCP Server（独立二进制）通过 `capabilities/list` API 拉取 schema（零编译耦合）
//   - 开放 API / Agent 的动作校验从这里取 id 集合
//
// 见 docs/架构-能力与常量统一治理方案_2026-09-23.md（P3）

use serde_json::{json, Value};

/// 能力风险等级（决定 MCP 是否暴露、API 只读闸门、是否需要人工确认）
///
/// 注：目前仅 `tool_defs`（MCP 视图）在用；API 只读闸门与 Agent 动作校验
/// 将在 P3b 接入本枚举（TODO 已记录），故暂标 allow(dead_code)。
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 只读（低风险）
    Read,
    /// 写操作（受 `api_readonly` 只读闸门约束）
    Write,
    /// Agent 浏览器动作（需人工确认）
    Agent,
}

/// MCP 工具定义（读类全暴露；写类暴露但受只读闸门约束；
/// `eval` / `tool_call` 不暴露——风险高，走浏览器 API 直连）。
///
/// 名称 = 浏览器 API 命令名，参数一一对应；与 `api.rs` 的分发分支保持一致。

fn tool_defs() -> Vec<Value> {
    let t = |name: &str, description: &str, props: Value, required: Vec<&str>| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": props,
                "required": required
            }
        })
    };
    vec![
        // ===== 读类 =====
        t(
            "status",
            "获取浏览器状态：版本号 + 当前标签数。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "list_tabs",
            "列出全部标签页（含 id、标题、URL、是否激活）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "active_tab",
            "获取当前活动标签页。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "history",
            "读取最近浏览历史。",
            json!({
                "limit": { "type": "integer", "description": "条数，默认 50，最大 500" }
            }),
            vec![],
        ),
        t(
            "bookmarks",
            "读取书签列表（含文件夹名）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "search_history",
            "按关键词搜索浏览历史。",
            json!({
                "query": { "type": "string", "description": "搜索关键词（必填）" },
                "limit": { "type": "integer", "description": "条数，默认 50" }
            }),
            vec!["query"],
        ),
        t(
            "list_downloads",
            "列出最近下载记录（最多 100 条）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "list_sessions",
            "列出会话快照（最多 50 条）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "list_groups",
            "列出标签分组。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "list_spaces",
            "列出空间（工作区）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "list_userscripts",
            "列出用户脚本。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "get_setting",
            "读取单个设置项（敏感键返回 null）。",
            json!({
                "key": { "type": "string", "description": "设置键名，如 theme（必填）" }
            }),
            vec!["key"],
        ),
        t(
            "list_settings",
            "列出全部设置（敏感键已过滤）。无参数。",
            json!({}),
            vec![],
        ),
        // ===== 写类 =====
        t(
            "open_url",
            "在新标签页打开网址（浏览器窗口前置）。",
            json!({
                "url": { "type": "string", "description": "目标网址，如 https://example.com（必填）" }
            }),
            vec!["url"],
        ),
        t(
            "new_tab",
            "打开内置新标签页。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "close_tab",
            "关闭指定标签页（返回接管的活动标签）。",
            json!({
                "id": { "type": "string", "description": "标签 id（先 list_tabs 获取，必填）" }
            }),
            vec!["id"],
        ),
        t(
            "switch_tab",
            "切换活动标签页（窗口前置）。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
        ),
        t(
            "reload_tab",
            "刷新指定标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
        ),
        t(
            "navigate",
            "让指定标签页导航到新网址。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" },
                "url": { "type": "string", "description": "目标网址（必填）" }
            }),
            vec!["id", "url"],
        ),
        t(
            "search",
            "用当前默认搜索引擎搜索（新标签打开）。",
            json!({
                "query": { "type": "string", "description": "搜索关键词（必填）" }
            }),
            vec!["query"],
        ),
        t(
            "pin_tab",
            "固定 / 取消固定标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
        ),
        t(
            "mute_tab",
            "静音 / 取消静音标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
        ),
        t(
            "minimize_window",
            "最小化主窗口。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "maximize_window",
            "最大化 / 还原主窗口（返回当前是否最大化）。无参数。",
            json!({}),
            vec![],
        ),
        t(
            "screenshot",
            "截取当前活动标签页可见区域，PNG 落盘，返回文件路径。无参数。",
            json!({}),
            vec![],
        ),
    ]
}

/// 全部能力 id（供 API 校验 / Agent 动作白名单 / 文档生成）
pub fn ids() -> Vec<String> {
    tool_defs()
        .iter()
        .filter_map(|t| t.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect()
}

/// `capabilities/list` API 响应：能力清单 + schema（MCP Server 动态拉取用）
pub fn list_json() -> Value {
    json!({ "tools": tool_defs(), "count": tool_defs().len() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_defs_wellformed() {
        let defs = tool_defs();
        assert_eq!(defs.len(), 25, "工具数应为 25（读 13 + 写 12）");
        for d in &defs {
            assert!(d.get("name").and_then(|v| v.as_str()).is_some(), "缺 name");
            assert!(d.get("description").is_some(), "缺 description");
            let schema = d.get("inputSchema").expect("缺 inputSchema");
            assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
        }
    }

    #[test]
    fn ids_unique() {
        let mut ids = ids();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n, "能力 id 必须唯一");
    }
}

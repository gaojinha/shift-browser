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
/// - `Read`/`Write`：MCP 视图与 API 只读闸门共用本枚举（2026-09-24 接入，
///   消灭 api.rs 手写第二份写命令名单的漂移风险）；
/// - `Agent`：预留给 Agent 动作校验（当前动作白名单在 `agent_actions`，
///   每步已有人工确认，尚未并入本枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Risk {
    /// 只读（低风险）
    Read,
    /// 写操作（受 `api_readonly` 只读闸门约束）
    Write,
    /// Agent 浏览器动作（需人工确认；待接入）
    #[allow(dead_code)]
    Agent,
}

/// 内部能力定义（带风险等级）；MCP JSON 视图由 [`tool_defs`] 投影生成，
/// API 只读闸门从 [`risk_of`] 查询——同一注册表，不再两处手写。
struct ToolDef {
    name: &'static str,
    description: &'static str,
    props: Value,
    required: Vec<&'static str>,
    risk: Risk,
}

/// MCP 工具定义（读类全暴露；写类暴露但受只读闸门约束；
/// `eval` / `tool_call` 不暴露——风险高，走浏览器 API 直连）。
///
/// 名称 = 浏览器 API 命令名，参数一一对应；与 `api.rs` 的分发分支保持一致。
fn registry() -> Vec<ToolDef> {
    let t = |name: &'static str,
             description: &'static str,
             props: Value,
             required: Vec<&'static str>,
             risk: Risk| ToolDef {
        name,
        description,
        props,
        required,
        risk,
    };
    vec![
        // ===== 读类（不改变浏览器状态）=====
        t(
            "status",
            "获取浏览器状态：版本号 + 当前标签数。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "list_tabs",
            "列出全部标签页（含 id、标题、URL、是否激活）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "active_tab",
            "获取当前活动标签页。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "history",
            "读取最近浏览历史。",
            json!({
                "limit": { "type": "integer", "description": "条数，默认 50，最大 500" }
            }),
            vec![],
            Risk::Read,
        ),
        t(
            "bookmarks",
            "读取书签列表（含文件夹名）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "search_history",
            "按关键词搜索浏览历史。",
            json!({
                "query": { "type": "string", "description": "搜索关键词（必填）" },
                "limit": { "type": "integer", "description": "条数，默认 50" }
            }),
            vec!["query"],
            Risk::Read,
        ),
        t(
            "list_downloads",
            "列出最近下载记录（最多 100 条）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "list_sessions",
            "列出会话快照（最多 50 条）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "list_groups",
            "列出标签分组。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "list_spaces",
            "列出空间（工作区）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "list_userscripts",
            "列出用户脚本。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        t(
            "get_setting",
            "读取单个设置项（敏感键返回 null）。",
            json!({
                "key": { "type": "string", "description": "设置键名，如 theme（必填）" }
            }),
            vec!["key"],
            Risk::Read,
        ),
        t(
            "list_settings",
            "列出全部设置（敏感键已过滤）。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
        // ===== 写类（改变浏览器状态，受 api_readonly 闸门约束）=====
        t(
            "open_url",
            "在新标签页打开网址（浏览器窗口前置）。",
            json!({
                "url": { "type": "string", "description": "目标网址，如 https://example.com（必填）" }
            }),
            vec!["url"],
            Risk::Write,
        ),
        t(
            "new_tab",
            "打开内置新标签页。无参数。",
            json!({}),
            vec![],
            Risk::Write,
        ),
        t(
            "close_tab",
            "关闭指定标签页（返回接管的活动标签）。",
            json!({
                "id": { "type": "string", "description": "标签 id（先 list_tabs 获取，必填）" }
            }),
            vec!["id"],
            Risk::Write,
        ),
        t(
            "switch_tab",
            "切换活动标签页（窗口前置）。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
            Risk::Write,
        ),
        t(
            "reload_tab",
            "刷新指定标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
            Risk::Write,
        ),
        t(
            "navigate",
            "让指定标签页导航到新网址。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" },
                "url": { "type": "string", "description": "目标网址（必填）" }
            }),
            vec!["id", "url"],
            Risk::Write,
        ),
        t(
            "search",
            "用当前默认搜索引擎搜索（新标签打开）。",
            json!({
                "query": { "type": "string", "description": "搜索关键词（必填）" }
            }),
            vec!["query"],
            Risk::Write,
        ),
        t(
            "pin_tab",
            "固定 / 取消固定标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
            Risk::Write,
        ),
        t(
            "mute_tab",
            "静音 / 取消静音标签页。",
            json!({
                "id": { "type": "string", "description": "标签 id（必填）" }
            }),
            vec!["id"],
            Risk::Write,
        ),
        t(
            "minimize_window",
            "最小化主窗口。无参数。",
            json!({}),
            vec![],
            Risk::Write,
        ),
        t(
            "maximize_window",
            "最大化 / 还原主窗口（返回当前是否最大化）。无参数。",
            json!({}),
            vec![],
            Risk::Write,
        ),
        // 截图只采集页面像素、不改变任何状态——按 Read 管理（与历史只读闸门行为一致）
        t(
            "screenshot",
            "截取当前活动标签页可见区域，PNG 落盘，返回文件路径。无参数。",
            json!({}),
            vec![],
            Risk::Read,
        ),
    ]
}

/// 仅开放 API 暴露、不进 MCP 清单的高危命令风险等级
/// （`eval`/`tool_call` 注释明确不暴露给 MCP，但 API 闸门必须覆盖）。
fn extra_risk(name: &str) -> Option<Risk> {
    match name {
        "eval" | "tool_call" | "plugins/set_enabled" => Some(Risk::Write),
        _ => None,
    }
}

/// 查询命令风险等级（API 只读闸门唯一来源；未知命令返回 None）
pub fn risk_of(name: &str) -> Option<Risk> {
    registry()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.risk)
        .or_else(|| extra_risk(name))
}

/// 是否为写命令（`api_readonly=true` 时统一拒绝）
pub fn is_write(name: &str) -> bool {
    risk_of(name) == Some(Risk::Write)
}

/// MCP JSON 视图：把内部注册表（含风险等级）投影为工具 schema；
/// risk 是内部安全元数据，不对外暴露。
fn tool_defs() -> Vec<Value> {
    registry()
        .iter()
        .map(|d| {
            json!({
                "name": d.name,
                "description": d.description,
                "inputSchema": {
                    "type": "object",
                    "properties": d.props,
                    "required": d.required
                }
            })
        })
        .collect()
}

/// 全部能力 id（供 API 校验 / Agent 动作白名单 / 文档生成）
pub fn ids() -> Vec<String> {
    registry().iter().map(|d| d.name.to_string()).collect()
}

/// `capabilities/list` API 响应：能力清单 + schema（MCP Server 动态拉取用）
pub fn list_json() -> Value {
    let defs = tool_defs();
    let count = defs.len();
    json!({ "tools": defs, "count": count })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_defs_wellformed() {
        let defs = tool_defs();
        assert_eq!(
            defs.len(),
            25,
            "工具数应为 25（读 14 含 screenshot + 写 11）"
        );
        for d in &defs {
            assert!(d.get("name").and_then(|v| v.as_str()).is_some(), "缺 name");
            assert!(d.get("description").is_some(), "缺 description");
            let schema = d.get("inputSchema").expect("缺 inputSchema");
            assert_eq!(schema.get("type").and_then(|v| v.as_str()), Some("object"));
            // risk 是内部安全元数据，绝不能通过 MCP 视图泄露
            assert!(d.get("risk").is_none(), "MCP 视图不得暴露 risk");
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

    /// 防漂移（P0-2 核心）：注册表 Risk 分类必须与只读闸门判定同源一致。
    /// 历史问题：api.rs 手写第二份写命令名单，新增写命令漏改即静默绕过只读模式。
    #[test]
    fn readonly_gate_matches_registry_risk() {
        for d in registry() {
            assert_eq!(
                is_write(d.name),
                d.risk == Risk::Write,
                "命令 {} 的只读闸门判定与注册表 Risk::{:?} 不一致",
                d.name,
                d.risk
            );
        }
        // 不进 MCP 清单的高危命令也必须被闸门覆盖
        for name in ["eval", "tool_call", "plugins/set_enabled"] {
            assert_eq!(risk_of(name), Some(Risk::Write), "{name} 应为写命令");
            assert!(is_write(name), "{name} 必须被只读闸门拦截");
        }
        // 未知命令不享有写身份（match 分发本身会拒未知命令，这里双保险）
        assert!(!is_write("definitely-not-a-command"));
        assert_eq!(risk_of("definitely-not-a-command"), None);
    }

    #[test]
    fn write_cmd_set_unchanged_snapshot() {
        // 行为基线：与重构前 api.rs 手写名单完全一致（screenshot 历史上不拦截）
        let expected = [
            "open_url",
            "new_tab",
            "close_tab",
            "switch_tab",
            "reload_tab",
            "navigate",
            "search",
            "pin_tab",
            "mute_tab",
            "minimize_window",
            "maximize_window",
            "eval",
            "tool_call",
            "plugins/set_enabled",
        ];
        for name in expected {
            assert!(is_write(name), "基线写命令 {name} 丢失");
        }
        assert!(
            !is_write("screenshot"),
            "screenshot 历史行为为只读，不得收紧"
        );
        assert!(!is_write("status"));
    }
}

/// `skills/list` API 响应（技能数据源：`contracts::skills`，与前端生成物同源）
pub fn skills_list_json() -> serde_json::Value {
    serde_json::json!({
        "skills": crate::contracts::skills::SKILLS.iter().map(|s| serde_json::json!({
            "id": s.id, "name": s.name, "icon": s.icon,
            "desc": s.desc, "uses": s.uses,
        })).collect::<Vec<_>>()
    })
}

#[cfg(test)]
mod skills_tests {
    use crate::contracts::skills;

    #[test]
    fn skills_all_matches_skills() {
        // 生成用 ALL 与 SKILLS 必须一致（防两边漂移）
        assert_eq!(skills::SKILLS.len(), skills::ALL.len());
        for (s, (id, name, icon, _)) in skills::SKILLS.iter().zip(skills::ALL.iter()) {
            assert_eq!(s.id, *id);
            assert_eq!(s.name, *name);
            assert_eq!(s.icon, *icon);
        }
    }
}

/// 插件（可插拔能力扩展单元）：四种形态统一为同一视图。
///
/// 背景：四类扩展各管各的——用户脚本/样式（`userscripts` 表）、内容过滤（广告拦截）、
/// Web 扩展（编译期 `.so`）、MCP 外部服务器（`mcp_servers` 表），
/// 生命周期/启停/审计各一套。这里提供统一视图（`plugins/list`），
/// 让设置页能一屏看全、后续统一启停与审计（见治理方案 P5）。
pub mod plugins {
    /// 插件形态
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PluginKind {
        /// 用户脚本 / 样式（注入页面）
        UserScript,
        /// 内容过滤规则（广告拦截）
        ContentFilter,
        /// Web 扩展（网络层拦截，编译期 .so）
        WebExtension,
        /// MCP 外部工具服务器
        McpServer,
    }

    impl PluginKind {
        /// 机器可读标识
        pub fn as_str(&self) -> &'static str {
            match self {
                PluginKind::UserScript => "userscript",
                PluginKind::ContentFilter => "content_filter",
                PluginKind::WebExtension => "web_extension",
                PluginKind::McpServer => "mcp_server",
            }
        }

        /// 中文标签（前端展示）
        pub fn label(&self) -> &'static str {
            match self {
                PluginKind::UserScript => "用户脚本",
                PluginKind::ContentFilter => "内容过滤",
                PluginKind::WebExtension => "Web 扩展",
                PluginKind::McpServer => "MCP 服务器",
            }
        }

        /// 是否支持运行时启停（Web 扩展为编译期加载，暂不支持）
        pub fn togglable(&self) -> bool {
            !matches!(self, PluginKind::WebExtension)
        }
    }

    /// 统一插件条目
    pub fn entry(
        kind: PluginKind,
        id: &str,
        name: &str,
        enabled: bool,
        detail: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "kind": kind.as_str(),
            "kindLabel": kind.label(),
            "togglable": kind.togglable(),
            "id": id,
            "name": name,
            "enabled": enabled,
            "detail": detail,
        })
    }
}

#[cfg(test)]
mod plugins_tests {
    use super::plugins::{entry, PluginKind};

    #[test]
    fn plugin_kinds_have_labels() {
        for k in [
            PluginKind::UserScript,
            PluginKind::ContentFilter,
            PluginKind::WebExtension,
            PluginKind::McpServer,
        ] {
            assert!(!k.as_str().is_empty() && !k.label().is_empty());
        }
        assert!(!PluginKind::WebExtension.togglable());
        assert!(PluginKind::UserScript.togglable());
    }

    #[test]
    fn plugin_entry_shape() {
        let e = entry(
            PluginKind::UserScript,
            "1",
            "测试脚本",
            true,
            "匹配 example.com",
        );
        assert_eq!(e["kind"], "userscript");
        assert_eq!(e["enabled"], true);
        assert_eq!(e["togglable"], true);
    }
}

/// Agent 浏览器动作（AI 执行层的动作白名单，单一来源）。
///
/// 与 `tool_defs()`（MCP/API 工具）区别：动作是 AI 在**当前标签页**上的原子操作，
/// 每步需人工确认（见 `engine/agent.rs`）。这里集中声明，
/// 供 function calling 的 `enum` 生成、白名单校验、以及后续的 AI 能力自述。
pub mod agent_actions {
    /// 一个动作
    pub struct Action {
        pub id: &'static str,
        pub desc: &'static str,
        /// 必填参数（供 Agent 校验 / 提示词生成）
        pub required: &'static [&'static str],
    }

    /// 全部动作（唯一来源）
    pub const ACTIONS: &[Action] = &[
        Action {
            id: "click",
            desc: "点击页面元素",
            required: &["selector"],
        },
        Action {
            id: "fill",
            desc: "填写输入框",
            required: &["selector", "value"],
        },
        Action {
            id: "scroll",
            desc: "滚动页面",
            required: &["direction"],
        },
        Action {
            id: "read",
            desc: "读取页面文本",
            required: &[],
        },
        Action {
            id: "wait",
            desc: "等待若干毫秒",
            required: &["ms"],
        },
        Action {
            id: "navigate",
            desc: "跳转到指定 URL",
            required: &["url"],
        },
        Action {
            id: "back",
            desc: "后退一页",
            required: &[],
        },
        Action {
            id: "snapshot",
            desc: "采集页面语义快照",
            required: &[],
        },
        Action {
            id: "submit",
            desc: "提交表单",
            required: &["selector"],
        },
    ];

    /// 动作 id 数组（供 function calling 的 `enum` / 白名单校验直接用）
    pub const IDS: [&str; 9] = [
        "click", "fill", "scroll", "read", "wait", "navigate", "back", "snapshot", "submit",
    ];
}

#[cfg(test)]
mod agent_actions_tests {
    use super::agent_actions;

    #[test]
    fn ids_match_actions() {
        // IDS 与 ACTIONS 必须一致（防两处漂移）
        assert_eq!(agent_actions::IDS.len(), agent_actions::ACTIONS.len());
        for (id, a) in agent_actions::IDS.iter().zip(agent_actions::ACTIONS.iter()) {
            assert_eq!(id, &a.id);
            assert!(!a.desc.is_empty());
        }
    }
}

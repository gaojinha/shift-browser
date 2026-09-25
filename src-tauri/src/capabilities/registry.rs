//! 能力注册表（工具 / 命令的单一事实来源，R1 拆分 · 2026-09-25）
//!
//! 背景：同一批能力原先在 3 处各写一遍——MCP `tool_defs()`（25 个 JSON Schema）、
//! 开放 API `match cmd`（30 个分支）、Agent 动作表（9 个）。三者号称"同名同参"，
//! 但没有机制保证，新增能力要改 3 处、漏一处就"定义了工具却调不通"。
//!
//! 本模块把**能力清单**收敛为唯一来源：
//!   - MCP Server（独立二进制）通过 `capabilities/list` API 拉取 schema（零编译耦合）
//!   - 开放 API / Agent 的动作校验从这里取 id 集合
//!
//! 见 docs/架构-能力与常量统一治理方案_2026-09-23.md（P3）

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
      "tabs/close_ai",
      "关闭所有 AI 标签（origin=ai）——只回收开放 API 打开的标签，绝不碰用户标签。AI 读完后清理现场用。",
      json!({}),
      vec![],
      Risk::Write,
    ),
    // 审计查询（K/O3）：读 AI 操作记录（只含 url/命令/结果码，无内容）。Read。
    t(
      "audit/reads",
      "查询最近 AI 读取/操作审计（命令 + url + 结果码，不含页面内容）。参数：limit（默认 50，硬顶 500）。只读模式下放行。",
      json!({
        "limit": { "type": "integer", "description": "返回条数（默认 50，硬顶 500）" }
      }),
      vec![],
      Risk::Read,
    ),
    // 网络外发审计（N3）：读本应用及其子进程的 TCP 外联端点（只读 /proc、零出网）。Read。
    t(
      "audit/egress",
      "网络外发审计：列出本应用及其全部子进程当前的 TCP 外联端点（IP:端口/状态），用于核对“浏览器到底连了哪儿”。只读 /proc、零出网。只读模式下放行。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    // ===== 内容处理（P1a）：均不改变浏览器状态，按 Read 管理 =====
    t(
      "extract",
      "按 CSS 选择器做结构化提取（固定模板，非任意 JS）。参数：selector（必填）、fields（默认 [\"text\"]，可选 text/href/src/alt/title/data-*）、tab_id（可选，缺省活动标签）、limit（默认 500，硬顶 5000）。返回匹配元素列表 + total/truncated。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填，querySelectorAll）" },
        "fields": { "type": "array", "items": { "type": "string" }, "description": "要提取的字段白名单（默认 [\"text\"]）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选，缺省活动标签）" },
        "limit": { "type": "integer", "description": "最多返回条数（默认 500，硬顶 5000）" }
      }),
      vec!["selector"],
      Risk::Read,
    ),
    t(
      "reader/extract",
      "给 URL 抓取并净化正文（阅读模式提取，readability）。参数：url（必填）。与 open_and_read 的 article 模式区别：本命令独立于标签，直接给 URL 出正文。",
      json!({
        "url": { "type": "string", "description": "文章 URL（必填，http/https）" }
      }),
      vec!["url"],
      Risk::Read,
    ),
    t(
      "translate",
      "翻译文本（复用本地翻译链路）。参数：text（必填）、target（可选，默认 zh-CN）。",
      json!({
        "text": { "type": "string", "description": "待翻译文本（必填）" },
        "target": { "type": "string", "description": "目标语言（可选，默认 zh-CN）" }
      }),
      vec!["text"],
      Risk::Read,
    ),
    t(
      "content/snapshot",
      "读取活动标签的语义快照（标题/表单/链接等结构化摘要；本地执行、不出网）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
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
      "截取标签页可见区域，PNG 落盘，返回文件路径。可选 id（标签 id）；缺省为活动标签。",
      json!({
        "id": { "type": "string", "description": "标签 id（可选；缺省为活动标签，先 list_tabs 获取）" }
      }),
      vec![],
      Risk::Read,
    ),
    // 页面读取状态：读活动标签的 url/title/加载态/脱敏元信息（复用 page_read.js 骨架采集，
    // 不碰 DOM 值、不返回内容），纯读——按 Read 管理；api_readonly 下天然放行（P2 只读语义落地）。
    t(
      "read_status",
      "读取活动标签页状态与元信息（url/title/最终 URL/加载态/脱敏提示 hints）。纯读、不返回页面内容，只读模式（api_readonly=true）下放行。",
      json!({
        "id": { "type": "string", "description": "标签 id（可选；缺省为活动标签）" }
      }),
      vec![],
      Risk::Read,
    ),
    // 打开并读取（P0 杀招命令）：一次完成「开标签 → 等加载 → 读页面」。
    // 语义是"看页面"（白名单 scheme + 内网 SSRF 守卫 + 固定采集脚本），登记 Read——
    // api_readonly 下放行（P2 核心拍板：「能看不能动」，拒绝的是写/交互/清理，不是看）。
    t(
      "open_and_read",
      "打开 URL 并读取页面（一次完成 开标签→等加载→读取）。参数：url（必填，http/https）、mode/selector/max_chars/max_items（同 read_page）、close（读完是否关闭新标签，默认 false）。返回 tab_id/final_url/title/ready_state/content_type/read。内网/私有地址默认拒绝（api_allowed_ips 白名单可放行）。",
      json!({
        "url": { "type": "string", "description": "目标网址（必填，仅 http/https）" },
        "mode": { "type": "string", "enum": ["text", "elements", "links", "images", "tables", "forms", "article", "html"], "description": "读取模式（默认 text）" },
        "selector": { "type": "string", "description": "CSS 选择器限定范围（可选）" },
        "max_chars": { "type": "integer", "description": "文本/HTML 字符上限（默认 200000，硬顶 2000000）" },
        "max_items": { "type": "integer", "description": "列表类模式条数上限（默认 500，硬顶 5000）" },
        "close": { "type": "boolean", "description": "读完是否关闭新标签（默认 false）" },
        "reuse": { "type": "boolean", "description": "复用同 URL 的已有 AI 标签（不新开，默认 false）" },
        "ua": { "type": "string", "enum": ["desktop", "mobile"], "description": "读取 UA：mobile 切移动版 UA（默认 desktop）" }
      }),
      vec!["url"],
      Risk::Read,
    ),
    // 页面读取：固定采集脚本（非任意 JS），8 种模式读活 DOM，不改变浏览器状态——按 Read 管理。
    // 脱敏规则见 page_read.js（password/hidden value 不返回、非 http(s) 图片 URL 只留前缀、html 模式去注释/script/style）
    t(
      "read_page",
      "读取活动标签页内容（8 种模式：text 可读文本 / elements 结构化元素 / links 链接 / images 图片 / tables 表格 / forms 表单控件（不含 value）/ article 正文 / html 脱敏后 HTML）。返回统一带 url/title/truncated/total 元信息。",
      json!({
        "mode": { "type": "string", "enum": ["text", "elements", "links", "images", "tables", "forms", "article", "html"], "description": "读取模式（默认 text）" },
        "selector": { "type": "string", "description": "CSS 选择器限定范围（可选；非法选择器返回错误；≤1024 字符）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选；缺省活动标签。open_and_read 返回的 tab_id 可直接复用，无需先切标签）" },
        "max_chars": { "type": "integer", "description": "文本/HTML 字符上限（默认 200000，硬顶 2000000）" },
        "max_items": { "type": "integer", "description": "列表类模式条数上限（默认 500，硬顶 5000）" }
      }),
      vec![],
      Risk::Read,
    ),
  ]
}

/// 仅开放 API 暴露、不进 MCP 清单的命令风险等级（API 闸门与防漂移测试必须覆盖）：
/// - 写：`eval`/`tool_call`（注释明确不暴露给 MCP）、`plugins/set_enabled`；
/// - 读：`tools`（列 MCP 工具清单，无状态变更）。
fn extra_risk(name: &str) -> Option<Risk> {
  match name {
    "eval" | "tool_call" | "plugins/set_enabled" => Some(Risk::Write),
    "tools" => Some(Risk::Read),
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
#[allow(dead_code)] // 目前经 tests 与 list_json 间接覆盖；保留为公开校验入口
pub fn ids() -> Vec<String> {
  registry().iter().map(|d| d.name.to_string()).collect()
}

/// `capabilities/list` API 响应：能力清单 + schema（MCP Server 动态拉取用）
pub fn list_json() -> Value {
  let defs = tool_defs();
  let count = defs.len();
  json!({ "tools": defs, "count": count })
}

// 注册表防漂移测试随被测面外置（避免本文件超长）
#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;

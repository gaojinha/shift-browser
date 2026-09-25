//! 能力注册表 · 读类条目（R1 三次拆分 · 2026-09-25）
//! 不改变浏览器状态的能力：状态/数据查询、内容读取与提取、审计查询。

use serde_json::json;

use super::core::{tool, Risk, ToolDef};

pub(super) fn defs() -> Vec<ToolDef> {
  vec![
    // ===== 状态与数据查询（不改变浏览器状态）=====
    tool(
      "status",
      "获取浏览器状态：版本号 + 当前标签数。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "list_tabs",
      "列出全部标签页（含 id、标题、URL、是否激活）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "active_tab",
      "获取当前活动标签页。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "history",
      "读取最近浏览历史。",
      json!({
        "limit": { "type": "integer", "description": "条数，默认 50，最大 500" }
      }),
      vec![],
      Risk::Read,
    ),
    tool(
      "bookmarks",
      "读取书签列表（含文件夹名）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "search_history",
      "按关键词搜索浏览历史。",
      json!({
        "query": { "type": "string", "description": "搜索关键词（必填）" },
        "limit": { "type": "integer", "description": "条数，默认 50" }
      }),
      vec!["query"],
      Risk::Read,
    ),
    tool(
      "list_downloads",
      "列出最近下载记录（最多 100 条）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "list_sessions",
      "列出会话快照（最多 50 条）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "list_groups",
      "列出标签分组。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "list_spaces",
      "列出空间（工作区）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "list_userscripts",
      "列出用户脚本。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    tool(
      "get_setting",
      "读取单个设置项（敏感键返回 null）。",
      json!({
        "key": { "type": "string", "description": "设置键名，如 theme（必填）" }
      }),
      vec!["key"],
      Risk::Read,
    ),
    tool(
      "list_settings",
      "列出全部设置（敏感键已过滤）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    // ===== 页面交互（P2）：等待条件不改变状态，按读管理（其余 action/* 在写表）=====
    tool(
      "action/wait_for",
      "等待条件满足（元素出现/消失、页面加载完成）。参数：condition（appear/gone/load）、selector（appear/gone 必填）、tab_id（可选）、timeout（毫秒，默认 10000，硬顶 60000）。超时返回 met:false。",
      json!({
        "condition": { "type": "string", "enum": ["appear", "gone", "load"], "description": "等待条件（默认 appear）" },
        "selector": { "type": "string", "description": "CSS 选择器（appear/gone 必填）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" },
        "timeout": { "type": "integer", "description": "超时毫秒（默认 10000，硬顶 60000）" }
      }),
      vec![],
      Risk::Read,
    ),
    // 审计查询（K/O3）：读 AI 操作记录（只含 url/命令/结果码，无内容）。Read。
    tool(
      "audit/reads",
      "查询最近 AI 读取/操作审计（命令 + url + 结果码，不含页面内容）。参数：limit（默认 50，硬顶 500）。只读模式下放行。",
      json!({
        "limit": { "type": "integer", "description": "返回条数（默认 50，硬顶 500）" }
      }),
      vec![],
      Risk::Read,
    ),
    // 网络外发审计（N3）：读本应用及其子进程的 TCP 外联端点（只读 /proc、零出网）。Read。
    tool(
      "audit/egress",
      "网络外发审计：列出本应用及其全部子进程当前的 TCP 外联端点（IP:端口/状态），用于核对“浏览器到底连了哪儿”。只读 /proc、零出网。只读模式下放行。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    // ===== 内容处理（P1a）：均不改变浏览器状态，按 Read 管理 =====
    tool(
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
    tool(
      "reader/extract",
      "给 URL 抓取并净化正文（阅读模式提取，readability）。参数：url（必填）。与 open_and_read 的 article 模式区别：本命令独立于标签，直接给 URL 出正文。",
      json!({
        "url": { "type": "string", "description": "文章 URL（必填，http/https）" }
      }),
      vec!["url"],
      Risk::Read,
    ),
    tool(
      "translate",
      "翻译文本（复用本地翻译链路）。参数：text（必填）、target（可选，默认 zh-CN）。",
      json!({
        "text": { "type": "string", "description": "待翻译文本（必填）" },
        "target": { "type": "string", "description": "目标语言（可选，默认 zh-CN）" }
      }),
      vec!["text"],
      Risk::Read,
    ),
    tool(
      "content/snapshot",
      "读取活动标签的语义快照（标题/表单/链接等结构化摘要；本地执行、不出网）。无参数。",
      json!({}),
      vec![],
      Risk::Read,
    ),
    // 截图只采集页面像素、不改变任何状态——按 Read 管理（与历史只读闸门行为一致）
    tool(
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
    tool(
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
    tool(
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
    tool(
      "read_page",
      "读取活动标签页内容（8 种模式：text 可读文本 / elements 结构化元素 / links 链接 / images 图片 / tables 表格 / forms 表单控件（不含 value）/ article 正文 / html 脱敏后 HTML）。返回统一带 url/title/truncated/total 元信息。",
      json!({
        "mode": { "type": "string", "enum": ["text", "elements", "links", "images", "tables", "forms", "article", "html"], "description": "读取模式（默认 text）" },
        "selector": { "type": "string", "description": "CSS 选择器限定范围（可选；非法选择器返回错误；≤1024 字符）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选；缺省活动标签。open_and_read 返回的 tab_id 可直接复用，无需先切标签）" },
        "expand": { "type": "boolean", "description": "读前展开原生 <details>（默认 false）" },
        "shadow": { "type": "boolean", "description": "尽力穿透开放 shadow root（默认 false）" },
        "iframe": { "type": "boolean", "description": "并入同源 iframe 文本、跨源给提示（默认 false）" },
        "lazy": { "type": "boolean", "description": "读前先滚动到底触发懒加载（默认 false）" },
        "max_chars": { "type": "integer", "description": "文本/HTML 字符上限（默认 200000，硬顶 2000000）" },
        "max_items": { "type": "integer", "description": "列表类模式条数上限（默认 500，硬顶 5000）" }
      }),
      vec![],
      Risk::Read,
    ),
  ]
}

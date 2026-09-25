//! 能力注册表 · 写类条目（R1 三次拆分 · 2026-09-25）
//! 改变浏览器状态的能力：标签开闭/导航、界面交互（action/*）、窗口控制。
//! 全部受 `api_readonly` 只读闸门约束。

use serde_json::json;

use super::core::{tool, Risk, ToolDef};

pub(super) fn defs() -> Vec<ToolDef> {
  vec![
    // ===== 标签开闭 / 导航（改变浏览器状态，受 api_readonly 闸门约束）=====
    tool(
      "open_url",
      "在新标签页打开网址（浏览器窗口前置）。",
      json!({
        "url": { "type": "string", "description": "目标网址，如 https://example.com（必填）" }
      }),
      vec!["url"],
      Risk::Write,
    ),
    tool(
      "new_tab",
      "打开内置新标签页。无参数。",
      json!({}),
      vec![],
      Risk::Write,
    ),
    tool(
      "close_tab",
      "关闭指定标签页（返回接管的活动标签）。",
      json!({
        "id": { "type": "string", "description": "标签 id（先 list_tabs 获取，必填）" }
      }),
      vec!["id"],
      Risk::Write,
    ),
    tool(
      "tabs/close_ai",
      "关闭所有 AI 标签（origin=ai）——只回收开放 API 打开的标签，绝不碰用户标签。AI 读完后清理现场用。",
      json!({}),
      vec![],
      Risk::Write,
    ),
    tool(
      "action/dismiss",
      "关闭常见弹窗（Cookie 同意/遮罩浮层）。保守识别（浮层内 close 语义或白名单文案），识别不了返回提示、绝不盲点。属交互，受只读闸门约束。参数：tab_id（可选）。",
      json!({
        "tab_id": { "type": "string", "description": "目标标签 id（可选，缺省活动标签）" }
      }),
      vec![],
      Risk::Write,
    ),
    // ===== 页面交互（P2）：受 api_agent_actions 开关约束（默认关）=====
    tool(
      "action/scroll",
      "滚动页面。参数：direction（up/down/top/bottom，默认 down）、amount（像素，默认 600）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "direction": { "type": "string", "enum": ["up", "down", "top", "bottom"], "description": "方向（默认 down）" },
        "amount": { "type": "integer", "description": "滚动像素（默认 600）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec![],
      Risk::Write,
    ),
    tool(
      "action/click",
      "点击元素（预检 not_found/not_visible/disabled 后 click）。参数：selector（必填）、index（默认 0）、tab_id（可选）。返回 navigated/url_after。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["selector"],
      Risk::Write,
    ),
    tool(
      "action/fill",
      "填写表单（预检后设 value 并触发 input/change；值只落字符串、不执行）。参数：selector（必填）、value、index（默认 0）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填）" },
        "value": { "type": "string", "description": "填入的字符串（不执行）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["selector"],
      Risk::Write,
    ),
    tool(
      "action/back",
      "浏览器后退（history.back）。参数：tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec![],
      Risk::Write,
    ),
    tool(
      "action/hover",
      "悬停元素（派发 mouseover/mouseenter/mousemove）——用于 hover 展开的菜单。参数：selector（必填）、index（默认 0）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["selector"],
      Risk::Write,
    ),
    tool(
      "action/key",
      "派发键盘事件（如 Enter/Escape/Tab/ArrowDown）。参数：key（必填）、selector（可选，先聚焦命中元素，缺省作用于当前焦点）、index、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "key": { "type": "string", "description": "键名（如 Enter/Escape/Tab/ArrowDown）" },
        "selector": { "type": "string", "description": "先聚焦的元素（可选）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["key"],
      Risk::Write,
    ),
    // ===== 标签与窗口控制 =====
    tool(
      "switch_tab",
      "切换活动标签页（窗口前置）。",
      json!({
        "id": { "type": "string", "description": "标签 id（必填）" }
      }),
      vec!["id"],
      Risk::Write,
    ),
    tool(
      "reload_tab",
      "刷新指定标签页。",
      json!({
        "id": { "type": "string", "description": "标签 id（必填）" }
      }),
      vec!["id"],
      Risk::Write,
    ),
    tool(
      "navigate",
      "让指定标签页导航到新网址。",
      json!({
        "id": { "type": "string", "description": "标签 id（必填）" },
        "url": { "type": "string", "description": "目标网址（必填）" }
      }),
      vec!["id", "url"],
      Risk::Write,
    ),
    tool(
      "search",
      "用当前默认搜索引擎搜索（新标签打开）。",
      json!({
        "query": { "type": "string", "description": "搜索关键词（必填）" }
      }),
      vec!["query"],
      Risk::Write,
    ),
    tool(
      "pin_tab",
      "固定 / 取消固定标签页。",
      json!({
        "id": { "type": "string", "description": "标签 id（必填）" }
      }),
      vec!["id"],
      Risk::Write,
    ),
    tool(
      "mute_tab",
      "静音 / 取消静音标签页。",
      json!({
        "id": { "type": "string", "description": "标签 id（必填）" }
      }),
      vec!["id"],
      Risk::Write,
    ),
    tool(
      "minimize_window",
      "最小化主窗口。无参数。",
      json!({}),
      vec![],
      Risk::Write,
    ),
    tool(
      "maximize_window",
      "最大化 / 还原主窗口（返回当前是否最大化）。无参数。",
      json!({}),
      vec![],
      Risk::Write,
    ),
  ]
}

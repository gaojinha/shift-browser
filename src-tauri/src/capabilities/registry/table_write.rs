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
      "点击元素（预检 not_found/not_visible/disabled 后 click）。参数：selector（必填）、index（默认 0）、pierce（穿透开放 shadow root，默认 false）、frames（同源 iframe 下钻路径数组：数字=第 N 个，字符串=选择器，≤4 层；跨源返回 frame_cross_origin:true 不谎报）、tab_id（可选）。返回 navigated/url_after。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填；可喂 action/find 返回的稳定 selector）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "pierce": { "type": "boolean", "description": "穿透开放 shadow root（默认 false）" },
        "frames": { "type": "array", "description": "同源 iframe 下钻路径（数组：数字=第 N 个，字符串=选择器）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["selector"],
      Risk::Write,
    ),
    tool(
      "action/fill",
      "填写表单（预检后设 value 并触发 input/change；值只落字符串、不执行）。参数：selector（必填）、value、index（默认 0）、pierce、frames（同 action/click）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填）" },
        "value": { "type": "string", "description": "填入的字符串（不执行）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "pierce": { "type": "boolean", "description": "穿透开放 shadow root（默认 false）" },
        "frames": { "type": "array", "description": "同源 iframe 下钻路径（数组：数字=第 N 个，字符串=选择器）" },
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
      "悬停元素（派发 mouseover/mouseenter/mousemove）——用于 hover 展开的菜单。参数：selector（必填）、index（默认 0）、pierce、frames（同 action/click）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "selector": { "type": "string", "description": "CSS 选择器（必填）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "pierce": { "type": "boolean", "description": "穿透开放 shadow root（默认 false）" },
        "frames": { "type": "array", "description": "同源 iframe 下钻路径（数组：数字=第 N 个，字符串=选择器）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["selector"],
      Risk::Write,
    ),
    tool(
      "action/key",
      "派发键盘事件（如 Enter/Escape/Tab/ArrowDown）。参数：key（必填）、modifiers（组合键数组，仅 Control/Shift/Alt/Meta；只对页面 JS 监听器有效，对浏览器原生快捷键无效）、selector（可选，先聚焦命中元素，缺省作用于当前焦点）、index、pierce、frames（同 action/click）、tab_id（可选）。受 api_agent_actions 开关约束。",
      json!({
        "key": { "type": "string", "description": "键名（如 Enter/Escape/Tab/ArrowDown）" },
        "modifiers": { "type": "array", "items": { "type": "string", "enum": ["Control", "Shift", "Alt", "Meta"] }, "description": "组合键（如 [\"Control\"]）" },
        "selector": { "type": "string", "description": "先聚焦的元素（可选）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "pierce": { "type": "boolean", "description": "穿透开放 shadow root（默认 false）" },
        "frames": { "type": "array", "description": "同源 iframe 下钻路径（数组：数字=第 N 个，字符串=选择器）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec!["key"],
      Risk::Write,
    ),
    tool(
      "action/type",
      "逐字符输入文本（纯字符串，不执行）。可选 selector（+index/pierce/frames）：直接定位输入元素，聚焦与输入同步完成（推荐，不受跨调用焦点丢失影响）；不传 selector 时作用于当前焦点（先 action/click 聚焦）。input/textarea 用原生 value setter + InputEvent（Vue/React 受控组件正确更新）；contenteditable 用 Selection/Range 插入。参数：text（必填；或 clear:true 单独清空）、clear（先清空，默认 false）、tab_id（可选）。密码框拒绝（password_refused）。受 api_agent_actions 开关约束。",
      json!({
        "text": { "type": "string", "description": "要输入的文本（必填；纯字符串，不执行）" },
        "selector": { "type": "string", "description": "目标输入元素 CSS 选择器（可选；不传则作用于当前焦点）" },
        "index": { "type": "integer", "description": "第几个匹配元素（默认 0）" },
        "pierce": { "type": "boolean", "description": "穿透开放 shadow root（默认 false）" },
        "frames": { "type": "array", "description": "同源 iframe 下钻路径（数组：数字=第 N 个，字符串=选择器）" },
        "clear": { "type": "boolean", "description": "输入前清空当前内容（默认 false）" },
        "tab_id": { "type": "string", "description": "目标标签 id（可选）" }
      }),
      vec![],
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
    // 整页汉化 · 写回/还原（P3b）：**改别人的页面**，故归 Write、受只读闸门约束。
    // apply  按收集阶段打在节点上的 __sbId 写回译文（原文存在节点上）
    // restore 从备份恢复原文
    // 安全：只改「当前文本仍等于收集时原文」的节点；页面自己更新过的（轮播、
    // 虚拟列表）跳过不覆盖（计入 mismatched），避免拿译文盖掉新内容。
    tool(
      "page_apply_translation",
      "把译文写回当前标签页面（mode=apply）或还原原文（mode=restore）。会真实修改页面 DOM，故归写类。参数：tab_id（可选，缺省活动标签）、mode（apply|restore，必填）、payload（apply 时必填 {pairs:[{id,orig,translated}], attrs:[{attr,orig,translated}]}；restore 时传 null）。返回 applied（实际改动处数）、mismatched（页面已变动而跳过的处数）、restored（restore 时的还原数）。限制：翻译质量由模型决定，写回前会逐条核对原文；核对不上就跳过，不会拿译文盖掉页面新内容。",
      json!({
        "tab_id": { "type": "string", "description": "目标标签 id（可选，缺省活动标签）" },
        "mode": { "type": "string", "description": "apply 写回译文 / restore 还原原文" },
        "payload": { "type": "object", "description": "apply 时为 {pairs,attrs}；restore 时为 null" }
      }),
      vec!["mode"],
      Risk::Write,
    ),
  ]
}

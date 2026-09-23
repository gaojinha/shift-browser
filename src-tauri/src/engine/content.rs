//! 页面理解层（阶段 1.1 · Content Layer）
//!
//! 目标：浏览器从"AI 能对话"升级为"AI 能理解页面"。
//! 语义快照 = 从活动标签的实时 DOM 提取结构化信息（不是截图）：
//! 标题 / URL / 正文摘要 / 可交互元素（链接·按钮·输入框·表单）——供 AI 理解与后续执行层调用。
//!
//! 隐私分级（阶段 1.1）：快照提取**全程本地**（webview 内 JS 执行），
//! 不抓 URL、不出网；是否把快照发给 AI 由前端在用户授权下决定。
//!
//! 复用：正文提取（content/extract）走 reader.rs 的 curl+readability 管线（独立命令）。

use ts_rs::TS;
use serde::{Deserialize, Serialize};
use crate::engine::webmcp::WebmcpToolDef;

/// 结构体：无障碍树语义元素（阶段 1.5 试验：对齐 Lighthouse Agentic Browsing
/// "代理为中心的无障碍"信号——每个可交互元素要有**程序化名称**（aria-label）与
/// **有效角色**（role），Agent 靠它们语义定位，不再只靠标签文本猜）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct AriaElement {
    /// 元素角色（role 属性或推导角色，如 button/link/checkbox/combobox）
    pub role: String,
    /// 程序化名称：aria-label > aria-labelledby 解析文本 > 可见文本
    pub name: String,
    /// 标签名（button/input/a/…）用于定位回查
    pub tag: String,
    /// 是否隐藏（aria-hidden="true"）—— Agent 应忽略隐藏元素
    pub hidden: bool,
}

/// 语义快照（页面理解层的核心产出）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct Snapshot {
    /// 页面标题
    pub title: String,
    /// 页面 URL
    pub url: String,
    /// 页面语言（lang 属性，可空）
    pub lang: Option<String>,
    /// 正文可见文本（去空白压缩，最多取前 N 字符）
    pub text: String,
    /// 页面可交互元素统计
    pub interactive: InteractiveStats,
    /// 表单结构（name/type/placeholder，最多列 M 个）
    pub forms: Vec<FormField>,
    /// 页面里可见的链接（href + 文本，最多列 K 个）
    pub links: Vec<LinkInfo>,
    /// WebMCP 工具声明（阶段 1.5 polyfill 捕获，最多 10 个）
    #[serde(default)]
    pub webmcp_tools: Vec<WebmcpToolDef>,
    /// 无障碍树语义元素（阶段 1.5：aria-label/role，最多 30 个，AI Agent 语义定位用）
    #[serde(default)]
    pub aria: Vec<AriaElement>,
}

/// AI 友好度报告（阶段 1.7：对齐 Lighthouse Agentic Browsing 四信号）
#[derive(Debug, Serialize, Deserialize)]
pub struct AiFriendlinessReport {
    /// 综合评分（0-100）
    pub overall: f32,
    /// 无障碍树评分（0-100）：有程序化名称的 aria 元素占比
    pub aria_score: f32,
    /// 表单可访问性评分（0-100）：有 label 的表单占比
    pub forms_score: f32,
    /// 改进建议（评分低时给出）
    pub suggestions: Vec<String>,
}

/// 计算页面对 AI 的友好度（基于语义快照的无障碍树 + 表单可访问性）。
/// 评分逻辑：
///   aria_score = 有 name 的 aria 元素数 / 总 aria 元素数（忽略 hidden）
///   forms_score = 有 label 的表单数 / 总表单数
///   overall = aria_score * 0.6 + forms_score * 0.4
pub fn ai_friendliness(snap: &Snapshot) -> AiFriendlinessReport {
    // 无障碍树评分
    let aria_total = snap.aria.len();
    let aria_named = snap.aria.iter().filter(|a| !a.hidden && !a.name.is_empty()).count();
    let aria_score = if aria_total > 0 {
        (aria_named as f32 / aria_total as f32) * 100.0
    } else {
        0.0
    };

    // 表单可访问性评分：表单有 name 或 label 字段
    let forms_total = snap.forms.len();
    let forms_labeled = snap.forms.iter()
        .filter(|f| !f.name.is_empty() || !f.placeholder.is_empty())
        .count();
    let forms_score = if forms_total > 0 {
        (forms_labeled as f32 / forms_total as f32) * 100.0
    } else {
        0.0
    };

    let overall = aria_score * 0.6 + forms_score * 0.4;

    // 改进建议
    let mut suggestions = Vec::new();
    if aria_score < 80.0 {
        suggestions.push(format!(
            "无障碍树覆盖不足（{:.0}%）：为可交互元素添加 aria-label",
            aria_score
        ));
    }
    if forms_score < 80.0 {
        suggestions.push(format!(
            "表单可访问性不足（{:.0}%）：为表单字段添加 label 或 aria-label",
            forms_score
        ));
    }
    if suggestions.is_empty() {
        suggestions.push("页面 AI 友好度良好".to_string());
    }

    AiFriendlinessReport {
        overall,
        aria_score,
        forms_score,
        suggestions,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct InteractiveStats {
    pub links: usize,
    pub buttons: usize,
    pub inputs: usize,
    pub textareas: usize,
    pub selects: usize,
    pub iframes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct FormField {
    pub name: String,
    pub field_type: String,
    pub placeholder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/")]
pub struct LinkInfo {
    pub text: String,
    pub href: String,
}

/// 正文可见文本最大长度（字符）。
/// 注：阶段 1.1 前端接入前暂未使用，保留为公共 API 契约。
#[allow(dead_code)]
const MAX_TEXT_CHARS: usize = 4000;
/// 表单字段最大收集数（待前端接入，见 MAX_TEXT_CHARS）
#[allow(dead_code)]
const MAX_FORM_FIELDS: usize = 20;
/// 链接最大收集数（待前端接入，见 MAX_TEXT_CHARS）
#[allow(dead_code)]
const MAX_LINKS: usize = 30;

/// DOM 提取脚本（在 webview 内执行，返回 JSON 字符串）。
/// 注意：这是字符串嵌入的 JS，**不能**用 Rust 的 {} 插值（会与 JS 模板串冲突），
/// 用 const 拼接（与 content_filter.rs 的 JSON 转义教训一致）。
const SNAPSHOT_JS: &str = r#"
(function() {
  var MAX_TEXT = 4000, MAX_FORMS = 20, MAX_LINKS = 30;
  var doc = document;
  var title = doc.title || '';
  var url = location.href || '';
  var lang = doc.documentElement ? doc.documentElement.getAttribute('lang') : null;
  // 正文可见文本：取 body innerText，压缩空白
  var text = '';
  if (doc.body) {
    text = (doc.body.innerText || '').replace(/\s+/g, ' ').trim();
    if (text.length > MAX_TEXT) text = text.slice(0, MAX_TEXT) + '…';
  }
  // 可交互元素统计
  var links = doc.querySelectorAll('a[href]').length;
  var buttons = doc.querySelectorAll('button, [role="button"], input[type="button"], input[type="submit"]').length;
  var inputs = doc.querySelectorAll('input:not([type="hidden"]):not([type="button"]):not([type="submit"])').length;
  var textareas = doc.querySelectorAll('textarea').length;
  var selects = doc.querySelectorAll('select').length;
  var iframes = doc.querySelectorAll('iframe').length;
  // 表单字段
  var forms = [];
  var formEls = doc.querySelectorAll('input:not([type="hidden"]), textarea, select');
  for (var i = 0; i < formEls.length && forms.length < MAX_FORMS; i++) {
    var f = formEls[i];
    forms.push({
      name: f.name || f.id || '',
      field_type: f.type || f.tagName.toLowerCase(),
      placeholder: f.placeholder || ''
    });
  }
  // 链接（可见文本优先）
  var linkInfo = [];
  var linkEls = doc.querySelectorAll('a[href]');
  for (var j = 0; j < linkEls.length && linkInfo.length < MAX_LINKS; j++) {
    var a = linkEls[j];
    var t = (a.innerText || a.textContent || '').replace(/\s+/g, ' ').trim();
    if (t.length > 80) t = t.slice(0, 80) + '…';
    linkInfo.push({ text: t, href: a.href });
  }
  // WebMCP 工具声明（polyfill 捕获，最多列 10 个）
  var webmcpTools = [];
  try {
    var wt = window.__webmcp_tools || [];
    for (var w = 0; w < wt.length && webmcpTools.length < 10; w++) {
      webmcpTools.push({
        name: wt[w].name || '',
        description: wt[w].description || '',
        parameters: wt[w].parameters || null
      });
    }
  } catch (e) {}
  // 无障碍语义信号（对齐 Lighthouse Agentic 信号②「代理为中心无障碍」：
  // 程序化名称 aria-label/labelledby、有效角色，以及隐藏态——供 AI 判断元素可操作性）
  var ariaList = [];
  var ariaSel = '[aria-label],[aria-labelledby],[aria-hidden="true"],[role]:not([role="presentation"]):not([role="none"])';
  var ariaEls = doc.querySelectorAll(ariaSel);
  for (var ar = 0; ar < ariaEls.length && ariaList.length < 30; ar++) {
    var ae = ariaEls[ar];
    var label = (ae.getAttribute('aria-label') || '').trim();
    if (ae.getAttribute('aria-labelledby')) {
      var lbNode = doc.getElementById(ae.getAttribute('aria-labelledby'));
      if (lbNode) label = (lbNode.innerText || lbNode.textContent || '').replace(/\s+/g, ' ').trim();
    }
    if (!label) continue; // 无名称的 aria 元素对 Agent 无用，跳过（骨架信号）
    ariaList.push({
      role: ae.getAttribute('role') || ae.tagName.toLowerCase(),
      label: label.slice(0, 60),
      tag: ae.tagName.toLowerCase(),
      hidden: ae.getAttribute('aria-hidden') === 'true'
    });
  }
  return JSON.stringify({
    title: title, url: url, lang: lang, text: text,
    interactive: { links: links, buttons: buttons, inputs: inputs, textareas: textareas, selects: selects, iframes: iframes },
    forms: forms, links: linkInfo, webmcp_tools: webmcpTools, aria: ariaList
  });
})()
"#;

/// 从 webview 提取语义快照（本地执行，不出网）。
/// **只发起、不等待**：结果经 `tx` 异步送回（evaluate 回调在主线程执行）。
/// 调用方必须在 with_webview 闭包外等待 channel——绝不能在闭包内 recv_timeout
/// （会阻塞主线程，evaluate 回调永远不执行，死锁 5 秒卡死 UI）。
pub fn extract_snapshot_async(
    wv: &webkit2gtk::WebView,
    tx: std::sync::mpsc::Sender<Result<Snapshot, String>>,
) {
    crate::engine::webview::eval_js_with(wv, SNAPSHOT_JS, move |res| {
        let result = match res {
            Ok(json) => serde_json::from_str(&json).map_err(|e| format!("快照解析失败: {}", e)),
            Err(e) => Err(format!("快照 JS 执行失败: {}", e)),
        };
        let _ = tx.send(result);
    });
}

/// 从页面文本生成简短摘要（供 AI 上下文使用，本地规则，不调模型）。
/// 取正文前 300 字符，若更长则截断加省略号。
/// 注：阶段 1.1 前端接入前暂未使用，保留为公共 API 契约。
#[allow(dead_code)]
pub fn summarize_text(snapshot: &Snapshot) -> String {
    let t = snapshot.text.trim();
    if t.is_empty() {
        return "(页面无可读文本)".to_string();
    }
    let mut out = format!("【{}】{}", snapshot.title, t);
    if out.chars().count() > 500 {
        let truncated: String = out.chars().take(500).collect();
        out = format!("{}…", truncated);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_js_is_valid_json_producer() {
        // 基本结构检查：脚本必须返回 JSON.stringify 结果
        assert!(SNAPSHOT_JS.contains("JSON.stringify"));
        assert!(SNAPSHOT_JS.contains("interactive"));
        assert!(SNAPSHOT_JS.contains("forms"));
    }

    #[test]
    fn summarize_truncates_long_text() {
        let snap = Snapshot {
            title: "测试".to_string(),
            url: "https://example.com".to_string(),
            lang: None,
            text: "字".repeat(1000),
            interactive: InteractiveStats::default(),
            forms: vec![],
            links: vec![],
            webmcp_tools: vec![],
            aria: vec![],
        };
        let s = summarize_text(&snap);
        assert!(s.chars().count() <= 510);
        assert!(s.contains("测试"));
    }
}

//! WebMCP polyfill 试验（阶段 1.5 · 标准跟踪）
//!
//! 背景：WebMCP（Web Model Context Protocol）是 Google 提出的标准提案，网站用
//! `navigator.modelContext.registerTool(def)` 向 AI 智能体暴露结构化工具。
//! 2026-09 时点处于 Chrome 149–156 公开 origin trial；**WebKitGTK 2.52.3 不支持**
//! （引擎版本冻结，见 docs/standard-tracking-2026-09-20.md 差距分析）。
//!
//! 本模块 = **客户端 polyfill 试验**：在页面脚本执行前（document_start）注入
//! 最小 `registerTool` 捕获实现，把网站暴露的 WebMCP 工具声明收集到
//! `window.__webmcp_tools`，供语义快照（content_snapshot）附带给 AI。
//!
//! 安全边界（试验特性，默认关闭，设置项 `webmcp_polyfill`）：
//! - 只实现 `registerTool`（捕获声明），**不伪造任何执行能力**
//! - 其他 modelContext API 不实现（页面调用会得到 undefined，与不支持时行为一致）
//! - 捕获的工具声明只存内存（window 变量），不落盘、不出网
//! - 幂等：重复注入无害

/// polyfill 注入脚本（document_start）。幂等：已安装则直接返回。
pub const WEBMCP_POLYFILL_JS: &str = r#"
(() => {
  if (window.__webmcpPolyfilled) return;
  window.__webmcpPolyfilled = true;
  window.__webmcp_tools = [];
  try {
    if (!window.navigator.modelContext) {
      window.navigator.modelContext = {
        registerTool: function (def) {
          try {
            var entry = {
              name: (def && def.name) || '',
              description: (def && def.description) || '',
              parameters: (def && def.parameters) || null
            };
            window.__webmcp_tools.push(entry);
          } catch (e) {}
          return Promise.resolve();
        }
      };
    }
  } catch (e) {}
})();
"#;

/// WebMCP 工具声明（快照附带给 AI）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct WebmcpToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyfill_is_idempotent_and_only_captures() {
        // 幂等标志 + 只捕获不执行
        assert!(WEBMCP_POLYFILL_JS.contains("__webmcpPolyfilled"));
        assert!(WEBMCP_POLYFILL_JS.contains("registerTool"));
        // 不伪造执行能力：无 executeTool / callTool 之类
        assert!(!WEBMCP_POLYFILL_JS.contains("executeTool"));
        assert!(!WEBMCP_POLYFILL_JS.contains("callTool"));
    }

}
//! 插件（可插拔能力扩展单元）：四种形态统一为同一视图（R1 拆分 · 2026-09-25）。
//!
//! 背景：四类扩展各管各的——用户脚本/样式（`userscripts` 表）、内容过滤（广告拦截）、
//! Web 扩展（编译期 `.so`）、MCP 外部服务器（`mcp_servers` 表），
//! 生命周期/启停/审计各一套。这里提供统一视图（`plugins/list`），
//! 让设置页能一屏看全、后续统一启停与审计（见治理方案 P5）。

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn plugin_entry_passes_through_all_fields() {
        // 透传字段逐一锁死：settings 页一屏看全依赖 kind/kindLabel/id/name/detail 全到位
        let e = entry(PluginKind::McpServer, "srv1", "文件服务器", false, "stdio");
        assert_eq!(e["kind"], "mcp_server");
        assert_eq!(e["kindLabel"], "MCP 服务器");
        assert_eq!(e["togglable"], true, "MCP 服务器支持运行时启停（仅 WebExtension 不可）");
        assert_eq!(e["id"], "srv1");
        assert_eq!(e["name"], "文件服务器");
        assert_eq!(e["enabled"], false);
        assert_eq!(e["detail"], "stdio");
        // 其余形态的机器标识与启停能力
        assert_eq!(PluginKind::ContentFilter.as_str(), "content_filter");
        assert_eq!(PluginKind::WebExtension.as_str(), "web_extension");
        assert!(PluginKind::ContentFilter.togglable());
        assert!(PluginKind::McpServer.togglable());
        assert!(!PluginKind::WebExtension.togglable(), "Web 扩展编译期加载，不可运行时启停");
    }
}

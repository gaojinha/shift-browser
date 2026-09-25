//! 快捷访问站点 URL 安全校验与规范化（R1 拆分 · 2026-09-25）
//!
//! 新标签页是带 IPC 桥（invoke_key）的特权页面，javascript:/data: 等 scheme
//! 一旦渲染成 <a href>，点击即在特权上下文执行任意命令，必须拒绝。

/// 校验是否为可安全渲染的网页 URL（http/https）
/// 新标签页是特权页面（带 IPC 桥），javascript:/data: 等 scheme 一旦渲染成
/// <a href> 点击即可在特权上下文执行任意命令，必须拒绝
pub(super) fn is_safe_web_url(url: &str) -> bool {
  let lower = url.trim().to_ascii_lowercase();
  lower.starts_with("http://") || lower.starts_with("https://")
}

/// 规范化快捷站点 URL：显式危险 scheme 拒绝；无 scheme 自动补 https://
/// （用户常输入 "example.com"，原版行为是补全协议）
pub(super) fn normalize_quick_site_url(url: &str) -> Result<String, String> {
  let u = url.trim();
  if u.is_empty() {
    return Err("快捷站点 URL 不能为空".to_string());
  }
  let lower = u.to_ascii_lowercase();
  for bad in ["javascript:", "data:", "file:", "vbscript:", "blob:"] {
    if lower.starts_with(bad) {
      return Err(format!("快捷站点不允许 {} 协议", bad));
    }
  }
  if lower.starts_with("http://") || lower.starts_with("https://") {
    Ok(u.to_string())
  } else {
    Ok(format!("https://{}", u))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalize_accepts_http_https_and_bare_domain() {
    assert_eq!(
      normalize_quick_site_url("https://example.com").unwrap(),
      "https://example.com"
    );
    assert_eq!(
      normalize_quick_site_url("http://example.com").unwrap(),
      "http://example.com"
    );
    // 无 scheme 自动补 https://
    assert_eq!(
      normalize_quick_site_url("example.com").unwrap(),
      "https://example.com"
    );
    // 大小写容忍
    assert_eq!(
      normalize_quick_site_url("HTTPS://example.com").unwrap(),
      "HTTPS://example.com"
    );
    // 首尾空白裁剪
    assert_eq!(
      normalize_quick_site_url("  example.com  ").unwrap(),
      "https://example.com"
    );
  }

  #[test]
  fn normalize_rejects_dangerous_schemes() {
    for bad in [
      "javascript:alert(1)",
      "data:text/html,<script>alert(1)</script>",
      "file:///etc/passwd",
      "vbscript:msgbox(1)",
      "blob:https://example.com/uuid",
    ] {
      assert!(normalize_quick_site_url(bad).is_err(), "应拒绝: {}", bad);
    }
    // 大小写变体也拒绝
    assert!(normalize_quick_site_url("JaVaScRiPt:alert(1)").is_err());
  }

  #[test]
  fn normalize_rejects_empty() {
    assert!(normalize_quick_site_url("").is_err());
    assert!(normalize_quick_site_url("   ").is_err());
  }

  #[test]
  fn is_safe_web_url_filters_schemes() {
    assert!(is_safe_web_url("https://example.com"));
    assert!(is_safe_web_url("http://example.com"));
    assert!(!is_safe_web_url("javascript:alert(1)"));
    assert!(!is_safe_web_url("file:///etc/passwd"));
    assert!(!is_safe_web_url("data:text/html,x"));
    assert!(!is_safe_web_url("example.com")); // 无 scheme 不算安全（渲染前必须规范化）
  }
}

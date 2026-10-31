//! CEF 页面注入脚本的**纯函数构建**（从 `browser.rs` 的 `inject_marker` 拆出）。
//!
//! 为什么要单独成文件：注入脚本是 F-CEF-03/F-CEF-08 的核心产物，
//! 但 `inject_marker` 需要真实的 `Frame` 句柄、无法单测。
//! 把"脚本长什么样"与"把脚本塞进 frame"分开后：
//! - 脚本内容可单测（品牌表拼接、扩展脚本顺序、空扩展不产生多余空行）；
//! - `inject_marker` 只剩三行调度逻辑。

/// 构建 document_start 注入脚本。
///
/// 顺序有语义，不许调换：
/// 1. `__SHIFT_INJECT_OK__` 链路标记；
/// 2. `__SHIFT_UA_BRANDS__` 品牌表（`privacy.js` 与 `userAgentData` 覆盖都读它）；
/// 3. `navigator.userAgentData` 覆盖（真 Chrome 的 brands 含 Google Chrome，内核原生只有两项）；
/// 4. 扩展/隐私/广告拦截脚本（`extra`，F-CEF-11 预计算，放最后以便读到品牌表）。
pub fn build_inject_script(brands: &str, extra: Option<&str>) -> String {
    let script = format!(
        r#"window.__SHIFT_INJECT_OK__ = true;
window.__SHIFT_UA_BRANDS__ = {brands};
(function () {{
  try {{
    var uad = navigator.userAgentData;
    if (!uad) return;
    var b = window.__SHIFT_UA_BRANDS__;
    Object.defineProperty(navigator, 'userAgentData', {{
      get: function () {{
        return {{
          brands: b || uad.brands,
          mobile: false,
          platform: 'Linux',
          getHighEntropyValues: function () {{
            return Promise.resolve({{ platform: 'Linux', platformVersion: '', architecture: '', model: '', uaFullVersion: '' }});
          }},
          toJSON: function () {{ return {{ brands: b || uad.brands, mobile: false, platform: 'Linux' }}; }}
        }};
      }},
      configurable: true
    }});
  }} catch (e) {{}}
}})();
"#
    );
    match extra {
        Some(e) if !e.trim().is_empty() => format!("{script}\n{e}"),
        _ => script,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BRANDS: &str =
        r#"[{"brand":"Chromium","version":"154"},{"brand":"Google Chrome","version":"154"}]"#;

    #[test]
    fn script_contains_marker_and_brands() {
        let s = build_inject_script(BRANDS, None);
        assert!(s.contains("window.__SHIFT_INJECT_OK__ = true;"));
        assert!(s.contains("window.__SHIFT_UA_BRANDS__ = ["));
        assert!(s.contains("Google Chrome"));
        assert!(s.contains("Object.defineProperty(navigator, 'userAgentData'"));
    }

    #[test]
    fn script_is_valid_js_shape() {
        let s = build_inject_script(BRANDS, None);
        // 括号配平是"没写坏"的最便宜证据（不做真 JS 解析）
        let open = s.matches('{').count();
        let close = s.matches('}').count();
        assert_eq!(open, close, "花括号不配平，注入脚本会语法错误");
        assert!(s.trim_end().ends_with("})();"));
    }

    #[test]
    fn extra_script_appended_after_brands() {
        let s = build_inject_script(BRANDS, Some("window.__EXT__ = 1;"));
        let brand_pos = s.find("__SHIFT_UA_BRANDS__").expect("品牌表缺失");
        let extra_pos = s.find("window.__EXT__").expect("扩展脚本缺失");
        assert!(
            extra_pos > brand_pos,
            "扩展脚本必须在品牌表之后（privacy.js 要读 __SHIFT_UA_BRANDS__）"
        );
    }

    #[test]
    fn empty_extra_does_not_add_blank_tail() {
        let with_none = build_inject_script(BRANDS, None);
        let with_empty = build_inject_script(BRANDS, Some("   "));
        assert_eq!(with_none, with_empty);
    }
}

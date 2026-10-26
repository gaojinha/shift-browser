//! CEF 页面的扩展/隐私/广告拦截注入（F-CEF-11）
//!
//! WebKit 路径靠 UserContentManager 常驻脚本；CEF 的 `on_load_start` 是同步回调、不能 await，
//! 因此启动时 `block_on` 预计算整段 JS 存进 static，回调里只取缓存执行。
//! 改设置（广告拦截/隐私档位/用户脚本等）需重开窗口才生效，CEF 仍是实验阶段，不做热更新。

use super::*;
use std::sync::Mutex;

static CEF_INJECT_JS: Mutex<Option<String>> = Mutex::new(None);

/// 取预计算的注入脚本（未就绪返回 None）
pub fn cached_inject_js() -> Option<String> {
    CEF_INJECT_JS.lock().ok().and_then(|g| g.clone())
}

/// 每段脚本单独 try 包裹：一段抛错不影响后面的段落
fn guarded(js: &str) -> String {
    format!("try {{\n{js}\n}} catch (e) {{}}\n")
}

/// CSS 只能经 JS 注入；document_start 时 head 可能还没有，挂到 documentElement
fn css_injector(css: &str) -> String {
    let lit = serde_json::to_string(css).unwrap_or_else(|_| "\"\"".to_string());
    guarded(&format!(
        "var __st = document.createElement('style'); __st.textContent = {lit}; \
         (document.head || document.documentElement).appendChild(__st);"
    ))
}

/// 用户脚本：WebKit 的 allow_list 是换行分隔的通配模式，这里在页面里按 location.href 匹配
fn userscript_injector(code: &str, patterns: &[String]) -> String {
    let pats = serde_json::to_string(patterns).unwrap_or_else(|_| "[]".to_string());
    let matcher = format!(
        "var __p = {pats}; var __ok = __p.length === 0 || __p.some(function (g) {{ \
         if (g === '*' || g === '<all_urls>') return true; \
         try {{ return new RegExp('^' + g.replace(/[.+?^${{}}()|[\\]\\\\]/g, '\\\\$&').replace(/\\*/g, '.*') + '$').test(location.href); }} catch (e) {{ return false; }} }});"
    );
    guarded(&format!("{matcher}\nif (__ok) {{\n{code}\n}}"))
}

/// 启动时调用一次：读设置、拼好整段注入脚本并缓存。
/// 顺序：前缀变量 → 隐私 → 广告拦截 → compat → WebMCP → 用户脚本。
/// 前缀变量（__SHIFT_PRIVACY_WHITELIST__ 等）由 `collect_enabled_items` 里的 `wl_prefix`
/// 拼进隐私项开头，这里不重写；隐私关闭时页面也就没有这些变量，与 WebKit 路径一致。
pub async fn build_and_cache(db: &Db) -> Result<usize, String> {
    crate::db::userscripts::refresh_cache(&db.pool).await;
    let (items, webmcp_on) = adblock::collect_enabled_items(db).await?;
    let compat_on = compat_enabled(db).await;

    // collect_enabled_items 的顺序是 扩展 → 广告拦截 → 隐私；按约定顺序重排：
    // 隐私项（含前缀）排最前，其余保持原序
    let (mut privacy, mut rest): (Vec<_>, Vec<_>) = items.into_iter().partition(|(js, _)| {
        js.as_deref()
            .map(|s| s.contains("__SHIFT_PRIVACY_WHITELIST__"))
            .unwrap_or(false)
    });
    privacy.append(&mut rest);

    let mut out = String::new();
    for (js, css) in &privacy {
        if let Some(js) = js {
            out.push_str(&guarded(js));
        }
        if let Some(css) = css {
            out.push_str(&css_injector(css));
        }
    }
    if compat_on {
        out.push_str(&guarded(COMPAT_CHROME_JS));
    }
    if webmcp_on {
        out.push_str(&guarded(crate::engine::webmcp::WEBMCP_POLYFILL_JS));
    }
    let scripts = crate::db::userscripts::cached_scripts();
    for (code, patterns) in &scripts {
        out.push_str(&userscript_injector(code, patterns));
    }

    log::info!(
        "[cef-inject] 预计算完成：{} 个注入项, compat={}, webmcp={}, 用户脚本 {} 个, 共 {} 字节",
        privacy.len(),
        compat_on,
        webmcp_on,
        scripts.len(),
        out.len()
    );
    let n = out.len();
    if let Ok(mut g) = CEF_INJECT_JS.lock() {
        *g = Some(out);
    }
    Ok(n)
}

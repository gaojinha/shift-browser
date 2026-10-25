//! 广告拦截（P1-3 拆分 · 2026-09-22）
//!
//! 内置 CSS 规则 + 关键词 + JS 模板 + 构建函数。原在 extensions.rs。

use super::*;

/// 反指纹会话 key：进程启动时随机生成一次（同会话内固定，重启即换）。
/// privacy.js 据此派生种子 = f(会话 key, origin)：同会话同源稳定、跨会话变化。
static SESSION_KEY: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn session_key() -> &'static str {
    SESSION_KEY.get_or_init(|| uuid::Uuid::new_v4().simple().to_string())
}

/// 内置广告拦截 CSS 规则（精简版，常见广告选择器）
/// 设置 adblock_enabled=true 时自动注入，优先级低于用户扩展（扩展后注入）
const ADBLOCK_CSS: &str = include_str!("../../resources/js/adblock.css");

/// 内置关键词（用户自定义关键词会追加在后面）
const BUILTIN_KEYWORDS: &[&str] = &[
    "关闭广告",
    "关闭推广",
    "收起广告",
    "不再显示",
    "跳过广告",
    "跳过推广",
    "头条报",
    // 2026-09-18 移除宽泛词「推广」：邮件正文含"产品推广"等字样时整块被隐藏，
    // 表现为"邮件正文空白"。文本关键词只保留明确的广告关闭按钮文案。
    "赞助",
    "下载APP",
    "下载 App",
    "下载客户端",
    "广告投放",
    "广告合作",
    "广告位",
    "广告服务",
];

/// 广告拦截 JS 模板。`__SB_KEYWORDS__` 在注入前替换成完整数组（内置 + 用户自定义）
const ADBLOCK_JS_TEMPLATE: &str = include_str!("../../resources/js/adblock.js");

fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

/// 拼装广告拦截 JS：内置关键词 + 用户自定义关键词
pub fn build_adblock_js(custom: &[String]) -> String {
    let mut all: Vec<String> = BUILTIN_KEYWORDS.iter().map(|s| (*s).to_string()).collect();
    for k in custom {
        let t = k.trim();
        if t.is_empty() {
            continue;
        }
        if !all.iter().any(|x| x == t) {
            all.push(t.to_string());
        }
    }
    let arr = all
        .iter()
        .map(|k| format!("'{}'", js_escape(k)))
        .collect::<Vec<_>>()
        .join(", ");
    ADBLOCK_JS_TEMPLATE.replace("__SB_KEYWORDS__", &format!("[{}]", arr))
}

pub async fn load_custom_keywords(db: &Db) -> Vec<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = 'adblock_keywords'")
            .fetch_optional(&db.pool)
            .await
            .ok()
            .flatten();
    match row {
        Some((v,)) => serde_json::from_str(&v).unwrap_or_default(),
        None => Vec::new(),
    }
}

/// 读取所有启用扩展的注入内容 + 广告拦截（如开启）
pub async fn collect_enabled_items(
    db: &Db,
) -> Result<(Vec<(Option<String>, Option<String>)>, bool), String> {
    let exts = ext_db::list(db).await.map_err(|e| e.to_string())?;
    let mut items = Vec::new();
    for ext in exts {
        if !ext.enabled {
            continue;
        }
        match read_extension_dir(&ext.path) {
            Ok((_, _, _, js, css, _, _)) => items.push((js, css)),
            Err(e) => log::warn!("[extensions] 读取扩展 {} 失败: {}", ext.name, e),
        }
    }
    // 注入相关设置一次查询拿全（2026-09-19 优化：原先 adblock/privacy 三档/白名单/
    // 自定义关键词/翻译超时 6 次串行查询合并为 1 次）
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT key, value FROM settings WHERE key IN \
         ('adblock_enabled','privacy_shield_level','privacy_shield','privacy_whitelist',\
          'adblock_keywords','translate_timeout','privacy_fp_mode','gpc_header')",
    )
    .fetch_all(&db.pool)
    .await
    .map_err(|e| e.to_string())?;
    let mut kv: std::collections::HashMap<String, String> = rows.into_iter().collect();
    let mut take = |k: &str| kv.remove(k);

    // 广告拦截（内置系统注入，追加在用户扩展后面）
    let adblock_on = take("adblock_enabled").map(|v| v == "true").unwrap_or(true); // 默认开启
                                                                                   // 同步给引擎层内容过滤器（网络层拦截）：开关变化时立即生效
    crate::engine::content_filter::set_enabled(adblock_on);
    if adblock_on {
        let custom: Vec<String> = take("adblock_keywords")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let js = build_adblock_js(&custom);
        log::info!(
            "[extensions] 广告拦截已开启（内置 {} 词 + 自定义 {} 词）",
            BUILTIN_KEYWORDS.len(),
            custom.len()
        );
        items.push((Some(js), Some(ADBLOCK_CSS.to_string())));
        ADBLOCK_INJECTIONS.fetch_add(1, Ordering::Relaxed);
        // 引擎层网络拦截规则：内置规则 + 自定义关键词（规则变化时会触发重新编译）
        let rules = crate::engine::content_filter::build_rules_with_keywords(&custom);
        crate::engine::content_filter::remember_rules(&rules);
    }
    // 内置隐私防护（三档）：off / standard / strict
    // 兼容早期 privacy_shield='false'（等价 off）
    let level = match take("privacy_shield_level") {
        Some(l) => l,
        None => match take("privacy_shield").as_deref() {
            Some("false") => "off".to_string(),
            _ => "standard".to_string(), // 默认标准档
        },
    };
    // 加噪白名单：命中域名的站点跳过"加噪类"防护（保留 GPC/WebRTC/参数清洗等）
    let whitelist: Vec<String> = take("privacy_whitelist")
        .and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok())
        .unwrap_or_default();
    // 翻译超时（与 translate::current_timeout 同规则）
    let timeout: u64 = take("translate_timeout")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v >= 5 && *v <= 120)
        .unwrap_or(15);
    // 反指纹模式（阶段 2）：noise（加噪，默认）/ uniform（统一化）
    let fp_mode = normalize_fp_mode(take("privacy_fp_mode").as_deref());
    // GPC 开关（2026-10-25 高哥拍板）：CEF 构建默认关（真 Chrome 无此信号，见指纹报告 §7），
    // WebKit 构建默认开。请求头（db/settings.rs）与 JS 属性（privacy.js）共用这一个值。
    let gpc_on = take("gpc_header")
        .map(|v| v != "false")
        .unwrap_or(crate::contracts::settings::GPC_HEADER_DEFAULT == "true");
    let wl_prefix = format!(
        "window.__SHIFT_PRIVACY_WHITELIST__ = {};\nwindow.__SHIFT_TRANSLATE_TIMEOUT = {};\nwindow.__SHIFT_FP_MODE__ = {};\nwindow.__SHIFT_SESSION_KEY__ = {};\nwindow.__SHIFT_UA_BRANDS__ = {};\nwindow.__SHIFT_SITE_OVERRIDES__ = {};\nwindow.__SHIFT_GPC_ON__ = {};\n",
        serde_json::to_string(&whitelist).unwrap_or_else(|_| "[]".to_string()),
        timeout,
        serde_json::to_string(fp_mode).unwrap_or_else(|_| "\"noise\"".to_string()),
        // U-5：进程级会话 key（启动随机生成）→ 反指纹种子 = f(会话 key, origin)
        serde_json::to_string(session_key()).unwrap_or_else(|_| "\"\"".to_string()),
        // Client Hints brands 与 UA 版本对齐（方案 B）：否则"UA 说 Chrome、brands 无 Chrome"被识破
        crate::useragent::brands_json(&crate::useragent::ua_for("")),
        // 站点覆盖表（2026-09-29）：注入全局不分站点，JS 按 location.host 查表
        // 动态对齐 platform/brands（privacy.js 与 compat 脚本消费）
        crate::useragent::overrides_json(),
        // GPC：CEF 构建默认 false → privacy.js 不注入 navigator.globalPrivacyControl
        gpc_on
    );
    match level.as_str() {
        "off" => log::info!("[extensions] 隐私防护已关闭（off）"),
        "strict" => {
            items.push((
                Some(format!(
                    "{}{}\n{}\n{}",
                    wl_prefix, PRIVACY_JS, STRICT_PRIVACY_JS, CONTEXT_TRANSLATE_JS
                )),
                None,
            ));
            PRIVACY_INJECTIONS.fetch_add(1, Ordering::Relaxed);
            log::info!(
                "[extensions] 隐私防护已注入（严格档 strict，白名单 {} 个）+ 右键翻译菜单",
                whitelist.len()
            );
        }
        _ => {
            items.push((
                Some(format!(
                    "{}{}\n{}",
                    wl_prefix, PRIVACY_JS, CONTEXT_TRANSLATE_JS
                )),
                None,
            ));
            PRIVACY_INJECTIONS.fetch_add(1, Ordering::Relaxed);
            log::info!(
                "[extensions] 隐私防护已注入（标准档 standard，白名单 {} 个）+ 右键翻译菜单",
                whitelist.len()
            );
        }
    }
    // WebMCP polyfill 试验开关（阶段 1.5，默认关）
    let webmcp_on = take("webmcp_polyfill")
        .map(|v| v == "true")
        .unwrap_or(false);
    if webmcp_on {
        log::info!("[extensions] WebMCP polyfill 试验已开启");
    }
    Ok((items, webmcp_on))
}

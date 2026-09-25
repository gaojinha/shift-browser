//! 内置拦截规则（纯数据 + 规则 JSON 构造）

/// 内置拦截规则（网络层）：广告网络 / 追踪 / 统计域名。
/// 用简单正则（前后 `.*` 子串匹配）——避免点号转义在 JSON 里产生歧义，
/// 且这些品牌串足够独特，误伤概率极低。
pub(crate) const BUILTIN_URL_RULES: &[&str] = &[
    // 国际广告网络
    ".*doubleclick.*",
    ".*googlesyndication.*",
    ".*googleadservices.*",
    ".*adservice.google.*",
    ".*pagead.*",
    ".*adnxs.*",
    ".*adsrvr.*",
    ".*rubiconproject.*",
    ".*pubmatic.*",
    ".*openx.*",
    ".*casalemedia.*",
    ".*criteo.*",
    ".*taboola.*",
    ".*outbrain.*",
    ".*smartadserver.*",
    ".*amazon-adsystem.*",
    ".*adsafeprotected.*",
    ".*moatads.*",
    ".*serving-sys.*",
    ".*adform.*",
    ".*bidswitch.*",
    ".*sharethrough.*",
    ".*teads.*",
    ".*33across.*",
    ".*gumgum.*",
    ".*smaato.*",
    ".*yieldmo.*",
    ".*360yield.*",
    ".*spotx.*",
    ".*springserve.*",
    // 追踪 / 统计 / 分析
    ".*google-analytics.*",
    ".*googletagmanager.*",
    ".*googletagservices.*",
    ".*scorecardresearch.*",
    ".*quantserve.*",
    ".*quantcount.*",
    ".*hotjar.*",
    ".*clarity.ms.*",
    ".*mixpanel.*",
    ".*segment.io.*",
    ".*amplitude.com.*",
    ".*branch.io.*",
    ".*adjust.com.*",
    ".*appsflyer.*",
    ".*matomo.*",
    ".*piwik.*",
    ".*crazyegg.*",
    ".*mouseflow.*",
    ".*fullstory.*",
    ".*bugsnag.*",
    ".*newrelic.*",
    // 社交像素 / 广告投放
    ".*connect.facebook.*",
    ".*facebook.com/tr.*",
    ".*analytics.twitter.*",
    ".*ads.linkedin.*",
    ".*px.adsrvr.*",
    ".*bat.bing.*",
    // 国内广告 / 统计
    ".*pos.baidu.*",
    ".*cpro.baidu.*",
    ".*cbjs.baidu.*",
    ".*hm.baidu.*",
    ".*union.cnzz.*",
    ".*cnzz.com.*",
    ".*umeng.com.*",
    ".*umengcloud.*",
    ".*mob.alibaba.*",
    ".*tanx.com.*",
    ".*alimama.*",
    ".*mmstat.*",
    ".*admaster.*",
    ".*miaozhen.*",
    ".*talkingdata.*",
    ".*growingio.*",
    ".*sensorsdata.*",
    ".*zhugeio.*",
];

/// 生成 WebKit 内容过滤器规则 JSON（`[{trigger, action}]`，仅内置规则）
#[allow(dead_code)] // 生产路径走 build_rules_with_keywords；此函数供单测与后续自检使用
pub fn build_rules() -> String {
    build_rules_with_keywords(&[])
}

/// 自定义关键词上限（防规则表爆炸）
const MAX_CUSTOM_KEYWORDS: usize = 200;

/// 生成规则 JSON：内置规则 + 用户自定义关键词（关键词也走网络层拦截）
pub fn build_rules_with_keywords(custom: &[String]) -> String {
    let mut patterns: Vec<String> = BUILTIN_URL_RULES.iter().map(|s| s.to_string()).collect();
    for kw in custom.iter().take(MAX_CUSTOM_KEYWORDS) {
        if let Some(p) = keyword_to_pattern(kw) {
            patterns.push(p);
        }
    }
    let mut out = String::with_capacity(patterns.len() * 90 + 2);
    out.push('[');
    for (i, pattern) in patterns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        // pattern 只含 `.` `*` 与字母数字汉字连字符，不会产生 JSON 非法转义
        out.push_str(&format!(
            r#"{{"trigger":{{"url-filter":"{}"}},"action":{{"type":"block"}}}}"#,
            pattern
        ));
    }
    out.push(']');
    out
}

/// 关键词 → 规则模式：只保留字母/数字/汉字与 `.` `-`，剔除其它字符
/// （防止用户输入的正则元字符造成非法规则或过度拦截）
fn keyword_to_pattern(keyword: &str) -> Option<String> {
    let cleaned: String = keyword
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    if cleaned.chars().count() < 2 {
        return None;
    }
    Some(format!(".*{}.*", cleaned))
}

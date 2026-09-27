use super::*;

#[test]
fn rules_json_is_well_formed() {
    let json = build_rules();
    let parsed: serde_json::Value = serde_json::from_str(&json).expect("规则 JSON 必须能被解析");
    let arr = parsed.as_array().expect("规则必须是数组");
    assert_eq!(arr.len(), BUILTIN_URL_RULES.len());
    for item in arr {
        assert_eq!(item["action"]["type"], "block");
        assert!(item["trigger"]["url-filter"].is_string());
    }
}

#[test]
fn rules_use_valid_regex_chars() {
    // 规则以 .* 开头的子串匹配；不含零宽断言等 WebKit 内容过滤器限制的构造
    for p in BUILTIN_URL_RULES {
        assert!(p.starts_with(".*"), "规则应以 .* 开头: {}", p);
        assert!(!p.contains("(?="), "不应包含零宽断言: {}", p);
        assert!(!p.contains("(?!"), "不应包含负向断言: {}", p);
    }
}

#[test]
fn keyword_rules_are_sanitized_and_appended() {
    let json = build_rules_with_keywords(&[
        "广告".to_string(),    // 有效
        "a*b".to_string(),     // 清洗为 ab，长度 2 → 有效
        "x".to_string(),       // 太短 → 丢弃
        "".to_string(),        // 空 → 丢弃
        "(?=bad)".to_string(), // 元字符被剔除，剩余 bad → 有效
    ]);
    let parsed: serde_json::Value =
        serde_json::from_str(&json).expect("带关键词的规则 JSON 必须可解析");
    let arr = parsed.as_array().expect("规则必须是数组");
    // 内置 + 3 个有效关键词
    assert_eq!(arr.len(), BUILTIN_URL_RULES.len() + 3);
    assert!(json.contains(".*广告.*"));
    assert!(json.contains(".*ab.*"));
    // 正则元字符不得进入规则
    assert!(!json.contains("a*b"));
    assert!(!json.contains("?="));
}

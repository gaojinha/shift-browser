use super::*;

#[test]
fn build_js_includes_builtin_and_custom() {
    let js = adblock::build_adblock_js(&["自定义词".to_string()]);
    assert!(js.contains("'关闭广告'"), "应包含内置词");
    assert!(js.contains("'自定义词'"), "应包含自定义词");
    assert!(!js.contains("__SB_KEYWORDS__"), "模板占位符应被替换");
}

#[test]
fn build_js_escapes_quotes_and_dedups() {
    let js = adblock::build_adblock_js(&["it's".to_string(), "关闭广告".to_string()]);
    assert!(js.contains(r"'it\'s'"), "单引号应转义");
    assert_eq!(js.matches("'关闭广告'").count(), 1, "内置词不应重复");
}

#[test]
fn privacy_selftest_standard_all_pass() {
    let page = r#"{"installed":true,"features":{"gpc":true,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":true,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", false, page);
    assert_eq!(r.total, 9);
    assert_eq!(r.passed, 9, "标准档全开应全过");
    assert!(r.checks.iter().all(|c| c.status == "pass"));
}

#[test]
fn privacy_selftest_whitelist_skips_noise_only() {
    let page = r#"{"installed":true,"features":{"gpc":true,"canvas":false,"webgl":false,"audio":false,"hardware":true,"webrtc":true,"webgpu":true,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", true, page);
    assert_eq!(r.passed, 6, "白名单下加噪三项跳过，其余 6 项通过");
    let skip: Vec<&str> = r
        .checks
        .iter()
        .filter(|c| c.status == "skip")
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(skip, vec!["canvas", "webgl", "audio"]);
}

#[test]
fn privacy_selftest_off_all_skip() {
    let page = r#"{"installed":false,"features":{}}"#;
    let r = build_privacy_selftest("off", "example.com", false, page);
    assert!(r.checks.iter().all(|c| c.status == "skip"), "关闭档应全部跳过");
    assert_eq!(r.passed, 0);
}

#[test]
fn privacy_selftest_non_web_all_skip() {
    let page = r#"{"installed":false,"features":{},"protocol":"tauri:"}"#;
    let r = build_privacy_selftest("standard", "", false, page);
    assert!(
        r.checks.iter().all(|c| c.status == "skip"),
        "非网页标签（tauri://）应全部跳过，不误报失败"
    );
    assert_eq!(r.passed, 0);
}

#[test]
fn whitelist_matches_domain_and_subdomain() {
    let wl = vec!["example.com".to_string(), "*.foo.cn".to_string()];
    assert!(is_whitelisted("example.com", &wl));
    assert!(is_whitelisted("www.example.com", &wl));
    assert!(is_whitelisted("a.b.foo.cn", &wl));
    assert!(!is_whitelisted("notexample.com", &wl));
    assert!(!is_whitelisted("", &wl));
}

#[test]
fn normalize_fp_mode_rules() {
    assert_eq!(normalize_fp_mode(Some("uniform")), "uniform");
    assert_eq!(normalize_fp_mode(Some("noise")), "noise");
    // 非法值/缺失一律回落 noise（默认加噪）
    assert_eq!(normalize_fp_mode(Some("bogus")), "noise");
    assert_eq!(normalize_fp_mode(None), "noise");
}

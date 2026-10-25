use super::*;

#[test]
fn session_key_is_non_empty_and_stable_within_process() {
    let k1 = adblock::session_key();
    let k2 = adblock::session_key();
    assert!(!k1.is_empty(), "会话 key 不应为空");
    assert_eq!(k1, k2, "同一进程生命周期内 session_key 应恒定");
    // simple uuid 格式为 32 位 hex
    assert_eq!(k1.len(), 32, "会话 key 应为 32 字符的 UUID Simple 字符串");
}

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
    let page = r#"{"installed":true,"features":{"gpc":true,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":true,"webgpu_engine":true,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", false, page);
    assert_eq!(r.total, 9);
    assert_eq!(r.passed, 9, "标准档全开应全过");
    assert!(r.checks.iter().all(|c| c.status == "pass"));
}

#[test]
fn privacy_selftest_gpc_off_is_skip_not_fail() {
    // F-CEF-07：CEF 构建默认关 GPC（真 Chrome 无此信号）→ 自测应标 skip 而非 fail，
    // 否则用户会以为防护坏了。其余 8 项全过时直方图应是 8 pass / 1 skip。
    let page = r#"{"installed":true,"features":{"gpc":false,"gpc_on":false,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":true,"webgpu_engine":true,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", false, page);
    assert_eq!(r.total, 9);
    assert_eq!(r.passed, 8, "GPC 关闭时其余 8 项应全过");
    let g = r.checks.iter().find(|c| c.id == "gpc").expect("应有 gpc 项");
    assert_eq!(g.status, "skip", "GPC 关闭属预期行为，应标 skip");
    assert!(
        g.detail.contains("与真 Chrome"),
        "应说明关闭是为了与真 Chrome 一致：{}",
        g.detail
    );
}

#[test]
fn privacy_selftest_gpc_field_missing_treated_as_on() {
    // 向后兼容：旧注入脚本不带 gpc_on 字段时按「开」处理，走原有 pass/fail 判定
    let page = r#"{"installed":true,"features":{"gpc":true,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":true,"webgpu_engine":true,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", false, page);
    let g = r.checks.iter().find(|c| c.id == "gpc").expect("应有 gpc 项");
    assert_eq!(g.status, "pass", "缺字段应按开处理");
}

#[test]
fn privacy_selftest_webgpu_engine_unsupported_is_skip_not_fail() {
    // WebKitGTK 无 WebGPU 实现：引擎不支持≠防护失效，必须标 skip（➖）而非 fail（❌），
    // 否则用户会误以为防护坏了；其余 8 项不受影响。
    let page = r#"{"installed":true,"features":{"gpc":true,"canvas":true,"webgl":true,"audio":true,"hardware":true,"webrtc":true,"webgpu":false,"webgpu_engine":false,"misc":true}}"#;
    let r = build_privacy_selftest("standard", "example.com", false, page);
    assert_eq!(r.total, 9);
    assert_eq!(r.passed, 8);
    let w = r
        .checks
        .iter()
        .find(|c| c.id == "webgpu")
        .expect("应有 webgpu 项");
    assert_eq!(w.status, "skip");
    assert!(
        w.detail.contains("引擎不支持"),
        "应说明引擎不支持：{}",
        w.detail
    );
    assert!(
        r.checks.iter().all(|c| c.status != "fail"),
        "引擎不支持时不得有 fail 项：{:?}",
        r.checks
    );
}

#[test]
fn privacy_selftest_whitelist_is_full_compat_bypass() {
    // 2026-09-26 起白名单 = 完全兼容模式：privacy.js 装任何防护前整体 return，
    // 页面上报的 features 应全为 false；自测只能有 inject 一项 pass，其余 8 项
    // 全部 skip（不能出现 fail，否则用户会以为防护坏了）。
    let page = r#"{"installed":true,"features":{"gpc":false,"canvas":false,"webgl":false,"audio":false,"hardware":false,"webrtc":false,"webgpu":false,"misc":false}}"#;
    let r = build_privacy_selftest("standard", "upwork.com", true, page);
    assert_eq!(r.total, 9);
    assert_eq!(r.passed, 1, "兼容模式仅 inject 通过");
    assert!(
        r.checks.iter().all(|c| c.status != "fail"),
        "白名单帧不得有 fail 项：{:?}",
        r.checks
    );
    let skip: Vec<&str> = r
        .checks
        .iter()
        .filter(|c| c.status == "skip")
        .map(|c| c.id.as_str())
        .collect();
    // 注意顺序：webgpu 单列判定挪到 hardware/webrtc/misc 循环之后（引擎不支持分支）
    assert_eq!(
        skip,
        vec!["gpc", "canvas", "webgl", "audio", "hardware", "webrtc", "misc", "webgpu"]
    );
}

#[test]
fn privacy_selftest_off_all_skip() {
    let page = r#"{"installed":false,"features":{}}"#;
    let r = build_privacy_selftest("off", "example.com", false, page);
    assert!(
        r.checks.iter().all(|c| c.status == "skip"),
        "关闭档应全部跳过"
    );
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

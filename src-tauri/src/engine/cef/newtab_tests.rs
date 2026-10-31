use super::{ensure_newtab_html, parse_site, render_newtab_html, QuickSite, RecentItem};

#[test]
fn newtab_html_written_and_self_contained() {
    let dir = std::env::temp_dir().join(format!("cef-newtab-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let url = ensure_newtab_html(&dir).expect("写启动页失败");
    assert!(url.ends_with("cef-newtab.html"));
    let disk = std::path::Path::new(url.strip_prefix("file://").expect("前缀"));
    assert!(disk.exists(), "启动页未落盘: {url}");
    let body = std::fs::read_to_string(disk).unwrap();
    assert!(body.contains("id=\"q\""), "必须有搜索框");
    assert!(!body.contains("__TAURI_INTERNALS__"), "不得依赖 Tauri IPC");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_newtab_injects_search_and_sites() {
    let sites = vec![
        QuickSite {
            icon: "🔍".into(),
            name: "必应".into(),
            url: "https://www.bing.com".into(),
        },
        QuickSite {
            icon: "📖".into(),
            name: "a</script>".into(),
            url: "https://a.com".into(),
        },
    ];
    let recent = vec![RecentItem {
        title: "示例".into(),
        url: "https://example.com".into(),
    }];
    let html = render_newtab_html("https://www.bing.com/search?q=", &sites, &recent);
    assert!(
        html.contains("\"https://www.bing.com/search?q=\""),
        "搜索模板必须注入"
    );
    assert!(html.contains("必应"));
    assert!(
        html.contains("a<\\/script>"),
        "</ 必须转义，防提前闭合 script"
    );
    assert!(!html.contains("a</script>"));
    assert!(!html.contains("__SEARCH_URL__"));
    assert!(!html.contains("__QUICK_SITES__"));
    assert!(
        html.contains("\"url\":\"https://example.com\""),
        "最近访问必须注入"
    );
    assert!(!html.contains("__RECENT__"));
}

#[test]
fn parse_site_rejects_non_web_schemes() {
    let ok = serde_json::json!({"icon":"x","name":"站点","url":"https://example.com"});
    assert!(parse_site(&ok).is_some());
    let bad = serde_json::json!({"icon":"x","name":"坏","url":"javascript:alert(1)"});
    assert!(parse_site(&bad).is_none());
    let no_url = serde_json::json!({"icon":"x","name":"缺 URL"});
    assert!(parse_site(&no_url).is_none());
}
